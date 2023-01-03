#![feature(async_closure)]

//use crate::crypto::OlmWrapper;
use std::pin::Pin;
use url::Url;
use eventsource_client::{Client, ClientBuilder, SSE};
use urlencoding::encode;
use futures::{Stream, task::{Context, Poll}};
use reqwest::{Result, Response};
use serde::{Deserialize, Serialize};

const IP_ADDR    : &str = "localhost";
const PORT_NUM   : &str = "8080";
const HTTP_PREFIX: &str = "http://";
const COLON      : &str = ":";

#[derive(PartialEq, Debug)]
pub enum Event {
  Otkey,
  Msg(String),
}

#[derive(Debug, Serialize)]
pub struct Batch<'a> {
  batch: Vec<OutgoingMessage<'a>>,
}

impl<'a> Batch<'a> {
  pub fn new() -> Self {
    Self {
      batch: Vec::<OutgoingMessage>::new(),
    }
  }

  pub fn from_vec(batch: Vec<OutgoingMessage<'a>>) -> Self {
    Self { batch }
  }

  pub fn push(&mut self, message: OutgoingMessage<'a>) {
    self.batch.push(message);
  }

  pub fn pop(&mut self) -> Option<OutgoingMessage<'a>> {
    self.batch.pop()
  }
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize)]
pub struct OutgoingMessage<'a> {
  deviceId: &'a str,
  payload: &'a str,
}

#[allow(non_snake_case)]
impl<'a> OutgoingMessage<'a> {
  pub fn new(deviceId: &'a str, payload: &'a str) -> Self {
    Self {
      deviceId,
      payload,
    }
  }
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
pub struct IncomingMessage<'a> {
  sender: &'a str,
  encPayload: &'a str,
  seqID: u64,
}

impl<'a> IncomingMessage<'a> {
  pub fn from_str(msg: &'a str) -> Self {
    serde_json::from_str(msg).unwrap()
  }
}

struct ServerComm<'a> {
  base_url: Url,
  //crypto  : &'a OlmWrapper,
  idkey   : &'a str,
  client  : reqwest::Client,
  listener: Pin<Box<dyn Stream<Item = eventsource_client::Result<SSE>>>>,
  //event emitter
}

impl<'a> ServerComm<'a> {
  pub fn new(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    //crypto: &'a OlmWrapper,
    //emitter
    idkey: &'a str,
  ) -> Self {
    let ip_addr = ip_arg.unwrap_or(IP_ADDR);
    let port_num = port_arg.unwrap_or(PORT_NUM);
    let base_url = Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num].join(""))
        .expect("Failed base_url construction");
    let listener = Box::new(
        ClientBuilder::for_url(base_url.join("/events").expect("Failed join of /events").as_str()).expect("Failed in ClientBuilder::for_url")
            .header("Authorization", &vec!["Bearer", idkey].join(" ")).expect("Failed header construction")
            .build()
        )
        .stream();
    Self {
      base_url,
      //crypto: crypto,
      idkey, //: crypto.get_idkey(),
      client  : reqwest::Client::new(),
      listener,
    }
  }

  pub async fn send_message(&self, batch: Batch<'a>) -> Result<Response> {
    self.client.post(self.base_url.join("/message").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .json(&batch)
        .send()
        .await
  }

  pub async fn get_otkey_from_server(&self, idkey: &'a str) -> Result<Response> {
    let mut url = self.base_url.join("/devices/otkey").expect("");
    url.set_query(Some(&vec!["deviceId", &encode(idkey)].join("=")));
    self.client.get(url.as_str())
        .send()
        .await
        //.json()
        //.await
        // TODO return otkey
  }

  async fn delete_messages_from_server(&self, msg: String) -> Result<Response> {
    self.client.delete(self.base_url.join("/self/messages").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .body(msg) // TODO .seqID
        .send()
        .await
  }

  async fn add_otkeys_to_server(&self) -> Result<Response> {
    self.client.post(self.base_url.join("/self/otkeys").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .json("more otkeys") // FIXME
        .send()
        .await
  }
}

impl<'a> Stream for ServerComm<'a> {
  type Item = eventsource_client::Result<Event>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
  use crate::{Event, ServerComm, Batch, OutgoingMessage, IncomingMessage};
  use futures::TryStreamExt;

  #[tokio::test]
  async fn test_init_default() {
    assert_eq!(ServerComm::new(None, None, "abcd").try_next().await, Ok(Some(Event::Otkey)));
  }

  //#[tokio::test]
  //async fn test_send_simple() {
  //  let idkey = "abcd";
  //  let payload = "hello";
  //  let batch = Batch::from_vec(vec![OutgoingMessage::new(idkey, payload)]);
  //  //println!("batch: {:?}", batch);
  //  let server_comm = ServerComm::new(None, None, idkey);
  //  match server_comm.send_message(batch).await {
  //    Ok(res) => println!("Send succeeded: {:?}", res),
  //    Err(err) => println!("Send failed: {:?}", err),
  //  }
  //}

  #[tokio::test]
  async fn test_init_and_send() {
    let idkey = "abcd";
    let payload = "hello";
    let batch = Batch::from_vec(vec![OutgoingMessage::new(idkey, payload)]);
    //println!("batch: {:?}", batch);
    //println!("string batch: {:?}", serde_json::to_string(&batch).unwrap());
    let mut server_comm = ServerComm::new(None, None, idkey);
    assert_eq!(server_comm.try_next().await, Ok(Some(Event::Otkey)));
    match server_comm.send_message(batch).await {
      Ok(_) => {
        match server_comm.try_next().await {
          Ok(Some(Event::Msg(msg_string))) => {
            //println!("msg: {:?}", msg_string);
            let msg: IncomingMessage<'_> = serde_json::from_str(msg_string.as_str()).unwrap();
            //println!("msg: {:?}", msg);
            assert_eq!(msg.sender, idkey);
            assert_eq!(msg.encPayload, payload);
          },
          Ok(Some(Event::Otkey)) => println!("FAIL got otkey event"),
          Ok(None) => println!("FAIL got none"),
          Err(err) => println!("FAIL got error: {:?}", err),
        }
      },
      Err(err) => println!("Send failed: {:?}", err),
    }
  }
}
