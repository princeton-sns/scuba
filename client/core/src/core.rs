use futures::channel::mpsc;
use reqwest::{Response, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};

use crate::hash_vectors::{CommonPayload, HashVectors, RecipientPayload};
use crate::olm_wrapper::OlmWrapper;
use crate::server_comm::{
    Batch, Event, IncomingMessage, OutgoingMessage, Payload, ServerComm, ToDelete,
};

// TODO persist natively

#[derive(Debug, Serialize, Deserialize)]
pub struct FullPayload {
    common: CommonPayload,
    per_recipient: RecipientPayload,
}

impl FullPayload {
    fn new(common: CommonPayload, per_recipient: RecipientPayload) -> FullPayload {
        Self {
            common,
            per_recipient,
        }
    }

    fn to_string(common: CommonPayload, per_recipient: RecipientPayload) -> String {
        serde_json::to_string(&FullPayload::new(common.clone(), per_recipient)).unwrap()
    }

    fn from_string(msg: String) -> FullPayload {
        serde_json::from_str(msg.as_str()).unwrap()
    }

    fn common(&self) -> &CommonPayload {
        &self.common
    }

    fn per_recipient(&self) -> &RecipientPayload {
        &self.per_recipient
    }
}

pub trait CoreClient: Sync + Send + 'static {
    async fn client_callback(&self, sender: String, message: String);
}

pub struct Core<C: CoreClient> {
    olm_wrapper: OlmWrapper,
    server_comm: RwLock<Option<ServerComm<C>>>,
    hash_vectors: Mutex<HashVectors>,
    client: RwLock<Option<Arc<C>>>,
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

        let arc_core = Arc::new(Core {
            olm_wrapper,
            server_comm: RwLock::new(None),
            hash_vectors,
            client: RwLock::new(client),
        });

        {
            let mut server_comm_guard = arc_core.server_comm.write().unwrap();
            let server_comm =
                ServerComm::new(ip_arg, port_arg, idkey.clone(), Some(arc_core.clone())).await;
            *server_comm_guard = Some(server_comm);
        }

