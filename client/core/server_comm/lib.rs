#![feature(async_closure)]

//use crate::crypto::OlmWrapper;
use std::collections::HashMap;
use url::Url;
use eventsource_client::Client;
use eventsource_client::ClientBuilder;
use eventsource_client::SSE;
use urlencoding::encode;
use futures::TryStreamExt;
use reqwest::Result;
use reqwest::Response;

const IP_ADDR    : &str = "localhost";
const PORT_NUM   : &str = "8080";
const HTTP_PREFIX: &str = "http://";
const COLON      : &str = ":";

#[derive(Debug)]
struct ServerComm<'a> {
  ip_addr : &'a str,
  port_num: &'a str,
  base_url: Url,
  //crypto  : &'a OlmWrapper,
  idkey   : &'a str,
  client  : reqwest::Client,
  //event emitter
  init_success: bool,
}

impl<'a> ServerComm<'a> {
  fn new(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    //crypto: &'a OlmWrapper,
    // emitter
    idkey_arg: &'a str,
  ) -> Self {
    let ip_addr = ip_arg.unwrap_or(IP_ADDR);
    let port_num = port_arg.unwrap_or(PORT_NUM);
    Self {
      ip_addr : ip_addr,
      port_num: port_num,
      base_url: Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num].join("")).expect(""),
      //crypto: crypto,
      idkey   : idkey_arg, //crypto.get_idkey(),
      client  : reqwest::Client::new(),
      init_success: false,
    }
  }

  async fn init(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    //crypto: &'a OlmWrapper,
    // emitter
    idkey_arg: &'a str,
  ) -> Result<ServerComm<'a>> {
    let sc = ServerComm::new(ip_arg, port_arg, idkey_arg);
    let listener = ClientBuilder::for_url(sc.base_url.join("/events").expect("").as_str()).expect("")
        .header("Authorization", &vec!["Bearer", sc.idkey].join(" ")).expect("")
        .build();

    let mut stream = listener.stream()
        .map_ok(async move |event| match &event {
          SSE::Comment(comment) => println!("Got comment: {:?}", comment),
          SSE::Event(event) => {
            println!("Got event: {:?}", event);
            // FIXME
            let base_url = Url::parse(&vec![HTTP_PREFIX, sc.ip_addr, COLON, sc.port_num].join("")).expect("");
            let client = reqwest::Client::new();
            match event.event_type.as_str() {
              "otkey" => {
                match client.post(base_url.join("/self/otkeys").expect("").as_str())
                    .header("Content-Type", "application/json")
                    .header("Authorization", vec!["Bearer", sc.idkey].join(" "))
                    .json("more otkeys")
                    .send()
                    .await {
                  // TODO turn into JSON and get 'otkey' field
                  Ok(response) => println!("{:?}", response),
                  Err(e) => println!("{:?}", e),
                }
              },
              "msg" => {
                let msg = &event.data; // TODO from JSON
                println!("{:?}", msg);
                // TODO emit msg to core
                match client.delete(base_url.join("/self/messages").expect("").as_str())
                    .header("Content-Type", "application/json")
                    .header("Authorization", vec!["Bearer", sc.idkey].join(" "))
                    .json(&msg) // TODO .seqID and to JSON
                    .send()
                    .await {
                  Ok(response) => println!("{:?}", response),
                  Err(e) => println!("{:?}", e),
                }
              },
              &_ => println!("Unexpected event"),
            }
          },
        })
        .map_err(|e| println!("Error streaming events: {:?}", e));

    while let Ok(Some(_)) = stream.try_next().await {
      println!("waiting...");
    }

    Ok(sc)
  }

  async fn send_message(&self, batch: HashMap::<&'a str, &'a str>) -> Result<Response> {
    self.client.post(self.base_url.join("/message").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .json(&batch)
        .send()
        .await
  }

  async fn get_otkey_from_server(&self, idkey: &'a str) -> Result<Response> {
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

#[cfg(test)]
mod tests {
  use crate::ServerComm;
  use std::collections::HashMap;

  #[actix_rt::test]
  async fn test_init_default() {
    match ServerComm::init(None, None, "abcd").await {
      Ok(server_comm) => {
        assert_eq!(server_comm.init_success, true);
      },
      Err(e) => println!("{:?}", e),
    }
  }

  #[actix_rt::test]
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

  #[actix_rt::test]
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
  }
}
