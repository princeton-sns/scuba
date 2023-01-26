use olm_rs::account::{OlmAccount, IdentityKeys, OneTimeKeys};
use olm_rs::session::{OlmMessage, OlmSession, PreKeyMessage};
use std::collections::HashMap;
use crate::server_comm::ServerComm;

// TODO sender-key optimization

const NUM_OTKEYS : usize = 20;

// TODO persist natively
pub struct OlmWrapper {
  toggle_off   : bool,
  idkeys       : IdentityKeys,
  account      : OlmAccount,
  message_queue: Vec<String>,
  sessions     : HashMap<String, Vec<OlmSession>>,
}

// TODO impl Error enum

impl OlmWrapper {
  pub fn new(toggle_arg: Option<bool>) -> Self {
    let account = OlmAccount::new();
    Self {
      toggle_off: toggle_arg.unwrap_or(false),
      idkeys: account.parsed_identity_keys(),
      account,
      message_queue: Vec::new(),
      sessions: HashMap::new(),
    }
  }

  pub fn generate_otkeys(&self, num: Option<usize>) -> OneTimeKeys {
    self.account.generate_one_time_keys(num.unwrap_or(NUM_OTKEYS));
    let otkeys = self.account.parsed_one_time_keys();
    self.account.mark_keys_as_published();
    otkeys
  }

  pub fn get_idkey(&self) -> String {
    self.idkeys.curve25519().to_string()
  }

  async fn new_outbound_session(
      &self,
      server_comm: &ServerComm,
      dst_idkey: &String
  ) -> OlmSession {
    match server_comm.get_otkey_from_server(dst_idkey).await {
      Ok(dst_otkey) => {
        match self.account.create_outbound_session(dst_idkey, &String::from(dst_otkey)) {
          Ok(new_session) => return new_session,
          Err(err) => panic!("Error creating outbound session: {:?}", err),
        }
      },
      Err(err) => panic!("Error getting otkey from server: {:?}", err),
    }
  }

  fn new_inbound_session(
      &self,
      prekey_msg: &PreKeyMessage
  ) -> OlmSession {
    match self.account.create_inbound_session(prekey_msg.clone()) {
      Ok(new_session) => return new_session,
      Err(err) => panic!("Error creating inbound session: {:?}", err),
    }
  }

  // TODO how many sessions with the same session_id should exist at one time? 
  // (for decrypting delayed messages) -> currently infinite

  async fn get_outbound_session(
      &mut self,
      server_comm: &ServerComm,
      dst_idkey: &String
  ) -> &OlmSession {
    if let None = self.sessions.get(dst_idkey) {
      self.sessions.insert(
          dst_idkey.to_string(),
          vec![self.new_outbound_session(server_comm, dst_idkey).await]
      );
    } else {
      let sessions_list = self.sessions.get_mut(dst_idkey).unwrap();
      if sessions_list.is_empty() || !sessions_list[sessions_list.len() - 1].has_received_message() {
        let session = self.new_outbound_session(server_comm, dst_idkey).await;
        self.sessions.get_mut(dst_idkey).unwrap().push(session);
      }
    }
    let sessions_list = self.sessions.get(dst_idkey).unwrap();
    &sessions_list[sessions_list.len() - 1]
  }

  fn get_inbound_session(
      &mut self,
      sender: &String,
      ciphertext: &OlmMessage,
  ) -> &OlmSession {
    match ciphertext {
      OlmMessage::Message(_) => {
        if let None = self.sessions.get(sender) {
          panic!("No pairwise sessions exist for idkey {:?}", sender);
        } else {
          let sessions_list = self.sessions.get_mut(sender).unwrap();
          return &sessions_list[sessions_list.len() - 1];
        }
      },
      OlmMessage::PreKey(prekey) => {
        if let None = self.sessions.get(sender) {
          self.sessions.insert(
              sender.to_string(),
              vec![self.new_inbound_session(&prekey)]
          );
        } else {
          let new_session = self.new_inbound_session(&prekey);
          self.sessions.get_mut(sender).unwrap().push(new_session);
        }
        let sessions_list = self.sessions.get(sender).unwrap();
        &sessions_list[sessions_list.len() - 1]
      },
    }
  }

