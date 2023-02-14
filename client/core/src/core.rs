use futures::channel::mpsc;
use reqwest::{Result, Response};
use serde::{Deserialize, Serialize};

use crate::olm_wrapper::OlmWrapper;
use crate::server_comm::{ServerComm, Batch, OutgoingMessage, Payload, Event, IncomingMessage, ToDelete};
use crate::hash_vectors::{HashVectors, CommonPayload, RecipientPayload};

// TODO persist natively

#[derive(Debug, Serialize, Deserialize)]
pub struct FullPayload {
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
  hash_vectors: Mutex<HashVectors>,
  sender: mpsc::Sender<(String, String)>,
}

impl Core {
  pub fn new<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
      sender: mpsc::Sender<(String, String)>,
  ) -> Core {
    let olm_wrapper = OlmWrapper::new(turn_encryption_off_arg);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(ip_arg, port_arg, idkey.clone());
    let hash_vectors = Mutex::new(HashVectors::new(idkey));

    Core { olm_wrapper, server_comm, hash_vectors, sender }
  }

  pub async fn new_and_init<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
      sender: mpsc::Sender<(String, String)>,
  ) -> Core {
    let olm_wrapper = OlmWrapper::new(turn_encryption_off_arg);
    let server_comm = ServerComm::init(ip_arg, port_arg, &olm_wrapper).await;
    let hash_vectors = Mutex::new(HashVectors::new(olm_wrapper.get_idkey()));

    Core { olm_wrapper, server_comm, hash_vectors, sender }
  }

  pub fn idkey(&self) -> String {
    self.olm_wrapper.get_idkey()
  }

  pub async fn send_message(
      &mut self,
      dst_idkeys: Vec<String>,
      payload: &String
  ) -> Result<Response> {
    let (common_payload, recipient_payloads) =
        self.hash_vectors.lock().unwrap().prepare_message(
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
    self.server_comm.send_message(&batch).await
  }

  // FIXME make immutable
  // self.olm_wrapper.need_mut_ref()
  // e.g. wrap olm_wrapper w Mutex (for now)
  // rule: never put a thing into a Mutex which calls some async functions
  // only wrap types that are used briefly, and make sure you unlock() before
  // calling any asyncs
  // TODO also, between unlock() and lock(), may have to recalculate any 
  // common vars to use
  pub async fn receive_message(&mut self) {
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

        // validate
        match self.hash_vectors.lock().unwrap().parse_message(
            &msg.sender(),
            full_payload.common,
            &full_payload.per_recipient
        ) {
          Ok(None) => println!("Validation succeeded, no message to process"),
          Ok(Some((seq, message))) => {
            // forward message
            // FIXME are callbacks easier to compile to wasm?
            self.sender.try_send((msg.sender().clone(), message));

            match self.server_comm.delete_messages_from_server(
                &ToDelete::from_seq_id(seq.try_into().unwrap())
            ).await {
              Ok(_) => println!("Sent delete-message successfully"),
              Err(err) => panic!("Error sending delete-message: {:?}", err),
            }
          },
          Err(err) => panic!("Validation failed: {:?}", err),
        }
      },
      Ok(Some(Event::Otkey)) => {
        println!("got otkey event from server");
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
  use futures::channel::mpsc;

  const BUFFER_SIZE: usize = 20;

  #[tokio::test]
  async fn test_new() {
    let (sender, _) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    let _ = Core::new(None, None, false, sender);
  }

  #[tokio::test]
  async fn test_new_and_init() {
    let (sender, _) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    let _ = Core::new_and_init(None, None, false, sender).await;
  }

  #[tokio::test]
  async fn test_send_message_to_self() {
    let payload = String::from("hello from me");
    let (sender, _) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    let mut core = Core::new_and_init(None, None, false, sender).await;
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
    let (sender, _) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    let mut core_0 = Core::new_and_init(None, None, false, sender.clone()).await;
    let mut core_1 = Core::new_and_init(None, None, false, sender).await;
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

  #[tokio::test]
  async fn test_handle_events() {
    let payload = String::from("hello from me");
    let (sender, mut receiver) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    let mut core = Core::new(None, None, false, sender.clone());
    // otkey
    core.receive_message().await;
    let idkey = core.olm_wrapper.get_idkey();
    let recipients = vec![idkey.clone()];

    match core.send_message(recipients, &payload).await {
      Ok(_) => println!("Message sent"),
      Err(err) => panic!("Error sending message: {:?}", err),
    }

    core.receive_message().await;

    match receiver.try_next().unwrap() {
      Some((sender, recv_payload)) => {
        assert_eq!(sender, idkey);
        assert_eq!(payload, recv_payload);
      },
      None => panic!("Got no message"),
    }
  }
}
