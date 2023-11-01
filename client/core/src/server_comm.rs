use crate::core::{Core, CoreClient};
use eventsource_client::{Client, ClientBuilder, SSE};
use futures::TryStreamExt;
use reqwest::{Response, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use url::Url;
use urlencoding::encode;

pub mod attestation {
    use super::EncryptedInboxMessage;
    use std::borrow::Borrow;

    // -------------------------------------------------------------------------
    // Attestation payload layout

    #[derive(Clone, Debug)]
    // Format:
    // - u64: first_epoch
    // - u64: next_epoch
    // - [u8; 32]: messages_digest
    pub struct AttestationData([u8; 48]);

    impl AttestationData {
        fn message_hash_table<T: Borrow<EncryptedInboxMessage>>(
            messages: impl Iterator<Item = T>,
        ) -> impl Iterator<Item = (u128, [u8; 32], [u8; 32])> {
            use sha2::{Digest, Sha256};

            let mut hasher = Sha256::new();

            messages.map(move |message| {
                for r in message.borrow().recipients.iter() {
                    hasher.update(r.as_bytes());
                    hasher.update(b";");
                }
                let mut recipients_digest = [0; 32];
                hasher.finalize_into_reset((&mut recipients_digest).into());

                let mut payload_digest = [0; 32];
                hasher.update(message.borrow().enc_common.0.as_bytes());
                hasher.finalize_into_reset((&mut payload_digest).into());

                (message.borrow().seq_id, recipients_digest, payload_digest)
            })
        }

        pub fn from_inbox_epochs<T: Borrow<EncryptedInboxMessage>>(
            client_id: &str,
            first_epoch: u64,
            next_epoch: u64,
            messages: impl Iterator<Item = T>,
        ) -> AttestationData {
            Self::from_inbox_epochs_int(
                client_id,
                first_epoch,
                next_epoch,
                Self::message_hash_table(messages),
            )
        }

        fn from_inbox_epochs_int<T: Borrow<(u128, [u8; 32], [u8; 32])>>(
            client_id: &str,
            first_epoch: u64,
            next_epoch: u64,
            message_hash_table: impl Iterator<Item = T>,
        ) -> AttestationData {
            use sha2::{Digest, Sha256};

            let mut hasher = Sha256::new();

            hasher.update(client_id.as_bytes());

            message_hash_table.for_each(|b| {
                let (epoch_id, recipients_digest, payload_digest) = b.borrow();
                hasher.update(&u128::to_le_bytes(*epoch_id));
                hasher.update(&recipients_digest);
                hasher.update(&payload_digest);
            });

            let mut attestation_messages_hash = [0; 32];
            hasher.finalize_into((&mut attestation_messages_hash).into());

            let mut attestation_data = [0; 48];
            attestation_data[0..8]
                .copy_from_slice(&u64::to_le_bytes(first_epoch));
            attestation_data[8..16]
                .copy_from_slice(&u64::to_le_bytes(next_epoch));
            attestation_data[16..].copy_from_slice(&attestation_messages_hash);
            AttestationData(attestation_data)
        }

        pub fn attest(
            &self,
            attestation_key: &ed25519_dalek::Keypair,
        ) -> Attestation {
            use ed25519_dalek::Signer;
            let signature = attestation_key.sign(&self.0);

            let mut attestation = [0; 48 + 64];
            attestation[0..48].copy_from_slice(&self.0);
            attestation[48..].copy_from_slice(&signature.to_bytes());
            Attestation(attestation)
        }

        pub fn produce_claim(
            &self,
            client_id: String,
            first_epoch: u64,
            next_epoch: u64,
            messages: Vec<EncryptedInboxMessage>,
            claim_message_idx: Option<usize>,
            attestation: Attestation,
        ) -> AttestationClaim {
            if let Some(idx) = claim_message_idx {
                assert!(idx < messages.len());
            }

            // Generate the hash-table to be passed alongside the attestation:
            let message_hash_table: Vec<_> =
                Self::message_hash_table(messages.iter()).collect();

            // Sanity check that the claim we're producing is supported by the
            // passed attestation:
            let attestation_data = Self::from_inbox_epochs_int(
                &client_id,
                first_epoch,
                next_epoch,
                message_hash_table.iter(),
            );
            assert!(attestation_data.0[..] == attestation.0[..48]);

            // Extract the server signature from the attestation:
            let mut signature = [0; 64];
            signature.copy_from_slice(&attestation.0[48..]);

            AttestationClaim {
                client_id,
                first_epoch,
                next_epoch,
                message_hash_table,
                message_recipients: claim_message_idx.map(|idx| {
                    // Need to clone, can't move out of Vec
                    (idx, messages[idx].recipients.clone())
                }),
                attestation: signature,
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct AttestationClaim {
        pub client_id: String,
        pub first_epoch: u64,
        pub next_epoch: u64,
        pub message_hash_table: Vec<(u128, [u8; 32], [u8; 32])>,
        pub message_recipients: Option<(usize, Vec<String>)>,
        pub attestation: [u8; 64],
    }

    impl AttestationClaim {
        pub fn attestation(&self) -> Attestation {
            let attestation_data = AttestationData::from_inbox_epochs_int(
                &self.client_id,
                self.first_epoch,
                self.next_epoch,
                self.message_hash_table.iter(),
            );
            let mut attestation_buf = [0; 48 + 64];
            attestation_buf[..48].copy_from_slice(&attestation_data.0);
            attestation_buf[48..].copy_from_slice(&self.attestation);
            Attestation(attestation_buf)
        }

        pub fn supports(
            &self,
            other: &AttestationClaim,
            public_key: &ed25519_dalek::PublicKey,
        ) -> bool {
            // Two claims support each other when
            // - one contains a message at a sequence number that the other does
            //   not (positive + negative claim), or
            // - both contain a message with an identical sequence number but
            //   differing receipients or common payload (positive + positive).
            //
            // For simpler handling, "sort" the two claims by whichever contains
            // a potentially conflicting message reference. It's invalid for
            // neither of them to point to a message:
            let (claim_p, claim_pn) = if self.message_recipients.is_some() {
                (self, other)
            } else {
                (other, self)
            };

            let (p_idx, claim_p_recipients) =
                if let Some(p) = &claim_p.message_recipients {
                    p
                } else {
                    // At least one positive claim required!
                    return false;
                };

            // Need to handle positive + positive & positive + negative
            // differently:
            if let Some((pn_idx, _recipients)) = &claim_pn.message_recipients {
                // Positive + positive claim! Compare sequence numbers. If they
                // match, recipients and payload must be identical.
                let (p_seqid, p_recipients, p_payload) =
                    claim_p.message_hash_table[*p_idx];
                let (pn_seqid, pn_recipients, pn_payload) =
                    claim_pn.message_hash_table[*pn_idx];

                if p_seqid != pn_seqid
                    || (p_recipients == pn_recipients
                        && p_payload == pn_payload)
                {
                    return false;
                }

            // Claims support each other, but individual claims not
            // verified yet!
            } else {
                // Positive + negative claim! The positive claim can only be
                // supported by the negative claim if the offending message's
                // sequence number is contained within the negative claim's
                // sequence space, so verify that.
                let (p_seqid, p_recipients, _p_payload) =
                    claim_p.message_hash_table[*p_idx];

                // Is there a better way to do this which doesn't involve
                // bitshifting & casting?
                let [e0, e1, e2, e3, e4, e5, e6, e7, _, _, _, _, _, _, _, _] =
                    u128::to_be_bytes(p_seqid);
                let p_epoch =
                    u64::from_be_bytes([e0, e1, e2, e3, e4, e5, e6, e7]);
                if p_epoch < claim_pn.first_epoch
                    || p_epoch >= claim_pn.next_epoch
                {
                    return false;
                }

                // claim_p's message lies in claim_pn's sequence space, now we
                // need to verify that claim_p should indeed have been received
                // by claim_pn. We can do this by ensuring that claim_pn's
                // client_id is in the recipients of claim_p's message.
                //
                // For this, we first have to validate that the
                // message_hash_table entry's recipients hash corresponds to the
                // recipients vec part of the claim, and then check that
                // claim_pn's client id is an element in that vec:
                let claim_p_recipients_digest = {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    for r in claim_p_recipients {
                        hasher.update(r.as_bytes());
                        hasher.update(b";");
                    }
                    let mut recipients_digest = [0; 32];
                    hasher.finalize_into_reset((&mut recipients_digest).into());
                    recipients_digest
                };

                if p_recipients != claim_p_recipients_digest {
                    // Can't reproduce the hash-table entry of the recipients of
                    // claim_p's message.
                    return false;
                }

                // p_recipients indeed corresponds to claim_p's
                // claim_p_recipients entry:
                if claim_p_recipients
                    .iter()
                    .find(|r| **r == claim_pn.client_id)
                    .is_none()
                {
                    // claim_pn's client_id isn't in the recipients list, so the
                    // negative claim can't be used here.
                    return false;
                }

                // Now, loop over claim_pn's message to ensure that it indeed
                // does not contain a message with the offending sequence
                // number:
                if claim_pn
                    .message_hash_table
                    .iter()
                    .find(|(seqid, _, _)| *seqid == p_seqid)
                    .is_some()
                {
                    return false;
                }

                // Claims support each other, but individual claims not
                // verified yet!
            }

            // Verify each of the invidual claim's cryptographic validity. If
            // they are both valid, we hold evidence that the server has behaved
            // incorrectly.
            //
            // It is fine to use verify_trusted here, as we're trusting the
            // attestation payload hash which is constructed based on the claim
            // data by `attestation()`.
            claim_p.attestation().verify_trusted(public_key)
                && claim_pn.attestation().verify_trusted(public_key)
        }
    }

    #[derive(Clone, Debug)]
    pub struct Attestation([u8; 48 + 64]);

    impl Attestation {
        pub fn into_arr(self) -> [u8; 48 + 64] {
            self.0
        }

        pub fn from_arr(arr: [u8; 48 + 64]) -> Self {
            Attestation(arr)
        }

        pub fn from_bytes(bytes: &[u8]) -> Result<Self, ()> {
            if bytes.len() != 48 + 64 {
                Err(())
            } else {
                let mut buf = [0; 48 + 64];
                buf.copy_from_slice(bytes);
                Ok(Attestation(buf))
            }
        }

        pub fn decode_base64(s: &str) -> Result<Self, ()> {
            use base64::{engine::general_purpose, Engine as _};

            if let Ok(vec) = general_purpose::STANDARD_NO_PAD.decode(s) {
                if vec.len() != 48 + 64 {
                    Err(())
                } else {
                    let mut buf = [0; 48 + 64];
                    buf.copy_from_slice(&vec);
                    Ok(Attestation(buf))
                }
            } else {
                Err(())
            }
        }

        pub fn encode_base64(&self) -> String {
            use base64::{engine::general_purpose, Engine as _};
            general_purpose::STANDARD_NO_PAD.encode(&self.0)
        }

        pub fn first_epoch(&self) -> u64 {
            u64::from_le_bytes([
                self.0[0], self.0[1], self.0[2], self.0[3], self.0[4],
                self.0[5], self.0[6], self.0[7],
            ])
        }

        pub fn next_epoch(&self) -> u64 {
            u64::from_le_bytes([
                self.0[8], self.0[9], self.0[10], self.0[11], self.0[12],
                self.0[13], self.0[14], self.0[15],
            ])
        }

        pub fn verify(
            &self,
            data: &AttestationData,
            public_key: &ed25519_dalek::PublicKey,
        ) -> bool {
            data.0 == self.0[0..48] && self.verify_trusted(public_key)
        }

        pub fn verify_trusted(
            &self,
            public_key: &ed25519_dalek::PublicKey,
        ) -> bool {
            use ed25519_dalek::Verifier;
            let signature =
                ed25519_dalek::Signature::from_bytes(&self.0[48..]).unwrap();
            public_key.verify(&self.0[..48], &signature).is_ok()
        }
    }
}

const BOOTSTRAP_SERVER_URL: &'static str = "http://localhost:8081";
const SERVER_ATTESTATION_PUBKEY: &'static str =
    "l07hNTVLaGBKesJDe1QT1ebxtKgh+nZnrGaeud5E99k";

#[derive(Debug)]
pub enum Event {
    Otkey,
    Msg(EncryptedInboxMessage),
}

// Transparent wrapper around a byte array, as a marker that this is
// the encrypted shared/common payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(transparent)]
pub struct EncryptedCommonPayload(pub String);

impl EncryptedCommonPayload {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        use base64::{engine::general_purpose, Engine as _};
        EncryptedCommonPayload(general_purpose::STANDARD_NO_PAD.encode(bytes))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::STANDARD_NO_PAD.decode(&self.0).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptedPerRecipientPayload {
    pub c_type: usize,
    pub ciphertext: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EncryptedOutboxMessage {
    pub enc_common: EncryptedCommonPayload,
    // map from recipient id to payload
    pub enc_recipients: HashMap<String, EncryptedPerRecipientPayload>,
}

#[derive(Debug, Serialize, Clone)]
pub struct OutboxMessages {
    pub messages: Vec<EncryptedOutboxMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptedInboxMessage {
    pub sender: String,
    // TODO: currently both the server and the common payload contain the
    // full list of recipients. We should optimize this to use a hash
    // of the list of recipients in the common payload.
    //
    // We need to make sure of the following:
    // - the sending client can't lie about the list of recipients of a
    //   message. We achieve that by having the server provide us the list of
    //   recipients.
    // - the server can't lie about the list of recipients to a subset of
    //   clients. We achieve that by including a hash of the list of recipients
    //   in the common payload.
    // If these two do not match, we MUST refuse (drop) the message. If the
    // sending client lied, this will just cause the message to be dropped
    // everywhere. If the server lied, this will then be detected through
    // LVS, because a subset of clients will see the message as dropped.
    pub recipients: Vec<String>,
    pub enc_common: EncryptedCommonPayload,
    pub enc_recipient: EncryptedPerRecipientPayload,
    pub seq_id: u128,
}

#[derive(Deserialize, Clone, Debug)]
pub struct EpochMessageBatch {
    pub epoch_id: u64,
    pub messages: Vec<EncryptedInboxMessage>,
    pub attestation: String,
}

impl EncryptedInboxMessage {
    pub fn from_string(msg: String) -> Self {
        serde_json::from_str(msg.as_str()).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToDelete {
    seq_id: u64,
}

impl ToDelete {
    pub fn from_seq_id(seq_id: u64) -> Self {
        Self { seq_id }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OtkeyResponse {
    otkey: String,
}

impl From<OtkeyResponse> for String {
    fn from(otkey_response: OtkeyResponse) -> String {
        otkey_response.otkey
    }
}

pub struct ServerComm<C: CoreClient> {
    base_url: Url,
    idkey: String,
    client: reqwest::Client,
    _listener_task_handle: tokio::task::JoinHandle<()>,
    _pd: PhantomData<C>,
}
// wasm FIXME s reqwest and SEE
// TODO make (some of) server comm a trait + would help make
// mockable

impl<C: CoreClient> ServerComm<C> {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        idkey: String,
        core_option: Option<Arc<Core<C>>>,
    ) -> Self {
        // Resolve our home-shard base-url by contacting the bootstrap shard:
        let client = reqwest::Client::new();
        let base_url = Url::parse(
	    &client
                .get(format!("{}/shard", BOOTSTRAP_SERVER_URL))
                .header(
                    "Authorization",
                    &format!("Bearer {}", &idkey),
                )
                .send()
                .await
                .expect("Failed to contact the bootstrap server shard")
                .text()
                .await
                .expect("Failed to retrieve response from the bootstrap server shard")
        ).expect("Failed to construct home-shard base url from response");

        let server_attestation_pubkey = {
            use base64::{engine::general_purpose, Engine as _};

            ed25519_dalek::PublicKey::from_bytes(
                &general_purpose::STANDARD_NO_PAD
                    .decode(SERVER_ATTESTATION_PUBKEY)
                    .unwrap(),
            )
            .unwrap()
        };

        let task_base_url = base_url.clone();
        let task_idkey = idkey.clone();
        let _listener_task_handle = tokio::spawn(async move {
            let mut listener = Box::new(
                ClientBuilder::for_url(
                    task_base_url
                        .join("/events")
                        .expect("Failed join of /events")
                        .as_str(),
                )
                .expect("Failed in ClientBuilder::for_url")
                .header(
                    "Authorization",
                    &vec!["Bearer", &task_idkey.to_string()].join(" "),
                )
                .expect("Failed header construction")
                .build(),
            )
            .stream();

            let mut next_epoch = 0;
            loop {
                //println!("IN LOOP");
                match listener.as_mut().try_next().await {
                    Err(err) => {
                        //println!("got ERR from server: {:?}", err);
                        if let Some(ref core) = core_option {
                            core.server_comm_callback(Err(err)).await;
                        }
                    }
                    Ok(None) => {
                        //println!("got NONE from server")
                    }
                    Ok(Some(event)) => {
                        match event {
                            SSE::Comment(_) => {}
                            SSE::Event(event) => {
                                match event.event_type.as_str() {
                                    "otkey" => {
                                        println!(
                                   "got OTKEY event from server - {:?}",
                                   task_idkey
                                );
                                        if let Some(ref core) = core_option {
                                            core.server_comm_callback(Ok(
                                                Event::Otkey,
                                            ))
                                            .await;
                                        }
                                    }
                                    "msg" => {
                                        //println!("got MSG event from
                                        // server");
                                        if let Some(ref core) = core_option {
                                            core.server_comm_callback(Ok(Event::Msg(
                                        EncryptedInboxMessage::from_string(event.data),
                                    )))
                                    .await;
                                        }
                                    }
                                    "epoch_message_batch" => {
                                        // println!("got EpochMessageBatch event
                                        // from server");
                                        if let Some(ref core) = core_option {
                                            let emb: EpochMessageBatch =
                                                serde_json::from_str(
                                                    &event.data,
                                                )
                                                .unwrap();
                                            // TODO: handle lost epochs
                                            println!("EpochMessageBatch for epoch {}", emb.epoch_id);

                                            let attestation = attestation::Attestation::decode_base64(&emb.attestation).expect("Failed to parse attestation payload");
                                            assert!(attestation.first_epoch() == next_epoch, "Attestation does not cover all epochs");
                                            assert!(attestation.next_epoch() == emb.epoch_id + 1, "Attestation claims to cover unreceived epochs");
                                            let attestation_data =
                                                attestation::AttestationData::from_inbox_epochs(
                                                    &task_idkey,
                                            attestation.first_epoch(), attestation.next_epoch(),
                                            emb.messages.iter());
                                            assert!(attestation.verify(&attestation_data, &server_attestation_pubkey), "Attestation verification failed");
                                            next_epoch =
                                                attestation.next_epoch();

                                            for msg in emb.messages {
                                                core.server_comm_callback(Ok(
                                                    Event::Msg(msg),
                                                ))
                                                .await;
                                            }
                                        }
                                    }
                                    _ => panic!(
                                        "Got unexpected SSE event: {:?}",
                                        event
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        });

        Self {
            base_url,
            idkey,
            client,
            _listener_task_handle,
            _pd: PhantomData,
        }
    }

    pub async fn send_message(
        &self,
        batch: &OutboxMessages,
    ) -> Result<Response> {
        self.client
            .post(self.base_url.join("/message").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&batch)
            .send()
            .await
    }

    pub async fn get_otkey_from_server(
        &self,
        dst_idkey: &String,
    ) -> Result<OtkeyResponse> {
        use tokio::time::{sleep, Duration};

        let mut retry_count = 0;
        sleep(Duration::from_millis(10)).await;

        loop {
            let mut url = self.base_url.join("/devices/otkey").expect("");
            url.set_query(Some(
                &vec!["device_id", &encode(dst_idkey).into_owned()].join("="),
            ));
            let res = self.client.get(url).send().await?;
            if res.status().is_success() || retry_count >= 3 {
                return res.json().await;
            } else {
                retry_count += 1;
                println!("Failed to fetch otkey for client_id \"\", retrying in 1 sec...");
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    pub async fn delete_messages_from_server(
        &self,
        to_delete: &ToDelete,
    ) -> Result<Response> {
        self.client
            .delete(self.base_url.join("/self/messages").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&to_delete)
            .send()
            .await
    }

    pub async fn add_otkeys_to_server<'a>(
        &self,
        to_add: &HashMap<String, String>,
    ) -> Result<Response> {
        self.client
            .post(self.base_url.join("/self/otkeys").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&to_add)
            .send()
            .await
    }
}

#[cfg(test)]
mod tests {
    //use super::{
    //    Batch, Event, IncomingMessage, OutgoingMessage,
    // EncryptedPerRecipientPayload, ServerComm,    ToDelete,
    //};
    use crate::core::stream_client::StreamClient;
    use crate::core::Core;
    //use crate::crypto::Crypto;
    use std::sync::Arc;
    //use tokio::sync::RwLock;

    //struct TestCore {
    //    server_comm: RwLock<Option<ServerComm<StreamClient>>>,
    //}

    //impl TestCore {
    //    pub async fn new(idkey: String) -> Arc<TestCore> {
    //        let arc_core = Arc::new(TestCore {
    //            server_comm: RwLock::new(None),
    //        });

    //        {
    //            let mut server_comm_guard =
    // arc_core.server_comm.write().await;            let
    // server_comm =
    // ServerComm::<StreamClient>::new(None, None, idkey,
    // Some(arc_core.clone())).await;        }

    //        arc_core
    //    }
    //}

    #[tokio::test]
    async fn test_new() {
        let arc_core: Arc<Core<StreamClient>> =
            Core::new(None, None, false, None).await;

        //let crypto = Crypto::new(false);
        //let idkey = crypto.get_idkey();
        //let arc_core: Arc<Core<StreamClient>> =
        // Arc::new(Core {    crypto,
        //    server_comm: RwLock::new(None),
        //    hash_vectors:
        // Mutex::new(HashVectors::new(idkey.clone())),
        //    client: RwLock::new(None),
        //});
        //ServerComm::<StreamClient>::new(None, None,
        // "abcd".to_string(), None).await;
    }

    /*
    #[tokio::test]
    async fn test_new() {
        assert_eq!(
            ServerComm::new(None, None, "abcd".to_string())
                .try_next()
                .await,
            Ok(Some(Event::Otkey))
        );
    }

    #[tokio::test]
    async fn test_send_message() {
        let idkey = String::from("efgh");
        let enc_per_recipient = String::from("hello");
        let batch = Batch::from_vec(vec![OutgoingMessage::new(
            idkey.clone(),
            EncryptedPerRecipientPayload::new(0, enc_per_recipient.clone()),
        )]);

        let mut server_comm = ServerComm::new(None, None, idkey.clone());
        assert_eq!(server_comm.try_next().await, Ok(Some(Event::Otkey)));

        match server_comm.send_message(&batch).await {
            Ok(_) => match server_comm.try_next().await {
                Ok(Some(Event::Msg(msg_string))) => {
                    let msg: IncomingMessage = serde_json::from_str(msg_string.as_str()).unwrap();
                    assert_eq!(msg.sender, idkey);
                    assert_eq!(msg.enc_per_recipient.ciphertext, enc_per_recipient);
                }
                Ok(Some(Event::Otkey)) => panic!("Got otkey event"),
                Ok(None) => panic!("Got none"),
                Err(err) => panic!("Got error: {:?}", err),
            },
            Err(err) => panic!("Error sending message: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_delete_messages() {
        let idkey = String::from("ijkl");
        let enc_per_recipient = String::from("hello");
        let batch = Batch::from_vec(vec![OutgoingMessage::new(
            idkey.clone(),
            EncryptedPerRecipientPayload::new(0, enc_per_recipient.clone()),
        )]);

        let mut server_comm = ServerComm::new(None, None, idkey.clone());
        assert_eq!(server_comm.try_next().await, Ok(Some(Event::Otkey)));

        match server_comm.send_message(&batch).await {
            Ok(_) => match server_comm.try_next().await {
                Ok(Some(Event::Msg(msg_string))) => {
                    let msg: IncomingMessage = serde_json::from_str(msg_string.as_str()).unwrap();
                    assert_eq!(msg.sender, idkey);
                    assert_eq!(msg.enc_per_recipient.ciphertext, enc_per_recipient);
                    assert!(msg.seq_id > 0);
                    println!("msg.seq_id: {:?}", msg.seq_id);

                    match server_comm
                        .delete_messages_from_server(&ToDelete::from_seq_id(msg.seq_id))
                        .await
                    {
                        Ok(_) => println!("Sent delete-message successfully"),
                        Err(err) => panic!("Error sending delete-message: {:?}", err),
                    }
                }
                Ok(Some(Event::Otkey)) => panic!("Got otkey event"),
                Ok(None) => panic!("Got none"),
                Err(err) => panic!("Got error: {:?}", err),
            },
            Err(err) => panic!("Error sending message: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_add_otkeys_to_server() {
        let crypto = Crypto::new(false);
        let idkey = crypto.get_idkey();
        let mut server_comm = ServerComm::new(None, None, idkey);
        match server_comm.try_next().await {
            Ok(Some(Event::Otkey)) => {
                let otkeys = crypto.generate_otkeys(None);
                println!("otkeys: {:?}", otkeys);
                match server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
                    Ok(_) => println!("Sent otkeys successfully"),
                    Err(err) => panic!("Error sending otkeys: {:?}", err),
                }
            }
            _ => panic!("Unexpected result"),
        }
    }

    #[tokio::test]
    async fn test_get_otkey_from_server() {
        let crypto = Crypto::new(false);
        let idkey = crypto.get_idkey();
        let otkeys = crypto.generate_otkeys(None);
        let mut values = otkeys.curve25519().values().cloned();
        let mut server_comm = ServerComm::new(None, None, idkey.clone());
        match server_comm.try_next().await {
            Ok(Some(Event::Otkey)) => {
                match server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
                    Ok(_) => println!("Sent otkeys successfully"),
                    Err(err) => panic!("Error sending otkeys: {:?}", err),
                }
            }
            _ => panic!("Unexpected result"),
        }
        match server_comm.get_otkey_from_server(&idkey).await {
            Ok(res) => {
                println!("otkey: {:?}", res);
                assert!(values.any(|x| x.eq(&res.otkey)));
            }
            Err(err) => panic!("Error getting otkey: {:?}", err),
        }
    }
    */
}