  fn try_all_sessions_decrypt(
      &mut self,
      sender: &String,
      ciphertext: &OlmMessage,
  ) -> String {
    // as long as get_inbound_session is called before this function the result
    // will never be None/empty
    let sessions_list = self.sessions.get(sender).unwrap();

    // skip the len - 1'th session since that was already tried
    for session in sessions_list.iter().rev().skip(1) {
      match session.decrypt(ciphertext.clone()) {
        Ok(plaintext) => return plaintext,
        _ => continue,
      }
    }
    panic!("No matching sessions were found");
  }

  pub async fn encrypt(
      &mut self,
      server_comm: &ServerComm,
      dst_idkey: &String,
      plaintext: &String,
  ) -> (usize, String) {
    if self.toggle_off {
      return (1, plaintext.to_string());
    }
    self.encrypt_helper(server_comm, dst_idkey, plaintext).await
  }

  async fn encrypt_helper(
      &mut self,
      server_comm: &ServerComm,
      dst_idkey: &String,
      plaintext: &String,
  ) -> (usize, String) {
    if *dst_idkey == self.get_idkey() {
      self.message_queue.push(plaintext.to_string());
      return (1, "".to_string());
    }
    let session = self.get_outbound_session(server_comm, dst_idkey).await;
    let (c_type, ciphertext) = session.encrypt(plaintext).to_tuple();
    (c_type.into(), ciphertext)
  }

  pub fn decrypt(
      &mut self,
      sender: &String,
      c_type: usize,
      ciphertext: &String,
  ) -> String {
    if self.toggle_off {
      return ciphertext.to_string();
    }
    self.decrypt_helper(
        sender,
        &OlmMessage::from_type_and_ciphertext(
            c_type,
            ciphertext.to_string()
        ).unwrap(),
    )
  }

