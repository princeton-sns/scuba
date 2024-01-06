use crate::core::{Core, CoreClient};
use async_trait::async_trait;
use eventsource_client::{Client, ClientBuilder, SSE};
use futures::TryStreamExt;
use reqwest::{Response, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, LinkedList};
use std::marker::PhantomData;
use std::sync::Arc;
use url::Url;
use urlencoding::encode;

const BOOTSTRAP_SERVER_URL: &'static str = "http://localhost:8081";
const SERVER_ATTESTATION_PUBKEY: &'static str =
    "l07hNTVLaGBKesJDe1QT1ebxtKgh+nZnrGaeud5E99k";

#[derive(Debug)]
pub enum Event {
    Otkey,
    Msg(EncryptedInboxMessage),
}

pub use noise_server_lib::shard::client_protocol::{
    Attestation, AttestationData, EncryptedCommonPayload,
    EncryptedInboxMessage, EncryptedOutboxMessage,
    EncryptedPerRecipientPayload, MessageBatch,
};

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
    pub otkey: String,
}

impl From<OtkeyResponse> for String {
    fn from(otkey_response: OtkeyResponse) -> String {
        otkey_response.otkey
    }
}

#[async_trait]
pub trait ServerComm {
    async fn send_message(
        &self,
        batch: EncryptedOutboxMessage,
    ) -> Result<Response>;

    async fn get_otkey_from_server(
        &self,
        dst_idkey: &String,
    ) -> Result<OtkeyResponse>;

    async fn delete_messages_from_server(
        &self,
        to_delete: &ToDelete,
    ) -> Result<Response>;

    async fn add_otkeys_to_server<'a>(
        &self,
        to_add: &HashMap<String, String>,
    ) -> Result<Response>;
}

pub struct ServerCommImpl<C: CoreClient> {
    base_url: Url,
    idkey: String,
    client: reqwest::Client,
    _listener_task_handle: tokio::task::JoinHandle<()>,
    _pd: PhantomData<C>,
}
// wasm FIXME s reqwest and SEE
// TODO make (some of) server comm a trait + would help make
// mockable

impl<C: CoreClient> ServerCommImpl<C> {
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
                                        //        println!(
                                        //   "got OTKEY event from server -
                                        // {:?}",
                                        //   task_idkey
                                        //);
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
                                            core.server_comm_callback(Ok(
                                                Event::Msg(
                                                    serde_json::from_str(
                                                        event.data.as_str(),
                                                    )
                                                    .unwrap(),
                                                ),
                                            ))
                                            .await;
                                        }
                                    }
                                    "epoch_message_batch" => {
                                        // println!("got EpochMessageBatch event
                                        // from server");
                                        if let Some(ref core) = core_option {
                                            let emb: MessageBatch =
                                                serde_json::from_str(
                                                    &event.data,
                                                )
                                                .unwrap();
                                            // TODO: handle lost epochs
                                            //println!("MessageBatch for epochs
                                            // {} to {}", emb.start_epoch_id,
                                            // emb.end_epoch_id);

                                            let attestation = Attestation::from_bytes(&emb.attestation).expect("Failed to parse attestation payload");
                                            assert!(attestation.first_epoch() == next_epoch, "Attestation does not cover all epochs");
                                            assert!(attestation.next_epoch() == emb.end_epoch_id, "Attestation claims to cover unreceived epochs");
                                            let attestation_data =
                                                AttestationData::from_inbox_epochs(
                                                    &task_idkey,
                                            attestation.first_epoch(), attestation.next_epoch(),
                                            emb.messages.iter().flat_map(|(_epoch_id, epoch_messages)| epoch_messages.iter()));
                                            assert!(attestation.verify(&attestation_data, &server_attestation_pubkey), "Attestation verification failed");
                                            next_epoch =
                                                attestation.next_epoch();

                                            for msg in emb.messages.into_owned().into_iter().flat_map(|(_epoch_id, epoch_messages)| epoch_messages.into_owned().into_iter()) {
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
}

#[async_trait]
impl<C: CoreClient> ServerComm for ServerCommImpl<C> {
    async fn send_message(
        &self,
        batch: EncryptedOutboxMessage,
    ) -> Result<Response> {
        let mut series = LinkedList::new();
        series.push_back(batch);

        self.client
            .post(self.base_url.join("/message-bin").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .body(bincode::serialize(&series).unwrap())
            .send()
            .await
    }

    async fn get_otkey_from_server(
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
                //println!("Failed to fetch otkey for client_id \"\", retrying
                // in 1 sec...");
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    async fn delete_messages_from_server(
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

    async fn add_otkeys_to_server<'a>(
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

    /*
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
    */

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
