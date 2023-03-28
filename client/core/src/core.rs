use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::{Condvar, Mutex, RwLock};

use crate::crypto::Crypto;
use crate::hash_vectors::{CommonPayload, HashVectors, ValidationPayload};
use crate::server_comm::{
    Batch, EncryptedCommonPayload, EncryptedPerRecipientPayload, Event,
    IncomingMessage, OutgoingMessage, ServerComm, ToDelete,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct PerRecipientPayload {
    val_payload: ValidationPayload,
    key: [u8; 16],
    iv: [u8; 16],
}

impl PerRecipientPayload {
    fn new(
        val_payload: ValidationPayload,
        key: [u8; 16],
        iv: [u8; 16],
    ) -> PerRecipientPayload {
        Self {
            val_payload,
            key,
            iv,
        }
    }

    fn key(&self) -> [u8; 16] {
        self.key
    }

    fn iv(&self) -> [u8; 16] {
        self.iv
    }

    fn to_string(
        val_payload: ValidationPayload,
        key: [u8; 16],
        iv: [u8; 16],
    ) -> String {
        serde_json::to_string(&PerRecipientPayload::new(val_payload, key, iv))
            .unwrap()
    }

    fn from_string(per_recipient_payload: String) -> PerRecipientPayload {
        serde_json::from_str(per_recipient_payload.as_str()).unwrap()
    }
}

pub trait CoreClient: Sync + Send + 'static {
    fn client_callback(&self, sender: String, message: String);
}

pub struct Core<C: CoreClient> {
    crypto: Crypto,
    server_comm: RwLock<Option<ServerComm>>,
    hash_vectors: Mutex<HashVectors>,
    client: RwLock<Option<Arc<C>>>,
    init: Mutex<bool>,
    init_cv: Condvar,
    outgoing_queue: Arc<Mutex<VecDeque<CommonPayload>>>,
    incoming_queue: Arc<Mutex<VecDeque<CommonPayload>>>,
    oq_cv: Condvar,
    iq_cv: Condvar,
}

