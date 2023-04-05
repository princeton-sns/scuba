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


// Bootstrap server
const BOOTSTRAP_SERVER_URL: &'static str = "http://localhost:8081";

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
	use base64::{Engine as _, engine::general_purpose};
	EncryptedCommonPayload(general_purpose::STANDARD_NO_PAD.encode(bytes))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
	use base64::{Engine as _, engine::general_purpose};
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptedInboxMessage {
    pub sender: String,
    // TODO: currently both the server and the common payload contain the
    // full list of recipients. We should optimize this to use a hash
    // of the list of recipients in the common payload.
    //
    // We need to make sure of the following:
    // - the sending client can't lie about the list of recipients of a message.
    //   We achieve that by having the server provide us the list of recipients.
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
                    Ok(Some(event)) => match event {
                        SSE::Comment(_) => {}
                        SSE::Event(event) => match event.event_type.as_str() {
                            "otkey" => {
                                println!(
                                   "got OTKEY event from server - {:?}",
                                   task_idkey
                                );
                                if let Some(ref core) = core_option {
                                    core.server_comm_callback(Ok(Event::Otkey))
                                        .await;
                                }
                            }
                            "msg" => {
                                //println!("got MSG event from server");
                                if let Some(ref core) = core_option {
                                    core.server_comm_callback(Ok(Event::Msg(
                                        EncryptedInboxMessage::from_string(event.data),
                                    )))
                                    .await;
                                }
                            }
			    "epoch_message_batch" => {
                                // println!("got EpochMessageBatch event from server");
                                if let Some(ref core) = core_option {
				    let emb: EpochMessageBatch = serde_json::from_str(&event.data).unwrap();
				    // TODO: handle lost epochs
				    println!("EpochMessageBatch for epoch {}, attestation: {}", emb.epoch_id, emb.attestation);
				    for msg in emb.messages {
					core.server_comm_callback(Ok(Event::Msg(
                                            msg,
					)))
					    .await;
				    }
                                }
                            }
                            _ => panic!("Got unexpected SSE event: {:?}", event),
                        },
                    },
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

    pub async fn send_message(&self, batch: &EncryptedOutboxMessage) -> Result<Response> {
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
