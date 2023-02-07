use std::pin::Pin;
use url::Url;
use eventsource_client::{Client, ClientBuilder, SSE};
use urlencoding::encode;
use futures::{Stream, task::{Context, Poll}};
use futures::TryStreamExt;
use reqwest::{Result, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::olm_wrapper::OlmWrapper;

const IP_ADDR    : &str = "localhost";
const PORT_NUM   : &str = "8080";
const HTTP_PREFIX: &str = "http://";
const COLON      : &str = ":";

#[derive(PartialEq, Debug)]
pub enum Event {
  Otkey,
  Msg(String),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
  c_type: usize,
  ciphertext: String,
}

impl Payload {
  pub fn new(c_type: usize, ciphertext: String) -> Payload {
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
  payload: Payload,
}

impl OutgoingMessage {
  pub fn new(device_id: String, payload: Payload) -> OutgoingMessage {
    Self { device_id, payload }
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
  payload: Payload,
  seq_id: u64,
}

impl IncomingMessage {
  pub fn from_string(msg: String) -> Self {
    serde_json::from_str(msg.as_str()).unwrap()
  }

  pub fn sender(&self) -> &String {
    &self.sender
  }

  pub fn payload(&self) -> &Payload {
    &self.payload
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
  base_url   : Url,
  idkey      : String,
  client     : reqwest::Client,
  listener   : Pin<Box<dyn Stream<Item = eventsource_client::Result<SSE>>>>,
}
// wasm FIXME s reqwest and SEE
// TODO make (some of) server comm a trait + would help make mockable

impl ServerComm {
  pub fn new<'a>(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    idkey: String,
  ) -> Self {
    let ip_addr = ip_arg.unwrap_or(IP_ADDR);
    let port_num = port_arg.unwrap_or(PORT_NUM);
    let base_url = Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num]
            .join("")
        ).expect("Failed base_url construction");
    let listener = Box::new(
        ClientBuilder::for_url(
                base_url
                .join("/events")
                .expect("Failed join of /events")
                .as_str()
            ).expect("Failed in ClientBuilder::for_url")
            .header(
                "Authorization",
                &vec!["Bearer", &idkey].join(" ")
            ).expect("Failed header construction")
            .build()
        )
        .stream();
    Self {
      base_url,
      idkey,
      client  : reqwest::Client::new(),
      listener,
    }
  }

  pub async fn init<'a>(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    olm_wrapper: &OlmWrapper,
  ) -> Self {
    let mut server_comm = ServerComm::new(ip_arg, port_arg, olm_wrapper.get_idkey());
    match server_comm.try_next().await {
      Ok(Some(Event::Otkey)) => {
        let otkeys = olm_wrapper.generate_otkeys(None);
        match server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
          Ok(_) => println!("Sent otkeys successfully"),
          Err(err) => panic!("Error sending otkeys: {:?}", err),
        }
      },
      _ => panic!("Unexpected event from server"),
    }
    server_comm
  }

  pub async fn send_message(&self, batch: &Batch) -> Result<Response> {
    self.client.post(self.base_url.join("/message").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
        .json(&batch)
        .send()
        .await
  }

  pub async fn get_otkey_from_server(
      &self,
      dst_idkey: &String
  ) -> Result<OtkeyResponse> {
    let mut url = self.base_url.join("/devices/otkey").expect("");
    url.set_query(
        Some(
            &vec!["device_id", &encode(dst_idkey).into_owned()]
            .join("=")
        )
    );
    self.client.get(url)
        .send()
        .await?
        .json()
        .await
  }

  pub async fn delete_messages_from_server(
      &self,
      to_delete: &ToDelete
  ) -> Result<Response> {
    self.client.delete(self.base_url.join("/self/messages").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
        .json(&to_delete)
        .send()
        .await
  }

  pub async fn add_otkeys_to_server<'a>(
      &self,
      to_add: &HashMap<String, String>
  ) -> Result<Response> {
    self.client.post(self.base_url.join("/self/otkeys").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", &self.idkey].join(" "))
        .json(&to_add)
        .send()
        .await
  }
}

impl Stream for ServerComm {
  type Item = eventsource_client::Result<Event>;

  fn poll_next(
      mut self: Pin<&mut Self>,
      cx: &mut Context<'_>
  ) -> Poll<Option<Self::Item>> {
    let event = self.listener.as_mut().poll_next(cx);
    match event {
      Poll::Pending => Poll::Pending,
      Poll::Ready(None) => Poll::Pending,
      Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
      Poll::Ready(Some(Ok(event))) => match event {
        SSE::Comment(_) => Poll::Pending,
        SSE::Event(event) => {
          match event.event_type.as_str() {
            "otkey" => Poll::Ready(Some(Ok(Event::Otkey))),
            "msg" => {
              let msg = String::from(event.data);
              Poll::Ready(Some(Ok(Event::Msg(msg))))
            },
            _ => Poll::Pending,
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{Event, ServerComm, Batch, OutgoingMessage, IncomingMessage, ToDelete, Payload};
  use futures::TryStreamExt;
  use crate::olm_wrapper::OlmWrapper;

  #[tokio::test]
  async fn test_new() {
    assert_eq!(
        ServerComm::new(None, None, "abcd".to_string()).try_next().await,
        Ok(Some(Event::Otkey))
    );
  }

  #[tokio::test]
  async fn test_init() {
    let olm_wrapper = OlmWrapper::new(false);
    let server_comm = ServerComm::init(None, None, &olm_wrapper).await;
    println!("server_comm.idkey: {:?}", server_comm.idkey);
  }

  #[tokio::test]
  async fn test_send_message() {
    let idkey = String::from("efgh");
    let payload = String::from("hello");
    let batch = Batch::from_vec(vec![OutgoingMessage::new(
        idkey.clone(),
        Payload::new(0, payload.clone())
    )]);

    let mut server_comm = ServerComm::new(None, None, idkey.clone());
    assert_eq!(server_comm.try_next().await, Ok(Some(Event::Otkey)));

    match server_comm.send_message(&batch).await {
      Ok(_) => {
        match server_comm.try_next().await {
          Ok(Some(Event::Msg(msg_string))) => {
            let msg: IncomingMessage =
                serde_json::from_str(msg_string.as_str()).unwrap();
            assert_eq!(msg.sender, idkey);
            assert_eq!(msg.payload.ciphertext, payload);
          },
          Ok(Some(Event::Otkey)) => panic!("Got otkey event"),
          Ok(None) => panic!("Got none"),
          Err(err) => panic!("Got error: {:?}", err),
        }
      },
      Err(err) => panic!("Error sending message: {:?}", err),
    }
  }

  #[tokio::test]
  async fn test_delete_messages() {
    let idkey = String::from("ijkl");
    let payload = String::from("hello");
    let batch = Batch::from_vec(vec![OutgoingMessage::new(
        idkey.clone(),
        Payload::new(0, payload.clone())
    )]);

    let mut server_comm = ServerComm::new(None, None, idkey.clone());
    assert_eq!(server_comm.try_next().await, Ok(Some(Event::Otkey)));

    match server_comm.send_message(&batch).await {
      Ok(_) => {
        match server_comm.try_next().await {
          Ok(Some(Event::Msg(msg_string))) => {
            let msg: IncomingMessage =
                serde_json::from_str(msg_string.as_str()).unwrap();
            assert_eq!(msg.sender, idkey);
            assert_eq!(msg.payload.ciphertext, payload);
            assert!(msg.seq_id > 0);
            println!("msg.seq_id: {:?}", msg.seq_id);

            match server_comm.delete_messages_from_server(
                &ToDelete::from_seq_id(msg.seq_id)
            ).await {
              Ok(_) => println!("Sent delete-message successfully"),
              Err(err) => panic!("Error sending delete-message: {:?}", err),
            }
          },
          Ok(Some(Event::Otkey)) => panic!("Got otkey event"),
          Ok(None) => panic!("Got none"),
          Err(err) => panic!("Got error: {:?}", err),
        }
      },
      Err(err) => panic!("Error sending message: {:?}", err),
    }
  }

  #[tokio::test]
  async fn test_add_otkeys_to_server() {
    let olm_wrapper = OlmWrapper::new(false);
    let idkey = olm_wrapper.get_idkey();
    let mut server_comm = ServerComm::new(None, None, idkey);
    match server_comm.try_next().await {
      Ok(Some(Event::Otkey)) => {
        let otkeys = olm_wrapper.generate_otkeys(None);
        println!("otkeys: {:?}", otkeys);
        match server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
          Ok(_) => println!("Sent otkeys successfully"),
          Err(err) => panic!("Error sending otkeys: {:?}", err),
        }
      },
      _ => panic!("Unexpected result"),
    }
  }

  #[tokio::test]
  async fn test_get_otkey_from_server() {
    let olm_wrapper = OlmWrapper::new(false);
    let idkey = olm_wrapper.get_idkey();
    let otkeys = olm_wrapper.generate_otkeys(None);
    let mut values = otkeys.curve25519().values().cloned();
    let mut server_comm = ServerComm::new(None, None, idkey.clone());
    match server_comm.try_next().await {
      Ok(Some(Event::Otkey)) => {
        match server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
          Ok(_) => println!("Sent otkeys successfully"),
          Err(err) => panic!("Error sending otkeys: {:?}", err),
        }
      },
      _ => panic!("Unexpected result"),
    }
    match server_comm.get_otkey_from_server(&idkey).await {
      Ok(res) => {
        println!("otkey: {:?}", res);
        assert!(values.any(|x| x.eq(&res.otkey)));
      },
      Err(err) => panic!("Error getting otkey: {:?}", err),
    }
  }
}