impl<C: CoreClient> Core<C> {
    pub fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off: bool,
        client: Option<Arc<C>>,
    ) -> Arc<Core<C>> {
        let crypto = Crypto::new(turn_encryption_off);
        let idkey = crypto.get_idkey();
        let hash_vectors = Mutex::new(HashVectors::new(idkey.clone()));

        // Core needs to effectively register itself as a client of
        // server_comm (no trait needed b/c only one implementation
        // will ever be used, at least at this point) - which is why
        // Core::new() should return Arc<Core<C>>

        let arc_core = Arc::new(Core {
            crypto,
            server_comm: RwLock::new(None),
            hash_vectors,
            client: RwLock::new(client),
            init: Mutex::new(false),
            init_cv: Condvar::new(),
            outgoing_queue: Arc::new(Mutex::new(
                VecDeque::<CommonPayload>::new(),
            )),
            incoming_queue: Arc::new(Mutex::new(
                VecDeque::<CommonPayload>::new(),
            )),
            oq_cv: Condvar::new(),
            iq_cv: Condvar::new(),
        });

        {
            let mut server_comm_guard = arc_core.server_comm.write().unwrap();
            let arc_core_copy = arc_core.clone();
            let callback =
                Box::new(move |event| {
                    arc_core_copy.server_comm_callback(event)
                });
            let server_comm = ServerComm::new(
                ip_arg,
                port_arg,
                idkey.clone(),
                callback,
            );
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
        self.crypto.get_idkey()
    }

    pub fn send_message(
        &self,
        dst_idkeys: Vec<String>,
        payload: &String,
    ) -> reqwest::Result<reqwest::blocking::Response> {
        /* TODO: remove
         * loop {
            let init = self.init.lock();
            if !*init {
                let _ = self.init_cv.wait(init).unwrap();
            } else {
                break;
            }
        }*/

        //println!("");
        //println!("---sending message ({:?})", self.idkey());
        //println!("");
        //println!("...TRYING SEND LOCK...");
        let mut hash_vectors_guard = self.hash_vectors.lock().unwrap();
        //println!("...GOT SEND LOCK...");
        let (common_payload, val_payloads) = hash_vectors_guard
            .prepare_message(dst_idkeys.clone(), payload.to_string());

        // FIXME What if common_payloads are identical?
        // If they're identical here, they can trigger a reordering detection,
        // but if they're identical upon reception, they won't negatively affect
        // the application (since they're identical).

        // Worst case, we add a sequence number to differentiate between
        // identical outgoing common_payloads.

        // However, as long as clients .await on their send_message() function,
        // will there ever be a case where messages get reordered sending-side?
        // Unless the client is multithreaded, I don't think so

        // add to outgoing_queue before releasing lock
        self.outgoing_queue
            .lock()
            .unwrap()
            .push_back(common_payload.clone());
        //println!("-----ADDING CP TO OQ: {:?}", common_payload.clone());

        core::mem::drop(hash_vectors_guard);
        //println!("...UNLOCKED SEND...");

        // symmetrically encrypt common_payload once
        let (common_ct, key, iv) =
            self.crypto.symmetric_encrypt(common_payload.clone());

        let mut batch = Batch::new();

        // TODO loop to encrypt key + iv + val_payload per client
        for (idkey, val_payload) in val_payloads {
            let per_recipient_payload =
                PerRecipientPayload::to_string(val_payload, key, iv);

            let (c_type, per_recipient_ct) = self
                .crypto
                .encrypt(
                    &self.server_comm.read().unwrap().as_ref().unwrap(),
                    &idkey,
                    &per_recipient_payload,
                );

            batch.push(OutgoingMessage::new(
                idkey,
                EncryptedCommonPayload::new(common_ct.clone()),
                EncryptedPerRecipientPayload::new(c_type, per_recipient_ct),
            ));
        }

        // loop until front of queue is ready to send
        loop {
            let mut oq_guard = self.outgoing_queue.lock().unwrap();
            //println!("-----SEND LOOP");
            //println!("oq_guard.front(): {:?}", oq_guard.front());
            if oq_guard.front() != Some(&common_payload) {
                let _ = self.oq_cv.wait(oq_guard);
            } else {
                //println!("~~POPPING~~");
                oq_guard.pop_front();
                break;
            }
        }

        self.server_comm
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .send_message(&batch)
    }

    pub fn server_comm_callback(
        &self,
        event: Event,
    ) {
        //println!("");
        //println!("---receiving message ({:?})", self.idkey());
        //println!("");
        match event {
            Event::Otkey => {
                let otkeys = self.crypto.generate_otkeys(None);
                match self
                    .server_comm
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .add_otkeys_to_server(&otkeys.curve25519())
                {
                    Ok(_) => {}
                    Err(err) => panic!("Error sending otkeys: {:?}", err),
                }
                /* TODO: remove
                // set init = true and notify init_cv waiters
                let mut init = self.init.lock();
                if !*init {
                    *init = true;
                    self.init_cv.notify_all();
                }*/
            }
            Event::Msg(msg_string) => {
                let msg: IncomingMessage =
                    IncomingMessage::from_string(msg_string);

                let decrypted_per_recipient = self.crypto.decrypt(
                    &msg.sender(),
                    msg.enc_per_recipient().c_type(),
                    &msg.enc_per_recipient().ciphertext(),
                );

                let per_recipient_payload =
                    PerRecipientPayload::from_string(decrypted_per_recipient);
                let decrypted_common = self.crypto.symmetric_decrypt(
                    msg.enc_common().byte_vec().clone(),
                    per_recipient_payload.key(),
                    per_recipient_payload.iv(),
                );
                let common_payload =
                    CommonPayload::from_string(decrypted_common);

                // If an incoming message is to be forwarded to the client
                // callback, the lock on hash_vectors below is not released
                // until _after_ the client callback finishes (technically,
                // it is not even released until the
                // deleted_messages_from_server() function returns). If the
                // client callback tries to send a message (e.g. when linking a
                // device, the client callback will receive an UpdateLinked
                // message and subsequently try to reply with a
                // ConfirmUpdateLinked message), the code will deadlock because
                // hash_vectors lock is still held by the line below.
                // tokio::sync doesn't provide a function for unlocking the
                // Mutex, but the Mutex needs to be asynchronous b/c when
                // sending a message, encrypt() is async, and the lock needs (?)
                // to be held across that .await point.

                // Actually, the worst thing that (I think) can happen if the
                // mutex is _sync_ is that messages will be reordered sending
                // side? Which violates sender-side ordering (translated to
                // real-time ordering).

                // Is this valid? Proof by contradiction: Say client A start to
                // send message X, meaning it updates its pending messages list
                // and prepares to send along its current hash_vector head state
                // (lets call this hvX). Then the thread doing this work yields
                // at the first encrypt() call, at which point client A now
                // starts to send message Y - updates pending messages list and
                // prepares hvY to be sent. Assuming message X is going to a
                // superset of message Y's recipients, and asumming each per-
                // recipient message is encrypted in lockstep (i.e. the threads
                // alternate between X and Y), encryption for Y will complete
                // first, be sent to the server first, and probably ordered
                // before X, although its hash vector state comes
                // chronologically after X. A mutual recipient of both messages
                // X and Y (say, client B) will receive Y first, find that hvY
                // does not match up with its current hash_vector state, and
                // conclude that the server has performed some reordering
                // attack. So, releasing the lock on the hash_vectors mutex
                // before the .await point in send_message would be invalid.

                // The other option is to release the lock on the hash_vectors
                // mutex before the .await point on the client callback in this
                // function (server_comm_callback). At this point, messages have
                // already been sent correctly and (lets assume) ordered
                // correctly by the server. If message X begins to be processed
                // by client B, it will be added to the hash_vectors data
                // structure of client B in the right order (no ordering
                // violation will be detected). Then, the mutex is unlocked,
                // and another thread starts processing message Y, which again
                // is added to the hash_vectors data structure correctly, but
                // could be sent to the application before message X. Depending
                // on the conflict resolution schemes, X could overwrite the
                // changes made by Y, which were intended to come after X.

                //println!("...TRYING RECV LOCK...");
                let mut hash_vectors_guard = self.hash_vectors.lock().unwrap();
                //println!("...GOT RECV LOCK...");
                let parsed_res = hash_vectors_guard.parse_message(
                    &msg.sender(),
                    common_payload.clone(),
                    &per_recipient_payload.val_payload,
                );

                // add to incoming_queue before releasing lock
                self.incoming_queue
                    .lock()
                    .unwrap()
                    .push_back(common_payload.clone());
                //println!(
                //    "-----ADDING CP TO IQ: {:?}",
                //    common_payload.clone()
                //);

                core::mem::drop(hash_vectors_guard);
                //println!("...UNLOCKED RECV...");

                // loop until front of queue is ready to forward
                /* TODO: channel?
                loop {
                    let mut iq_guard = self.incoming_queue.lock().unwrap();
                    //println!("-----RECV LOOP");
                    //println!("iq_guard.front(): {:?}", iq_guard.front());
                    if iq_guard.front() != Some(&common_payload) {
                        let _ = self.iq_cv.wait(iq_guard).unwrap();
                    } else {
                        //println!("~~POPPING~~");
                        iq_guard.pop_front();
                        break;
                    }
                }
                */

                match parsed_res {
                    // No message to forward
                    Ok(None) => {
                        //println!("val only");
                    }
                    // Forward message
                    Ok(Some((seq, message))) => {
                        //println!("forwarding");
                        self.client
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .client_callback(msg.sender().clone(), message);

                        // TODO allow client to determine when to send these
                        match self
                            .server_comm
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .delete_messages_from_server(
                                &ToDelete::from_seq_id(seq.try_into().unwrap()),
                            )
                        {
                            Ok(_) => {}
                            Err(err) => panic!(
                                "Error sending delete-message: {:?}",
                                err
                            ),
                        }
                    }
                    Err(err) => {
                        panic!("Validation failed: {:?}", err);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
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

    impl CoreClient for StreamClient {
        fn client_callback(&self, sender: String, message: String) {
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
            Core::new(None, None, false, Some(arc_client));

        let payload = String::from("hello from me");
        let idkey = arc_core.crypto.get_idkey();
        let recipients = vec![idkey.clone()];

        if let Err(err) = arc_core.send_message(recipients, &payload) {
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
            Core::new(None, None, false, Some(arc_client_a));
        let idkey_a = arc_core_a.crypto.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b));
        let idkey_b = arc_core_b.crypto.get_idkey();

        let (client_c, mut receiver_c) = StreamClient::new();
        let arc_client_c = Arc::new(client_c);
        let arc_core_c: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_c));
        let idkey_c = arc_core_c.crypto.get_idkey();

        let payload = String::from("hello from me");
        let recipients =
            vec![idkey_a.clone(), idkey_b.clone(), idkey_c.clone()];

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

        match receiver_c.next().await {
            Some((sender, msg)) => {
                assert_eq!(sender, idkey_a);
                assert_eq!(msg, payload);
            }
            None => panic!("c got NONE from core"),
        }
    }

    #[tokio::test]
    async fn test_send_message_to_others_only() {
        let (client_a, _receiver_a) = StreamClient::new();
        let arc_client_a = Arc::new(client_a);
        let arc_core_a: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_a));
        let idkey_a = arc_core_a.crypto.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b));
        let idkey_b = arc_core_b.crypto.get_idkey();

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
