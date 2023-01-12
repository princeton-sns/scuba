mod olm_wrapper;
mod server_comm;

use reqwest::{Result, Response};

struct Core<'a> {
  olm_wrapper: olm_wrapper::OlmWrapper<'a>,
  server_comm: server_comm::ServerComm,
}

impl<'a> Core<'a> {
  async fn new() -> Core<'a> {
    let olm_wrapper = olm_wrapper::OlmWrapper::new(None);
    let server_comm = server_comm::ServerComm::init(None, None, &olm_wrapper).await;
    Self {
      olm_wrapper,
      server_comm,
    }
  }

  async fn send_message(&self, dst_idkeys: Vec<String>, payload: &String) -> Result<Response> {
    let mut batch = server_comm::Batch::new();
    for idkey in dst_idkeys {
      batch.push(server_comm::OutgoingMessage::new(&idkey, &payload.to_string()));
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
  async fn test_core_new() {
    let _ = Core::new().await;
  }

  #[tokio::test]
  async fn test_core_send_message() {
    let payload = String::from("hello from me");
    let mut core = Core::new().await;
    let idkey = core.olm_wrapper.get_idkey();
    let recipients = vec![idkey];
    match core.send_message(recipients, &payload).await {
      Ok(res) => println!("response: {:?}", res),
      Err(err) => panic!("error: {:?}", err),
    }
    match core.server_comm.try_next().await {
      Ok(Some(server_comm::Event::Msg(msg_string))) => {
        println!("msg_string: {:?}", msg_string);
      },
      Ok(Some(server_comm::Event::Otkey)) => panic!("FAIL got otkey event"),
      Ok(None) => panic!("FAIL got none"),
      Err(err) => panic!("FAIL got error: {:?}", err),
    }
  }
}
