use async_condvar_fair::Condvar;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::crypto::Crypto;
use crate::hash_vectors::{CommonPayload, HashVectors, ValidationPayload};
use crate::server_comm::{
    EncryptedCommonPayload, EncryptedOutboxMessage,
    EncryptedPerRecipientPayload, Event, ServerComm, ToDelete,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct PerRecipientPayload {
    pub val_payload: ValidationPayload,
    pub key: [u8; 32],
    pub tag: [u8; 16],
    pub nonce: [u8; 12],
}

pub type SequenceNumber = u128;

#[async_trait]
pub trait CoreClient: Sync + Send + 'static {
    async fn client_callback(
        &self,
        seq: SequenceNumber,
        sender: String,
        message: String,
    );
}

pub struct Core<C: CoreClient> {
    crypto: Crypto,
    server_comm: RwLock<Option<ServerComm<C>>>,
    hash_vectors: Mutex<HashVectors>,
    client: RwLock<Option<Arc<C>>>,
    init: parking_lot::Mutex<bool>,
    init_cv: Condvar,
    outgoing_queue: Arc<Mutex<VecDeque<CommonPayload>>>,
    incoming_queue: Arc<Mutex<VecDeque<CommonPayload>>>,
    oq_cv: Condvar,
    iq_cv: Condvar,
    common_ct_size_filename: Option<&'static str>,
}

impl<C: CoreClient> Core<C> {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off: bool,
        common_ct_size_filename: Option<&'static str>,
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
            init: parking_lot::Mutex::new(false),
            init_cv: Condvar::new(),
            outgoing_queue: Arc::new(Mutex::new(
                VecDeque::<CommonPayload>::new(),
            )),
            incoming_queue: Arc::new(Mutex::new(
                VecDeque::<CommonPayload>::new(),
            )),
            oq_cv: Condvar::new(),
            iq_cv: Condvar::new(),
            common_ct_size_filename,
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
        self.crypto.get_idkey()
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
        println!("dst_idkeys: {:?}", &dst_idkeys);

        let mut hash_vectors_guard = self.hash_vectors.lock().await;
        let (common_payload, val_payloads) = hash_vectors_guard
            .prepare_message(
                dst_idkeys.clone(),
                bincode::serialize(payload).unwrap(),
            );

        println!(
            "common_payload.recipients.len(): {:?}",
            &common_payload.recipients.len()
        );

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
            .await
            .push_back(common_payload.clone());

        core::mem::drop(hash_vectors_guard);

        // symmetrically encrypt common_payload once
        let (common_ct, tag, key, nonce) = self
            .crypto
            .symmetric_encrypt(bincode::serialize(&common_payload).unwrap());

        if let Some(filename) = &self.common_ct_size_filename {
            let mut f = File::options()
                .append(true)
                .create(true)
                .open(filename)
                .unwrap();
            write!(f, "{}\n", common_ct.clone().len());
        }

        // Can't use .iter().map().collect() due to async/await
        let mut encrypted_per_recipient_payloads = BTreeMap::new();
        for (idkey, val_payload) in val_payloads {
            println!("idkey: {:?}", &idkey.len());
            //println!("val_payload index: {:?}",
            // &val_payload.validation_seq.map_or(0, |x| x.len()));
            println!(
                "val_payload head: {:?}",
                &val_payload.validation_digest.map_or(0, |x| x.len())
            );
            let (c_type, ciphertext) = self
                .crypto
                .session_encrypt(
                    &self.server_comm.read().await.as_ref().unwrap(),
                    &idkey,
                    bincode::serialize(&PerRecipientPayload {
                        val_payload,
                        key,
                        tag,
			nonce,
                    })
                    .unwrap(),
                )
                .await;

            let sessionlock = self.crypto.sessions.lock();
            if let Some(val) = sessionlock.get(&idkey) {
                let session = &val.1[0];
                let pickled = session.pickle(olm_rs::PicklingMode::Unencrypted);
                println!("pickled session: {:?}", &pickled);
                println!("pickled session len: {:?}", &pickled.len());
            }

            // Ensure we're never encrypting to the same key twice
            assert!(encrypted_per_recipient_payloads
                .insert(
                    idkey,
                    EncryptedPerRecipientPayload { c_type, ciphertext }
                )
                .is_none());
        }

        let encrypted_message = EncryptedOutboxMessage {
            enc_common: EncryptedCommonPayload(common_ct),
            enc_recipients: encrypted_per_recipient_payloads,
        };

        // loop until front of queue is ready to send
        loop {
            let mut oq_guard = self.outgoing_queue.lock().await;
            if oq_guard.front() != Some(&common_payload) {
                let _ = self.oq_cv.wait_no_relock(oq_guard).await;
            } else {
                oq_guard.pop_front();
                break;
            }
        }

