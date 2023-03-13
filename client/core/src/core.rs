use async_condvar_fair::Condvar;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::hash_vectors::{CommonPayload, HashVectors, RecipientPayload};
use crate::olm_wrapper::OlmWrapper;
use crate::server_comm::{
    Batch, Event, IncomingMessage, OutgoingMessage, Payload, ServerComm,
    ToDelete,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct FullPayload {
    common: CommonPayload,
    per_recipient: RecipientPayload,
}

impl FullPayload {
    fn new(
        common: CommonPayload,
        per_recipient: RecipientPayload,
    ) -> FullPayload {
        Self {
            common,
            per_recipient,
        }
    }

    fn to_string(
        common: CommonPayload,
        per_recipient: RecipientPayload,
    ) -> String {
        serde_json::to_string(&FullPayload::new(common.clone(), per_recipient))
            .unwrap()
    }

    fn from_string(msg: String) -> FullPayload {
        serde_json::from_str(msg.as_str()).unwrap()
    }
}

#[async_trait]
pub trait CoreClient: Sync + Send + 'static {
    async fn client_callback(&self, sender: String, message: String);
}

pub struct Core<C: CoreClient> {
    olm_wrapper: OlmWrapper,
    server_comm: RwLock<Option<ServerComm<C>>>,
    hash_vectors: Mutex<HashVectors>,
    client: RwLock<Option<Arc<C>>>,
    init: parking_lot::Mutex<bool>,
    init_cv: Condvar,
}

impl<C: CoreClient> Core<C> {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off_arg: bool,
        client: Option<Arc<C>>,
    ) -> Arc<Core<C>> {
        let olm_wrapper = OlmWrapper::new(turn_encryption_off_arg);
        let idkey = olm_wrapper.get_idkey();
        let hash_vectors = Mutex::new(HashVectors::new(idkey.clone()));

        // Core needs to effectively register itself as a client of
        // server_comm (no trait needed b/c only one implementation
        // will ever be used, at least at this point) - which is why
        // Core::new() should return Arc<Core<C>>

        let arc_core = Arc::new(Core {
            olm_wrapper,
            server_comm: RwLock::new(None),
            hash_vectors,
            client: RwLock::new(client),
            init: parking_lot::Mutex::new(false),
            init_cv: Condvar::new(),
        });

        {
            let mut server_comm_guard = arc_core.server_comm.write().await;
            let server_comm = ServerComm::new(
                ip_arg,
                port_arg,
                idkey.clone(),
                Some(arc_core.clone()),
            )
            .await;
            *server_comm_guard = Some(server_comm);
        }

        arc_core
    }

    pub async fn set_client(&self, client: Arc<C>) {
        *self.client.write().await = Some(client);
    }

    pub async fn unset_client(&self) {
        *self.client.write().await = None;
    }

    pub fn idkey(&self) -> String {
        self.olm_wrapper.get_idkey()
    }

    pub async fn send_message(
        &self,
        dst_idkeys: Vec<String>,
        payload: &String,
    ) -> reqwest::Result<reqwest::Response> {
        loop {
            let init = self.init.lock();
            if !*init {
                let _ = self.init_cv.wait(init).await;
            } else {
                break;
            }
        }

        let (common_payload, recipient_payloads) = self
            .hash_vectors
            .lock()
            .await
            .prepare_message(dst_idkeys.clone(), payload.to_string());
        let mut batch = Batch::new();
        for (idkey, recipient_payload) in recipient_payloads {
            let full_payload = FullPayload::to_string(
                common_payload.clone(),
                recipient_payload,
            );

            let (c_type, ciphertext) = self
                .olm_wrapper
                .encrypt(
                    &self.server_comm.read().await.as_ref().unwrap(),
                    &idkey,
                    &full_payload,
                )
                .await;

            batch.push(OutgoingMessage::new(
                idkey,
                Payload::new(c_type, ciphertext),
            ));
        }
        self.server_comm
            .read()
            .await
            .as_ref()
            .unwrap()
            .send_message(&batch)
            .await
    }

    pub async fn server_comm_callback(
        &self,
        event: eventsource_client::Result<Event>,
    ) {
        match event {
            Err(err) => panic!("err: {:?}", err),
            Ok(Event::Otkey) => {
                let otkeys = self.olm_wrapper.generate_otkeys(None);
                // FIXME is this read()...await ok??
                // had to use tokio's RwLock instead of std's in order to
                // make this Send
                match self
                    .server_comm
                    .read()
                    .await
                    .as_ref()
                    .unwrap()
                    .add_otkeys_to_server(&otkeys.curve25519())
                    .await
                {
                    Ok(_) => {} //println!("Sent otkeys successfully"),
                    Err(err) => panic!("Error sending otkeys: {:?}", err),
                }
                // set init = true and notify init_cv waiters
                let mut init = self.init.lock();
                if !*init {
                    *init = true;
                    self.init_cv.notify_all();
                }
            }
            Ok(Event::Msg(msg_string)) => {
                let msg: IncomingMessage =
                    IncomingMessage::from_string(msg_string);

                let decrypted = self.olm_wrapper.decrypt(
                    &msg.sender(),
                    msg.payload().c_type(),
                    &msg.payload().ciphertext(),
                );

                let full_payload = FullPayload::from_string(decrypted);

                // validate
                match self.hash_vectors.lock().await.parse_message(
                    &msg.sender(),
                    full_payload.common,
                    &full_payload.per_recipient,
                ) {
                    Ok(None) => {
                        //println!("Validation succeeded,
                        // no message to process")
                    }
                    Ok(Some((seq, message))) => {
                        // forward message to CoreClient
                        self.client
                            .read()
                            .await
                            .as_ref()
                            .unwrap()
                            .client_callback(msg.sender().clone(), message)
                            .await;

                        match self
                            .server_comm
                            .read()
                            .await
                            .as_ref()
                            .unwrap()
                            .delete_messages_from_server(
                                &ToDelete::from_seq_id(seq.try_into().unwrap()),
                            )
                            .await
                        {
                            Ok(_) => {
                                //println!("Sent
                                // delete-message
                                // successfully")
                            }
                            Err(err) => panic!(
                                "Error sending delete-message: {:?}",
                                err
                            ),
                        }
                    }
                    Err(err) => panic!("Validation failed: {:?}", err),
                }
            }
        }
    }

    // FIXME make immutable
    // self.olm_wrapper.need_mut_ref()
    // e.g. wrap olm_wrapper w Mutex (for now)
    // rule: never put a thing into a Mutex which calls some
    // async functions only wrap types that are used
    // briefly, and make sure you unlock() before
    // calling any asyncs
    // TODO also, between unlock() and lock(), may have to
    // recalculate any common vars to use
    pub async fn receive_message(&mut self) {
        unimplemented!()
    }
}

