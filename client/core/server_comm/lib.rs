//use crate::crypto::OlmWrapper;
use std::pin::Pin;
use futures::{Stream, task::{Context,Poll}};
use std::collections::HashMap;
use url::Url;
use eventsource_client::Client;
use eventsource_client::ClientBuilder;
use eventsource_client::SSE;
use urlencoding::encode;
use reqwest::Result;
use reqwest::Response;
//use async_std::task;

const IP_ADDR    : &str = "localhost";
const PORT_NUM   : &str = "8080";
const HTTP_PREFIX: &str = "http://";
const COLON      : &str = ":";

pub struct ServerComm<'a> {
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
    // emitter
    idkey_arg: &'a str,
  ) -> Self {
    let ip_addr = ip_arg.unwrap_or(IP_ADDR);
    let port_num = port_arg.unwrap_or(PORT_NUM);
    let base_url = Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num].join("")).expect("");

    let listener = Box::new(ClientBuilder::for_url(base_url.join("/events").expect("").as_str()).expect("")
        .header("Authorization", &vec!["Bearer", idkey_arg].join(" ")).expect("")
        .build()).stream();
    Self {
      base_url,
      //crypto: crypto,
      idkey   : idkey_arg, //crypto.get_idkey(),
      client  : reqwest::Client::new(),
      listener,
    }
  }

  pub async fn send_message(&self, batch: HashMap::<&'a str, &'a str>) -> Result<Response> {
    self.client.post(self.base_url.join("/message").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .json(&batch)
        .send()
        .await
  }

  pub async fn get_otkey_from_server(&self, idkey: &'a str) -> Result<Response> {
    let mut url = self.base_url.join("/devices/otkey").expect("");
    url.set_query(Some(&vec!["device_id", &encode(idkey)].join("=")));
    self.client.get(url.as_str())
        .send()
        // TODO as JSON
        .await
    //  Ok(response) => println!("{:?}", response), // TODO return otkey
    //  Err(e) => println!("{:?}", e),
    //}
  }
}

#[derive(Eq, PartialEq, Debug)]
pub enum Event {
  Otkey,
  Msg(String),
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
            "otkey" => {
              Poll::Ready(Some(Ok(Event::Otkey)))
            },
            "msg" => {
              let msg = String::from(event.data); // TODO from JSON
              Poll::Ready(Some(Ok(Event::Msg(msg))))
            }
            _ => Poll::Pending,
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{Event, ServerComm};
  use std::collections::HashMap;
  use futures::TryStreamExt;

  #[tokio::test]
  async fn test_init_default() {
    assert_eq!(ServerComm::new(None, None, "abcd").try_next().await, Ok(Some(Event::Otkey)));
  }

  /*#[tokio::test]
  async fn test_send_simple() {
    let mut batch = HashMap::<&str, &str>::new();
    let payload = "hello from <abcd>";
    batch.insert("abcd", payload);
    match ServerComm::init(None, None, "abcd").await {
      Ok(server_comm) => {
        match server_comm.send_message(batch).await {
          Ok(res) => println!("OK: {:?}", res),
          Err(e) => println!("ERR: {:?}", e),
        }
      },
      Err(e) => println!("{:?}", e),
    }
  }

  #[tokio::test]
  async fn test_get_otkey() {
    match ServerComm::init(None, None, "abcd").await {
      Ok(sc1) => {
        match ServerComm::init(None, None, "efgh").await {
          Ok(sc2) => {
            match sc2.get_otkey_from_server(sc1.idkey).await {
              Ok(res) => println!("OK: {:?}", res),
              Err(e) => println!("ERR: {:?}", e),
            }
          },
          Err(e2) => println!("e2: {:?}", e2),
        }
      },
      Err(e1) => println!("e1: {:?}", e1),
    }
  }*/
}
