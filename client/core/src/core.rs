use reqwest::{Result, Response};
use serde::{Deserialize, Serialize};

use crate::olm_wrapper::OlmWrapper;
use crate::server_comm::{ServerComm, Batch, OutgoingMessage, Payload, Event, IncomingMessage, ToDelete};
use crate::hash_vectors::{HashVectors, CommonPayload, RecipientPayload};

// TODO persist natively

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

pub struct Core {
  olm_wrapper: OlmWrapper,
  server_comm: ServerComm,
  hash_vectors: HashVectors,
}

impl Core {
  pub fn new() -> Core {
    let olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let hash_vectors = HashVectors::new(idkey);

    Core { olm_wrapper, server_comm, hash_vectors }
  }

  pub async fn new_and_init() -> Core {
    let olm_wrapper = OlmWrapper::new(None);
    let server_comm = ServerComm::init(None, None, &olm_wrapper).await;
    let hash_vectors = HashVectors::new(olm_wrapper.get_idkey());

    Core { olm_wrapper, server_comm, hash_vectors }
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
          &idkey,
          &full_payload,
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

  pub async fn handle_events(&mut self) {
    use futures::TryStreamExt;

    match self.server_comm.try_next().await {
      Ok(Some(Event::Msg(msg_string))) => {
        let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

        let decrypted = self.olm_wrapper.decrypt(
          &msg.sender(),
          msg.payload().c_type(),
          &msg.payload().ciphertext(),
        );

        let full_payload = FullPayload::from_string(decrypted);
        println!("full_payload: {:?}", full_payload);

        match self.server_comm.delete_messages_from_server(
            &ToDelete::from_seq_id(msg.seq_id())
        ).await {
          Ok(_) => println!("Sent delete-message successfully"),
          Err(err) => panic!("Error sending delete-message: {:?}", err),
        }
      },
      Ok(Some(Event::Otkey)) => {
        let otkeys = self.olm_wrapper.generate_otkeys(None);
        match self.server_comm.add_otkeys_to_server(&otkeys.curve25519()).await {
          Ok(_) => println!("Sent otkeys successfully"),
          Err(err) => panic!("Error sending otkeys: {:?}", err),
        }
      },
      Ok(None) => panic!("Got <None> event from server"),
      Err(err) => panic!("Got error while awaiting events from server: {:?}", err),
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::core::{Core, FullPayload};
  use crate::server_comm::{Event, IncomingMessage, ToDelete};
  use futures::TryStreamExt;

  #[tokio::test]
  async fn test_new() {
    let _ = Core::new();
  }

  #[tokio::test]
  async fn test_new_and_init() {
    let _ = Core::new_and_init().await;
  }

  #[tokio::test]
  async fn test_send_message_to_self() {
    let payload = String::from("hello from me");
    let mut core = Core::new_and_init().await;
    let idkey = core.olm_wrapper.get_idkey();
    let recipients = vec![idkey];

    match core.send_message(recipients, &payload).await {
      Ok(_) => {
        match core.server_comm.try_next().await {
          Ok(Some(Event::Msg(msg_string))) => {
            let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

            let decrypted = core.olm_wrapper.decrypt(
              &msg.sender(),
              msg.payload().c_type(),
              &msg.payload().ciphertext(),
            );

            let full_payload = FullPayload::from_string(decrypted);
            assert_eq!(*full_payload.common().message(), payload);

            match core.server_comm.delete_messages_from_server(
                &ToDelete::from_seq_id(msg.seq_id())
            ).await {
              Ok(_) => println!("Sent delete-message successfully"),
              Err(err) => panic!("Error sending delete-message: {:?}", err),
            }
          },
          Ok(Some(Event::Otkey)) => panic!("FAIL got otkey event"),
          Ok(None) => panic!("FAIL got none"),
          Err(err) => panic!("FAIL got error: {:?}", err),
        }
      },
      Err(err) => panic!("Error sending message: {:?}", err),
    }
  }

  #[tokio::test]
  async fn test_send_message_to_other() {
    let payload = String::from("hello from me");
    let mut core_0 = Core::new_and_init().await;
    let mut core_1 = Core::new_and_init().await;
    let idkey_1 = core_1.olm_wrapper.get_idkey();
    let recipients = vec![idkey_1];

    match core_0.send_message(recipients, &payload).await {
      Ok(_) => {
        match core_1.server_comm.try_next().await {
          Ok(Some(Event::Msg(msg_string))) => {
            let msg: IncomingMessage = IncomingMessage::from_string(msg_string);

            let decrypted = core_1.olm_wrapper.decrypt(
              &msg.sender(),
              msg.payload().c_type(),
              &msg.payload().ciphertext(),
            );

            let full_payload = FullPayload::from_string(decrypted);
            assert_eq!(*full_payload.common().message(), payload);
          },
          Ok(Some(Event::Otkey)) => panic!("FAIL got otkey event"),
          Ok(None) => panic!("FAIL got none"),
          Err(err) => panic!("FAIL got error: {:?}", err),
        }
      },
      Err(err) => panic!("error: {:?}", err),
    }
  }
}