  fn decrypt_helper(
      &mut self,
      sender: &String,
      ciphertext: &OlmMessage,
  ) -> String {
    if *sender == self.get_idkey() {
      // FIXME handle dos attack where client poses as "self" - this
      // unwrap will panic
      return self.message_queue.pop().unwrap().to_string();
    }
    let session = self.get_inbound_session(sender, ciphertext);
    match session.decrypt(ciphertext.clone()) {
      Ok(plaintext) => return plaintext,
      Err(err) => {
        match ciphertext {
          // iterate through all sessions in case this message was delayed
          OlmMessage::Message(_) => return self.try_all_sessions_decrypt(sender, ciphertext),
          OlmMessage::PreKey(_) => panic!("Error creating inbound session from prekey message: {:?}", err),
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{OlmWrapper, NUM_OTKEYS};
  use crate::server_comm::ServerComm;

  #[test]
  fn test_new() {
    let olm_wrapper = OlmWrapper::new(None);
    assert_eq!(olm_wrapper.toggle_off, false);
  }

  #[test]
  fn test_idkey() {
    let olm_wrapper = OlmWrapper::new(None);
    println!("idkey: {:?}", olm_wrapper.get_idkey());
  }

  #[test]
  fn test_gen_otkeys() {
    let olm_wrapper = OlmWrapper::new(None);
    let otkeys = olm_wrapper.generate_otkeys(None);
    assert_eq!(NUM_OTKEYS, otkeys.curve25519().len());
    println!("otkeys: {:?}", otkeys.curve25519());
  }

  #[test]
  fn test_gen_otkeys_custom_num() {
    let num = 7;
    let olm_wrapper = OlmWrapper::new(None);
    let otkeys = olm_wrapper.generate_otkeys(Some(num));
    assert_eq!(num, otkeys.curve25519().len());
    println!("otkeys: {:?}", otkeys.curve25519());
  }

  #[tokio::test]
  async fn test_dummy_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(Some(true));
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext = String::from("hello");
    let (_, ciphertext) = olm_wrapper.encrypt(&server_comm, &idkey, &plaintext)
        .await;
    assert_eq!(plaintext, ciphertext);
  }

  #[tokio::test]
  async fn test_self_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext = String::from("hello");
    let empty = String::from("");
    let (_, ciphertext) = olm_wrapper.encrypt(&server_comm, &idkey, &plaintext)
        .await;
    assert_eq!(empty, ciphertext);
    assert_eq!(plaintext, olm_wrapper.message_queue.pop().unwrap());
  }

  #[test]
  fn test_dummy_decrypt() {
    let mut olm_wrapper = OlmWrapper::new(Some(true));
    let idkey = olm_wrapper.get_idkey();
    let plaintext: &str = "hello";
    let decrypted = olm_wrapper.decrypt(&idkey, 1, &plaintext.to_string());
    assert_eq!(plaintext, decrypted);
  }

  #[tokio::test]
  async fn test_self_decrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext = String::from("hello");
    let empty = String::from("");
    let (c_type, ciphertext) = olm_wrapper.encrypt(&server_comm, &idkey, &plaintext).await;
    let decrypted = olm_wrapper.decrypt(&idkey, c_type, &ciphertext);
    assert_eq!(empty, ciphertext);
    assert_eq!(plaintext, decrypted);
  }

  #[tokio::test]
  async fn test_self_outbound_session() {
    let olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::init(None, None, &olm_wrapper).await;
    let session = olm_wrapper.new_outbound_session(&server_comm, &idkey).await;
    println!("New session: {:?}", session);
    println!("New session ID: {:?}", session.session_id());
    assert!(!session.has_received_message());
  }

  #[tokio::test]
  async fn test_encrypt_and_decrypt_once() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let _ = ServerComm::init(None, None, &ow2).await;

    let plaintext = String::from("testing testing one two three");

    let (c_type, ciphertext) = ow1.encrypt(&sc1, &idkey2, &plaintext).await;
    let decrypted = ow2.decrypt(&idkey1, c_type, &ciphertext);

    assert_eq!(plaintext, decrypted);
  }

  #[tokio::test]
  async fn test_get_session_init() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let _ = ServerComm::init(None, None, &ow2).await;

    let plaintext = "testing testing one two three";

    // 1 -> 2
    assert_eq!(None, ow1.sessions.get(&idkey2));
    assert_eq!(None, ow2.sessions.get(&idkey1));

    let ob_session = ow1.get_outbound_session(&sc1, &idkey2).await;
    let ciphertext = ob_session.encrypt(plaintext);

    // using prekey
    let ib_session = ow2.get_inbound_session(&idkey1, &ciphertext);

    assert_eq!(ob_session.session_id(), ib_session.session_id());

    let ow1_session_list = ow1.sessions.get(&idkey2);
    let ow2_session_list = ow2.sessions.get(&idkey1);