        self.server_comm
            .read()
            .await
            .as_ref()
            .unwrap()
            .send_message(encrypted_message)
            .await
    }

    pub async fn server_comm_callback(
        &self,
        event: eventsource_client::Result<Event>,
    ) {
        match event {
            Err(err) => panic!("err: {:?}", err),
            Ok(Event::Otkey) => {
                let otkeys = self.crypto.generate_otkeys(None);
                match self
                    .server_comm
                    .read()
                    .await
                    .as_ref()
                    .unwrap()
                    .add_otkeys_to_server(&otkeys.curve25519())
                    .await
                {
                    Ok(_) => {}
                    Err(err) => panic!("Error sending otkeys: {:?}", err),
                }
                // set init = true and notify init_cv waiters
                let mut init = self.init.lock();
                if !*init {
                    *init = true;
                    self.init_cv.notify_all();
                }
            }
            Ok(Event::Msg(msg)) => {
                let decrypted_per_recipient = self.crypto.session_decrypt(
                    &msg.sender,
                    msg.enc_recipient.c_type,
                    msg.enc_recipient.ciphertext,
                );

                let per_recipient_payload: PerRecipientPayload =
                    bincode::deserialize(&decrypted_per_recipient).unwrap();
                let decrypted_common = self.crypto.symmetric_decrypt(
                    msg.enc_common.0,
                    per_recipient_payload.key,
                    per_recipient_payload.tag,
                    per_recipient_payload.nonce,
                );
                let common_payload: CommonPayload =
                    bincode::deserialize(&decrypted_common).unwrap();

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

                let mut hash_vectors_guard = self.hash_vectors.lock().await;
                let parsed_res = hash_vectors_guard.parse_message(
                    &msg.sender,
                    common_payload.clone(),
                    &per_recipient_payload.val_payload,
                );

                // add to incoming_queue before releasing lock
                self.incoming_queue
                    .lock()
                    .await
                    .push_back(common_payload.clone());

                core::mem::drop(hash_vectors_guard);

                // loop until front of queue is ready to forward
                loop {
                    let mut iq_guard = self.incoming_queue.lock().await;
                    if iq_guard.front() != Some(&common_payload) {
                        let _ = self.iq_cv.wait_no_relock(iq_guard).await;
                    } else {
                        iq_guard.pop_front();
                        break;
                    }
                }

                match parsed_res {
                    // No message to forward
                    Ok(None) => {}
                    // Forward message
                    Ok(Some((seq, message))) => {
                        self.client
                            .read()
                            .await
                            .as_ref()
                            .unwrap()
                            .client_callback(
                                seq as SequenceNumber,
                                msg.sender.clone(),
                                bincode::deserialize::<String>(&message)
                                    .unwrap(),
                            )
                            .await;

                        // TODO allow client to determine when to send these
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
        async fn client_callback(
            &self,
            seq: crate::core::SequenceNumber,
            sender: String,
            message: String,
        ) {
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
    async fn test_storage_overheads() {
        let (client, mut receiver) = StreamClient::new();
        let arc_client = Arc::new(client);
        let arc_core: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some("commons.txt"), Some(arc_client))
                .await;

        let idkeys = arc_core.crypto.account.lock().identity_keys();
        let pickled = arc_core
            .crypto
            .account
            .lock()
            .pickle(olm_rs::PicklingMode::Unencrypted);
        println!("pickled: {:?}", &pickled);
        println!("pickled len: {:?}", pickled.len());
        let parsed = arc_core.crypto.account.lock().parsed_identity_keys();
        let curve = parsed.curve25519();
        let string = curve.to_string();
        println!("parsed: {:?}", &parsed);
        println!("curve: {:?}", &curve);
        println!("string: {:?}", &string);
        println!("string.len: {:?}", &string.len());
    }

    /*
    #[tokio::test]
    async fn test_send_message_to_self_only() {
        let (client, mut receiver) = StreamClient::new();
        let arc_client = Arc::new(client);
        let arc_core: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client)).await;

        let payload = String::from("hello from me");
        let idkey = arc_core.crypto.get_idkey();
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
        let idkey_a = arc_core_a.crypto.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b)).await;
        let idkey_b = arc_core_b.crypto.get_idkey();

        let (client_c, mut receiver_c) = StreamClient::new();
        let arc_client_c = Arc::new(client_c);
        let arc_core_c: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_c)).await;
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
            Core::new(None, None, false, Some(arc_client_a)).await;
        let idkey_a = arc_core_a.crypto.get_idkey();

        let (client_b, mut receiver_b) = StreamClient::new();
        let arc_client_b = Arc::new(client_b);
        let arc_core_b: Arc<Core<StreamClient>> =
            Core::new(None, None, false, Some(arc_client_b)).await;
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
    */
}
