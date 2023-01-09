mod olm_wrapper;
mod server_comm;

use reqwest::{Result, Response};

struct Core<'a> {
  //crypto: olm_wrapper::OlmWrapper,
  server_comm: server_comm::ServerComm<'a>,
}

impl<'a> Core<'a> {
  fn new(idkey: &'a str) -> Self {
    //let crypto = olm_wrapper::OlmWrapper::new(None, None, None);
    Self {
      //server_comm: server_comm::ServerComm::new(None, None, idkey, crypto),
      server_comm: server_comm::ServerComm::new(None, None, idkey),
    }
  }

  async fn send_message(&self, dest_idkeys: Vec<&str>, payload: &str) -> Result<Response> {
    let mut batch = server_comm::Batch::new();
    for idkey in dest_idkeys {
      batch.push(server_comm::OutgoingMessage::new(idkey, payload));
    }
    self.server_comm.send_message(&batch).await
  }

  //fn on_message(&self, msg: &str) {
  //  println!("RECEIVED: {}", msg);
  //}
}

#[cfg(test)]
mod tests {
  use super::{Core, server_comm};
  use futures::TryStreamExt;

  #[tokio::test]
  async fn test_core_init() {
    let idkey = "abcd";
    let mut core = Core::new(idkey);
    assert_eq!(core.server_comm.try_next().await, Ok(Some(server_comm::Event::Otkey)));
  }

  #[tokio::test]
  async fn test_core_send_message() {
    let idkey = "efgh";
    let recipients = vec![idkey];
    let payload = "hello from me";
    let mut core = Core::new(idkey);
    assert_eq!(core.server_comm.try_next().await, Ok(Some(server_comm::Event::Otkey)));
    match core.send_message(recipients, payload).await {
      Ok(res) => println!("response: {:?}", res),
      Err(err) => println!("error: {:?}", err),
    }
    match core.server_comm.try_next().await {
      Ok(Some(server_comm::Event::Msg(msg_string))) => {
        println!("msg_string: {:?}", msg_string);
      },
      _ => println!("unexpected result"),
    }
  }
}