    assert_ne!(None, ow1_session_list);
    assert_ne!(None, ow2_session_list);
    assert_eq!(ow1_session_list.unwrap().len(), 1);
    assert_eq!(ow2_session_list.unwrap().len(), 1);
  }

  #[tokio::test]
  async fn test_get_session_without_received_msg() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let _ = ServerComm::init(None, None, &ow2).await;

    let plaintext = "testing testing one two three";

    // 1 -> 2
    assert_eq!(None, ow1.sessions.get(&idkey2));
    assert_eq!(None, ow2.sessions.get(&idkey1));

    let first_ob_session = ow1.get_outbound_session(&sc1, &idkey2).await;
    let ciphertext = first_ob_session.encrypt(plaintext);
    // using prekey
    let first_ib_session = ow2.get_inbound_session(&idkey1, &ciphertext);

    // decrypt() sets flag for has_received_message()
    let decrypted = first_ib_session.decrypt(ciphertext.clone()).unwrap();
    assert_eq!(plaintext, decrypted);

    let first_ob_id = first_ob_session.session_id().clone();
    let first_ib_id = first_ib_session.session_id().clone();

    // 1 -> 2 again
    let second_ob_session = ow1.get_outbound_session(&sc1, &idkey2).await;
    let ciphertext = second_ob_session.encrypt(plaintext);
    // using prekey
    let second_ib_session = ow2.get_inbound_session(&idkey1, &ciphertext);

    let second_ob_id = second_ob_session.session_id().clone();
    let second_ib_id = second_ib_session.session_id().clone();

    assert_eq!(first_ob_id, first_ib_id);
    assert_eq!(second_ob_id, second_ib_id);
    assert_ne!(first_ob_id, second_ob_id);
    assert_ne!(first_ib_id, second_ib_id);
  }

  #[tokio::test]
  async fn test_get_session_with_received_msg() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let sc2 = ServerComm::init(None, None, &ow2).await;

    let plaintext = "testing testing one two three";

    // 1 -> 2
    assert_eq!(None, ow1.sessions.get(&idkey2));
    assert_eq!(None, ow2.sessions.get(&idkey1));

    let first_ob_session = ow1.get_outbound_session(&sc1, &idkey2).await;
    let first_ciphertext = first_ob_session.encrypt(plaintext);
    // using prekey
    let first_ib_session = ow2.get_inbound_session(&idkey1, &first_ciphertext);

    // decrypt() sets flag for has_received_message()
    let decrypted = first_ib_session.decrypt(first_ciphertext.clone()).unwrap();
    assert_eq!(plaintext, decrypted);

    let first_ob_id = first_ob_session.session_id().clone();
    let first_ib_id = first_ib_session.session_id().clone();

    // 2 -> 1
    let second_ob_session = ow2.get_outbound_session(&sc2, &idkey1).await;
    let second_ciphertext = second_ob_session.encrypt(plaintext);
    // using message
    let second_ib_session = ow1.get_inbound_session(&idkey2, &second_ciphertext);

    let second_ob_id = second_ob_session.session_id().clone();
    let second_ib_id = second_ib_session.session_id().clone();

    assert_eq!(first_ob_id, first_ib_id);
    assert_eq!(second_ob_id, second_ib_id);
    assert_eq!(first_ob_id, second_ib_id);
    assert_eq!(first_ib_id, second_ob_id);
  }

  #[tokio::test]
  async fn test_encrypt_and_decrypt_without_received_msg() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let _ = ServerComm::init(None, None, &ow2).await;

    // 1 -> 2
    let first_plaintext = String::from("testing testing one two three");
    let (first_ctype, first_ciphertext) = ow1.encrypt(&sc1, &idkey2, &first_plaintext).await;
    let first_decrypted = ow2.decrypt(&idkey1, first_ctype, &first_ciphertext);
    assert_eq!(first_plaintext, first_decrypted);

    // 1 -> 2
    let second_plaintext = String::from("three two one testing testing");
    let (second_ctype, second_ciphertext) = ow1.encrypt(&sc1, &idkey2, &second_plaintext).await;
    let second_decrypted = ow2.decrypt(&idkey1, second_ctype, &second_ciphertext);
    assert_eq!(second_plaintext, second_decrypted);
  }

  #[tokio::test]
  async fn test_encrypt_and_decrypt_with_received_msg() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let sc2 = ServerComm::init(None, None, &ow2).await;

    // 1 -> 2
    let first_plaintext = String::from("testing testing one two three");
    let (first_ctype, first_ciphertext) = ow1.encrypt(&sc1, &idkey2, &first_plaintext).await;
    let first_decrypted = ow2.decrypt(&idkey1, first_ctype, &first_ciphertext);
    assert_eq!(first_plaintext, first_decrypted);

    // 2 -> 1
    let second_plaintext = String::from("three two one testing testing");
    let (second_ctype, second_ciphertext) = ow2.encrypt(&sc2, &idkey1, &second_plaintext).await;
    let second_decrypted = ow1.decrypt(&idkey2, second_ctype, &second_ciphertext);
    assert_eq!(second_plaintext, second_decrypted);
  }

  #[tokio::test]
  async fn test_delayed_message() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let sc2 = ServerComm::init(None, None, &ow2).await;

    // encrypt 1 -> 2 and "send" (decrypt)
    let first_plaintext = String::from("testing testing one two three");
    let (first_ctype, first_ciphertext) = ow1.encrypt(&sc1, &idkey2, &first_plaintext).await;
    let first_decrypted = ow2.decrypt(&idkey1, first_ctype, &first_ciphertext);
    assert_eq!(first_plaintext, first_decrypted);

    // encrypt another 1 -> 2 without "sending" (decrypting) - uses a diff session
    // b/c has not yet received a response
    let second_plaintext = String::from("three two one testing testing");
    let (second_ctype, second_ciphertext) = ow1.encrypt(&sc1, &idkey2, &second_plaintext).await;

    // encrypt 2 -> 1 and "send" (decrypt)
    let third_plaintext = String::from("one testing three testing two");
    let (third_ctype, third_ciphertext) = ow2.encrypt(&sc2, &idkey1, &third_plaintext).await;
    let third_decrypted = ow1.decrypt(&idkey2, third_ctype, &third_ciphertext);
    assert_eq!(third_plaintext, third_decrypted);

    // "send" (decrypt) second message
    let second_decrypted = ow2.decrypt(&idkey1, second_ctype, &second_ciphertext);
    assert_eq!(second_plaintext, second_decrypted);
  }

  #[tokio::test]
  async fn test_very_delayed_message() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let sc2 = ServerComm::init(None, None, &ow2).await;

    let plaintext = String::from("testing testing one two three");

    // encrypt 1 -> 2 and "send" (decrypt)
    let (first_ctype, first_ciphertext) = ow1.encrypt(&sc1, &idkey2, &plaintext).await;
    let first_decrypted = ow2.decrypt(&idkey1, first_ctype, &first_ciphertext);
    assert_eq!(plaintext, first_decrypted);

    // encrypt another 1 -> 2 without "sending" (decrypting) - uses a diff session
    // b/c has not yet received a response
    let (second_ctype, second_ciphertext) = ow1.encrypt(&sc1, &idkey2, &plaintext).await;

    // encrypt 2 -> 1 and "send" (decrypt)
    let (third_ctype, third_ciphertext) = ow2.encrypt(&sc2, &idkey1, &plaintext).await;
    let third_decrypted = ow1.decrypt(&idkey2, third_ctype, &third_ciphertext);
    assert_eq!(plaintext, third_decrypted);

    // encrypt another 2 -> 1 and "send" (decrypt)
    let (fourth_ctype, fourth_ciphertext) = ow2.encrypt(&sc2, &idkey1, &plaintext).await;
    let fourth_decrypted = ow1.decrypt(&idkey2, fourth_ctype, &fourth_ciphertext);
    assert_eq!(plaintext, fourth_decrypted);

    // encrypt another 2 -> 1 and "send" (decrypt)
    let (fifth_ctype, fifth_ciphertext) = ow2.encrypt(&sc2, &idkey1, &plaintext).await;
    let fifth_decrypted = ow1.decrypt(&idkey2, fifth_ctype, &fifth_ciphertext);
    assert_eq!(plaintext, fifth_decrypted);

    // encrypt another 2 -> 1 and "send" (decrypt)
    let (sixth_ctype, sixth_ciphertext) = ow2.encrypt(&sc2, &idkey1, &plaintext).await;
    let sixth_decrypted = ow1.decrypt(&idkey2, sixth_ctype, &sixth_ciphertext);
    assert_eq!(plaintext, sixth_decrypted);

    // "send" (decrypt) second message
    let second_decrypted = ow2.decrypt(&idkey1, second_ctype, &second_ciphertext);
    assert_eq!(plaintext, second_decrypted);
  }
}