        arc_core
    }

    pub fn set_client(&self, client: Arc<C>) {
        *self.client.write().unwrap() = Some(client);
    }

    pub fn unset_client(&self) {
        *self.client.write().unwrap() = None;
    }

    pub fn idkey(&self) -> String {
        self.olm_wrapper.get_idkey()
    }

    // TODO register_listener
    // probably not a callback, probably a CoreClient
    // C: CoreClient, take an Arc<CoreClient>
    // TODO message functions => by implemting a CoreClient trait
    // TODO shim Client that forces waiting for messages

    // TODO core should also register itself as a ServerCommClient
    // in which case, Core::new() should return Arc<Core<C>>

    pub async fn send_message(
        &self,
        dst_idkeys: Vec<String>,
        payload: &String,
    ) -> Result<Response> {
        let (common_payload, recipient_payloads) = self
            .hash_vectors
            .lock()
            .unwrap()
            .prepare_message(dst_idkeys.clone(), payload.to_string());
        let mut batch = Batch::new();
        for (idkey, recipient_payload) in recipient_payloads {
            let full_payload = FullPayload::to_string(common_payload.clone(), recipient_payload);

            let (c_type, ciphertext) = self
                .olm_wrapper
                .encrypt(
                    &self.server_comm.read().unwrap().as_ref().unwrap(),
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
            .unwrap()
            .as_ref()
            .unwrap()
            .send_message(&batch)
            .await
    }

    pub async fn server_comm_callback(&self, event: eventsource_client::Result<Event>) {
        println!("in server_comm_callback");
        match event {
            Err(err) => println!("err: {:?}", err),
            Ok(Event::Otkey) => {
                println!("otkey event");
                //let otkeys = self.olm_wrapper.generate_otkeys(None);
                //let server_comm_read_guard = self
                //    .server_comm
                //    .read()
                //    .unwrap();
                //match server_comm_read_guard
                //    .as_ref()
                //    .unwrap()
                //    .add_otkeys_to_server(&otkeys.curve25519())
                //    .await
                //{
                //    Ok(_) => println!("Sent otkeys successfully"),
                //    Err(err) => panic!("Error sending otkeys: {:?}", err),
                //}
            }
            Ok(Event::Msg(_)) => {
                println!("msg event");
                //self.client.lock().map(|client| {
                //    client.client_callback();
                //});
            }
        }
    }

    // FIXME make immutable
    // self.olm_wrapper.need_mut_ref()
    // e.g. wrap olm_wrapper w Mutex (for now)
    // rule: never put a thing into a Mutex which calls some async functions
    // only wrap types that are used briefly, and make sure you unlock() before
    // calling any asyncs
    // TODO also, between unlock() and lock(), may have to recalculate any
    // common vars to use
    pub async fn receive_message(&mut self) {
        unimplemented!()
        // use futures::TryStreamExt;

        // // FIXME
        // match self.server_comm().try_next().await {
        //     Ok(Some(Event::Msg(msg_string))) => {
        //         let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

        //         let decrypted = self.olm_wrapper.decrypt(
        //             &msg.sender(),
        //             msg.payload().c_type(),
        //             &msg.payload().ciphertext(),
        //         );

        //         let full_payload = FullPayload::from_string(decrypted);

        //         // validate
        //         match self.hash_vectors.lock().unwrap().parse_message(
        //             &msg.sender(),
        //             full_payload.common,
        //             &full_payload.per_recipient,
        //         ) {
        //             Ok(None) => println!("Validation succeeded, no message to process"),
        //             Ok(Some((seq, message))) => {
        //                 // forward message
        //                 // FIXME are callbacks easier to compile to wasm?
        //                 self.sender.try_send((msg.sender().clone(), message));

        //                 match self
        //                     .server_comm
        //                     .read().unwrap().as_ref().unwrap()
        //                     .delete_messages_from_server(&ToDelete::from_seq_id(
        //                         seq.try_into().unwrap(),
        //                     ))
        //                     .await
        //                 {
        //                     Ok(_) => println!("Sent delete-message successfully"),
        //                     Err(err) => panic!("Error sending delete-message: {:?}", err),
        //                 }
        //             }
        //             Err(err) => panic!("Validation failed: {:?}", err),
        //         }
        //     }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{Core, CoreClient, FullPayload};
    use crate::server_comm::{Event, IncomingMessage, ToDelete};
    use futures::channel::mpsc::{channel, Receiver, Sender};
    use futures::StreamExt;
    use std::sync::Arc;

    const BUFFER_SIZE: usize = 20;

    pub struct StreamClient {
        sender: tokio::sync::Mutex<Sender<(String, String)>>,
    }

    // TODO what is being received here?
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

    impl CoreClient for StreamClient {
        async fn client_callback(&self, sender: String, message: String) {
            use futures::SinkExt;
            println!("in client_callback");
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

    #[tokio::test]
    async fn test_new() {
        let (client, mut receiver) = StreamClient::new();
        let arc_client = Arc::new(client);
        let arc_core: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client)).await;

        match receiver.next().await {
            Some((sender, msg)) => {
                println!("got SOME from core");
                println!("sender: {:?}", sender);
                println!("msg: {:?}", msg);
            }
            None => println!("got NONE from core"),
        };
        //|(sender, msg)| {
        //  println!("sender: ", sender);
        //  println!("msg: ",  msg);
        //});
    }

    /*
    #[tokio::test]
    async fn test_send_message_to_self() {
        let payload = String::from("hello from me");
        let (sender, _) = channel::<(String, String)>(BUFFER_SIZE);
        let mut core = Core::new(None, None, false, sender).await;
        let idkey = core.olm_wrapper.get_idkey();
        let recipients = vec![idkey];

        match core.send_message(recipients, &payload).await {
            Ok(_) => match core.server_comm.read().unwrap().as_ref().unwrap().try_next().await {
                Ok(Some(Event::Msg(msg_string))) => {
                    let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

                    let decrypted = core.olm_wrapper.decrypt(
                        &msg.sender(),
                        msg.payload().c_type(),
                        &msg.payload().ciphertext(),
                    );

                    let full_payload = FullPayload::from_string(decrypted);
                    assert_eq!(*full_payload.common().message(), payload);

                    match core
                        .server_comm
                        .delete_messages_from_server(&ToDelete::from_seq_id(msg.seq_id()))
                        .await
                    {
                        Ok(_) => println!("Sent delete-message successfully"),
                        Err(err) => panic!("Error sending delete-message: {:?}", err),
                    }
                }
                Ok(Some(Event::Otkey)) => panic!("FAIL got otkey event"),
                Ok(None) => panic!("FAIL got none"),
                Err(err) => panic!("FAIL got error: {:?}", err),
            },
            Err(err) => panic!("Error sending message: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_send_message_to_other() {
        let payload = String::from("hello from me");
        let (sender, _) = channel::<(String, String)>(BUFFER_SIZE);
        let core_0 = Core::new(None, None, false, sender.clone()).await;
        let mut core_1 = Core::new_and_init(None, None, false, sender).await;
        let idkey_1 = core_1.olm_wrapper.get_idkey();
        let recipients = vec![idkey_1];

        match core_0.send_message(recipients, &payload).await {
            Ok(_) => match core_1.server_comm.try_next().await {
                Ok(Some(Event::Msg(msg_string))) => {
                    let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

                    let decrypted = core_1.olm_wrapper.decrypt(
                        &msg.sender(),
                        msg.payload().c_type(),
                        &msg.payload().ciphertext(),
                    );

                    let full_payload = FullPayload::from_string(decrypted);
                    assert_eq!(*full_payload.common().message(), payload);
                }
                Ok(Some(Event::Otkey)) => panic!("FAIL got otkey event"),
                Ok(None) => panic!("FAIL got none"),
                Err(err) => panic!("FAIL got error: {:?}", err),
            },
            Err(err) => panic!("error: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_handle_events() {
        let payload = String::from("hello from me");
        let (sender, mut receiver) = channel::<(String, String)>(BUFFER_SIZE);
        let mut core = Core::new(None, None, false, sender.clone()).await;
        // otkey
        core.receive_message().await;
        let idkey = core.olm_wrapper.get_idkey();
        let recipients = vec![idkey.clone()];

        match core.send_message(recipients, &payload).await {
            Ok(_) => println!("Message sent"),
            Err(err) => panic!("Error sending message: {:?}", err),
        }

        core.receive_message().await;

        match receiver.try_next().unwrap() {
            Some((sender, recv_payload)) => {
                assert_eq!(sender, idkey);
                assert_eq!(payload, recv_payload);
            }
            None => panic!("Got no message"),
        }
    }*/
}