pub mod stream_client {
    use crate::core::CoreClient;
    use async_trait::async_trait;
    use futures::channel::mpsc::{channel, Receiver, Sender};

    pub struct StreamClient {
        sender: tokio::sync::Mutex<Sender<(String, String)>>,
    }

    pub struct StreamClientReceiver {
        receiver: Receiver<(String, String)>,
    }

    impl StreamClient {
        pub fn new() -> (Self, StreamClientReceiver) {
            let (sender, receiver) = channel::<(String, String)>(5);

            (
                StreamClient {
                    sender: tokio::sync::Mutex::new(sender),
                },
                StreamClientReceiver { receiver },
            )
        }
    }

    #[async_trait]
    impl CoreClient for StreamClient {
        async fn client_callback(&self, sender: String, message: String) {
            use futures::SinkExt;
            self.sender
                .lock()
                .await
                .send((sender, message))
                .await
                .unwrap();
        }
    }

    impl futures::stream::Stream for StreamClientReceiver {
        type Item = (String, String);

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::pin::Pin::new(&mut self.receiver).poll_next(cx)
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            self.receiver.size_hint()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::stream_client::StreamClient;
    use crate::core::Core;
    use futures::StreamExt;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_send_message_to_self_only() {
        let (client, mut receiver) = StreamClient::new();
        let arc_client = Arc::new(client);
        let arc_core: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client)).await;

        let payload = String::from("hello from me");
        let idkey = arc_core.olm_wrapper.get_idkey();
        let recipients = vec![idkey.clone()];

        if let Err(err) = arc_core.send_message(recipients, &payload).await {
            panic!("Error sending message: {:?}", err);
        }

        match receiver.next().await {
            Some((sender, msg)) => {
                assert_eq!(sender, idkey);
                assert_eq!(msg, payload);
            }
            None => panic!("got NONE from core"),
        };
    }

    #[tokio::test]
    async fn test_send_message_to_self_and_others() {
        let (client_a, mut receiver_a) = StreamClient::new();
        let arc_client_a = Arc::new(client_a);
        let arc_core_a: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_a)).await;
        let idkey_a = arc_core_a.olm_wrapper.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b)).await;
        let idkey_b = arc_core_b.olm_wrapper.get_idkey();

        let payload = String::from("hello from me");
        let recipients = vec![idkey_a.clone(), idkey_b.clone()];

        if let Err(err) = arc_core_a.send_message(recipients, &payload).await {
            panic!("Error sending message: {:?}", err);
        }

        match receiver_a.next().await {
            Some((sender, msg)) => {
                assert_eq!(sender, idkey_a);
                assert_eq!(msg, payload);
            }
            None => panic!("a got NONE from core"),
        }

        match receiver_b.next().await {
            Some((sender, msg)) => {
                assert_eq!(sender, idkey_a);
                assert_eq!(msg, payload);
            }
            None => panic!("b got NONE from core"),
        }
    }

    #[tokio::test]
    async fn test_send_message_to_others_only() {
        let (client_a, _receiver_a) = StreamClient::new();
        let arc_client_a = Arc::new(client_a);
        let arc_core_a: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_a)).await;
        let idkey_a = arc_core_a.olm_wrapper.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b)).await;
        let idkey_b = arc_core_b.olm_wrapper.get_idkey();

        let payload = String::from("hello from me");
        let recipients = vec![idkey_b.clone()];

        if let Err(err) = arc_core_a.send_message(recipients, &payload).await {
            panic!("Error sending message: {:?}", err);
        }

        match receiver_b.next().await {
            Some((sender, msg)) => {
                assert_eq!(sender, idkey_a);
                assert_eq!(msg, payload);
            }
            None => panic!("b got NONE from core"),
        }
    }
}
