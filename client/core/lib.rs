#![feature(is_sorted)]

mod olm_wrapper;
mod server_comm;
mod hash_vectors;

use reqwest::{Result, Response};
use serde::{Deserialize, Serialize};

use olm_wrapper::OlmWrapper;
use server_comm::{ServerComm, Batch, OutgoingMessage, Payload};
use hash_vectors::{HashVectors, CommonPayload, RecipientPayload};

#[derive(Debug, Serialize, Deserialize)]
struct FullPayload {
  common: CommonPayload,
  per_recipient: RecipientPayload,
}

impl FullPayload {
  fn new(
      common: CommonPayload,
      per_recipient: RecipientPayload
  ) -> FullPayload {
    Self { common, per_recipient }
  }

  fn to_string(
      common: CommonPayload,
      per_recipient: RecipientPayload
  ) -> String {
    serde_json::to_string(
        &FullPayload::new(common.clone(), per_recipient)
    ).unwrap()
  }

  fn from_string(msg: String) -> FullPayload {
    serde_json::from_str(msg.as_str()).unwrap()
  }

  fn common(&self) -> &CommonPayload {
    &self.common
  }

  fn per_recipient(&self) -> &RecipientPayload {
    &self.per_recipient
  }
}

struct Core {
  olm_wrapper: OlmWrapper,
  server_comm: ServerComm,
  hash_vectors: HashVectors,
}

// TODO event emitter

impl Core {
  async fn new() -> Core {
    let olm_wrapper = OlmWrapper::new(None);
    let server_comm = ServerComm::init(None, None, &olm_wrapper).await;
    let hash_vectors = HashVectors::new(olm_wrapper.get_idkey());
    Core {
      olm_wrapper,
      server_comm,
      hash_vectors
    }
  }

  async fn send_message(
      &mut self,
      dst_idkeys: Vec<String>,
      payload: &String
  ) -> Result<Response> {
    let (common_payload, recipient_payloads) =
        self.hash_vectors.prepare_message(
            dst_idkeys.clone(),
            payload.to_string()
        );
    let mut batch = Batch::new();
    for (idkey, recipient_payload) in recipient_payloads {
      let full_payload = FullPayload::to_string(
          common_payload.clone(),
          recipient_payload
      );

      let (c_type, ciphertext) = self.olm_wrapper.encrypt(
          &self.server_comm,
          &full_payload,
          &idkey,
      ).await;

      batch.push(
          OutgoingMessage::new(
              idkey,
              Payload::new(c_type, ciphertext)
          )
      );
    }
    println!("\nbatch: {:#?}\n", batch);
    self.server_comm.send_message(&batch).await
  }

  /*async fn get_events(&mut self) {
    use futures::TryStreamExt;

    match self.server_comm.try_next().await {
      Ok(Some(server_comm::Event::Msg(msg))) => {
        // TODO deserialize message
        println!("msg: {:?}", msg);
      },
      Ok(Some(server_comm::Event::Otkey)) => {
        // TODO add otkeys
        println!("TODO need more otkeys");
      },
      Ok(None) => panic!("Got <None> event from server"),
      Err(err) => panic!("Got error while awaiting events from server: {:?}", err),
    }
  }*/
}

// TODO in all fxn signatures, have sender/idkeys come first in param list

#[cfg(test)]
mod tests {
  use super::{Core, server_comm, FullPayload};
  use futures::TryStreamExt;

  #[tokio::test]
  async fn test_new() {
    let _ = Core::new().await;
  }

  #[tokio::test]
  async fn test_send_message_to_self() {
    let payload = String::from("hello from me");
    let mut core = Core::new().await;
    let idkey = core.olm_wrapper.get_idkey();
    let recipients = vec![idkey];

    match core.send_message(recipients, &payload).await {
      Ok(_) => {
        match core.server_comm.try_next().await {
          Ok(Some(server_comm::Event::Msg(msg_string))) => {
            let msg: server_comm::IncomingMessage = 
                server_comm::IncomingMessage::from_string(msg_string);

            let decrypted = core.olm_wrapper.decrypt(
              msg.payload().c_type(),
              &msg.payload().ciphertext(),
              &msg.sender()
            );

            let full_payload = FullPayload::from_string(decrypted);
            assert_eq!(*full_payload.common().message(), payload);
          },
          Ok(Some(server_comm::Event::Otkey)) => panic!("FAIL got otkey event"),
          Ok(None) => panic!("FAIL got none"),
          Err(err) => panic!("FAIL got error: {:?}", err),
        }
      },
      Err(err) => panic!("Error sending message: {:?}", err),
    }
  }

  /*
  #[tokio::test]
  async fn test_send_message_to_other() {
    let payload = String::from("hello from me");
    let mut core_0 = Core::new().await;
    let mut core_1 = Core::new().await;
    //let idkey_0 = core_0.olm_wrapper.get_idkey();
    let idkey_1 = core_1.olm_wrapper.get_idkey();
    let recipients = vec![idkey_1];

    match core_0.send_message(recipients, &payload).await {
      Ok(_) => {
        match core_1.server_comm.try_next().await {
          Ok(Some(server_comm::Event::Msg(msg_string))) => {
            println!("msg_string: {:?}", msg_string);
          },
          Ok(Some(server_comm::Event::Otkey)) => panic!("FAIL got otkey event"),
          Ok(None) => panic!("FAIL got none"),
          Err(err) => panic!("FAIL got error: {:?}", err),
        }
      },
      Err(err) => panic!("error: {:?}", err),
    }
  }
  */
}
