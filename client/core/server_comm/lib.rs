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
  test_toggle: bool,
  init_success: bool,
}

impl<'a> ServerComm<'a> {
  fn new(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    //crypto: &'a OlmWrapper,
    // emitter
    test_arg: Option<bool>,
  ) -> Self {
    let ip_addr = ip_arg.unwrap_or(IP_ADDR);
    let port_num = port_arg.unwrap_or(PORT_NUM);
    Self {
      ip_addr : ip_addr,
      port_num: port_num,
      base_url: Url::parse(&vec![HTTP_PREFIX, ip_addr, COLON, port_num].join("")).expect(""),
      //crypto: crypto,
      idkey   : "abcd", //crypto.get_idkey(),
      client  : reqwest::Client::new(),
      test_toggle: test_arg.unwrap_or(false),
      init_success: false,
    }
  }

  async fn init(
    ip_arg: Option<&'a str>,
    port_arg: Option<&'a str>,
    //crypto: &'a OlmWrapper,
    // emitter
    test_arg: Option<bool>,
  ) -> Result<ServerComm<'a>> {
    let mut sc = ServerComm::new(ip_arg, port_arg, test_arg);
    let listener = ClientBuilder::for_url(sc.base_url.as_str()).expect("")
        .header("Authorization", &vec!["Bearer", sc.idkey].join(" ")).expect("")
        .build();
    match sc.client.get(sc.base_url.join("/events").expect("").as_str())
        .header("Authorization", vec!["Bearer", sc.idkey].join(" "))
        .send().await {
      Ok(response) => sc.init_success = true,
      Err(e) => println!("{:?}", e),
    }

    let _ = Box::pin(listener.stream())
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
    Ok(sc)

  }

  async fn send_message(&self, batch: HashMap::<&'a str, &'a str>) {
    match self.client.post(self.base_url.join("/message").expect("").as_str())
        .header("Content-Type", "application/json")
        .header("Authorization", vec!["Bearer", self.idkey].join(" "))
        .json(&batch)
        .send()
        .await {
      Ok(response) => println!("{:?}", response),
      Err(e) => println!("{:?}", e),
    }
  }

  async fn get_otkey_from_server(&self, idkey: &'a str) {
    let mut url = self.base_url.join("/devices/otkey").expect("");
    url.set_query(Some(&vec!["device_id", &encode(idkey)].join("=")));
    match self.client.get(url.as_str())
        .send()
        // TODO as JSON
        .await {
      Ok(response) => println!("{:?}", response), // TODO return otkey
      Err(e) => println!("{:?}", e),
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::ServerComm;
  //use std::collections::HashMap;

  #[actix_rt::test]
  async fn test_init_default() {
    match ServerComm::init(None, None, None).await {
      Ok(server_comm) => {
        println!("server_comm: {:?}", server_comm);
        assert_eq!(server_comm.init_success, true);
      },
      Err(e) => println!("{:?}", e),
    }
  }

  //#[test]
  //fn test_send_simple() {
  //  let mut batch = HashMap::<&str, &str>::new();
  //  let payload = "hello from <abcd>";
  //  batch.insert("abcd", payload);
  //  println!("payload: {:?}", payload);
  //  let server_comm = ServerComm::init(None, None, None);
  //  server_comm.send_message(batch);
  //}
}
