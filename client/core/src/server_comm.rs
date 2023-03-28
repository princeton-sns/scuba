use eventsource::reqwest::Client;
use reqwest::{blocking::Response, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;
use urlencoding::encode;

const IP_ADDR: &str = "localhost";
const PORT_NUM: &str = "8080";
const HTTP_PREFIX: &str = "http://";
const COLON: &str = ":";

#[derive(PartialEq, Debug)]
pub enum Event {
    Otkey,
    Msg(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedCommonPayload {
    byte_vec: Vec<u8>,
}

impl EncryptedCommonPayload {
    pub fn new(byte_vec: Vec<u8>) -> EncryptedCommonPayload {
        Self { byte_vec }
    }

    pub fn byte_vec(&self) -> &Vec<u8> {
        &self.byte_vec
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedPerRecipientPayload {
    c_type: usize,
    ciphertext: String,
}

impl EncryptedPerRecipientPayload {
    pub fn new(
        c_type: usize,
        ciphertext: String,
    ) -> EncryptedPerRecipientPayload {
        Self { c_type, ciphertext }
    }

    pub fn c_type(&self) -> usize {
        self.c_type
    }

    pub fn ciphertext(&self) -> &String {
        &self.ciphertext
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMessage {
    device_id: String,
    enc_common: EncryptedCommonPayload,
    enc_per_recipient: EncryptedPerRecipientPayload,
}

impl OutgoingMessage {
    pub fn new(
        device_id: String,
        enc_common: EncryptedCommonPayload,
        enc_per_recipient: EncryptedPerRecipientPayload,
    ) -> OutgoingMessage {
        Self {
            device_id,
            enc_common,
            enc_per_recipient,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Batch {
    batch: Vec<OutgoingMessage>,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            batch: Vec::<OutgoingMessage>::new(),
        }
    }

    pub fn from_vec(batch: Vec<OutgoingMessage>) -> Self {
        Self { batch }
    }

    pub fn push(&mut self, message: OutgoingMessage) {
        self.batch.push(message);
    }

    //pub fn pop(&mut self) -> Option<OutgoingMessage> {
    //  self.batch.pop()
    //}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingMessage {
    sender: String,
    enc_common: EncryptedCommonPayload,
    enc_per_recipient: EncryptedPerRecipientPayload,
    seq_id: u64,
}

impl IncomingMessage {
    pub fn from_string(msg: String) -> Self {
        serde_json::from_str(msg.as_str()).unwrap()
    }

    pub fn sender(&self) -> &String {
        &self.sender
    }

    pub fn enc_common(&self) -> &EncryptedCommonPayload {
        &self.enc_common
    }

    pub fn enc_per_recipient(&self) -> &EncryptedPerRecipientPayload {
        &self.enc_per_recipient
    }

    pub fn seq_id(&self) -> u64 {
        self.seq_id
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

pub struct ServerComm {
    base_url: Url,
    idkey: String,
    client: reqwest::blocking::Client,
    //_listener_task_handle: tokio::task::JoinHandle<()>,
}
// wasm FIXME s reqwest and SEE
// TODO make (some of) server comm a trait + would help make
// mockable

impl ServerComm {
    pub fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        idkey: String,
        sse_callback: Box<dyn Fn(Event) + Send>,
    ) -> Self {
        let ip_addr = ip_arg.unwrap_or(IP_ADDR);
        let port_num = port_arg.unwrap_or(PORT_NUM);
        let base_url =
            Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num].join(""))
                .expect("Failed base_url construction");

        let task_base_url = base_url.clone();
        let task_idkey = idkey.clone();

        std::thread::spawn(move || {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(format!("Bearer {}", task_idkey).as_str()).unwrap()
            );
            let rc = reqwest::blocking::Client::builder()
                .default_headers(headers).build().expect("client");
            let client = Client::new_with_client(task_base_url.join("/events").expect("Failed join of /events"), rc);
            for event in client.filter_map(|e| e.ok()) {
                match event.event_type.as_ref().map(String::as_str) {
                    Some("otkey") => {
                        //println!(
                        //    "got OTKEY event from server - {:?}",
                        //    task_idkey
                        //);
                        sse_callback(Event::Otkey);
                    }
                    Some("msg") => {
                        //println!("got MSG event from server");
                        let msg = String::from(event.data);
                        //println!("msg: {:?}", msg);
                        sse_callback(Event::Msg(msg));
                    },
                    _ => {}
                }
            }
        });

        Self {
            base_url,
            idkey,
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn send_message(&self, batch: &Batch) -> Result<Response> {
        self.client
            .post(self.base_url.join("/message").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&batch)
            .send()
    }

    pub fn get_otkey_from_server(
        &self,
        dst_idkey: &String,
    ) -> Result<OtkeyResponse> {
        let mut url = self.base_url.join("/devices/otkey").expect("");
        url.set_query(Some(
            &vec!["device_id", &encode(dst_idkey).into_owned()].join("="),
        ));
        self.client.get(url).send()?.json()
    }

    pub fn delete_messages_from_server(
        &self,
        to_delete: &ToDelete,
    ) -> Result<Response> {
        self.client
            .delete(self.base_url.join("/self/messages").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&to_delete)
            .send()
    }

    pub fn add_otkeys_to_server<'a>(
        &self,
        to_add: &HashMap<String, String>,
    ) -> Result<Response> {
        self.client
            .post(self.base_url.join("/self/otkeys").expect("").as_str())
            .header("Content-Type", "application/json")
            .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
            .json(&to_add)
            .send()
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
            Core::new(None, None, false, None);

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
