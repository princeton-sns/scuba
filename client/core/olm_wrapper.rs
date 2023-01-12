use olm_rs::account::{OlmAccount, IdentityKeys, OneTimeKeys};
use olm_rs::session::{OlmMessage, OlmSession, PreKeyMessage};
use std::collections::HashMap;
use crate::server_comm::ServerComm;

const NUM_OTKEYS : usize = 10;

// TODO persist natively
pub struct OlmWrapper<'a> {
  toggle_off   : bool,
  idkeys       : IdentityKeys,
  account      : OlmAccount,
  message_queue: Vec<&'a str>,
  sessions     : HashMap<&'a str, Vec<OlmSession>>,
}

impl<'a> OlmWrapper<'a> {
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
      server_comm: &'a ServerComm,
      dst_idkey: &'a str
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
      prekey_msg: PreKeyMessage
  ) -> OlmSession {
    match self.account.create_inbound_session(prekey_msg) {
      Ok(new_session) => return new_session,
      Err(err) => panic!("Error creating inbound session: {:?}", err),
    }
  }

  async fn get_active_session(
      &mut self,
      server_comm: &'a ServerComm,
      dst_idkey: &'a str
  ) -> &OlmSession {
    let mut need_new_session = false;
    if let Some(sessions_list) = self.sessions.get_mut(dst_idkey) {
      if sessions_list.is_empty() || !sessions_list[sessions_list.len() - 1].has_received_message() {
        need_new_session = true;
      }
    } else {
      self.sessions.insert(dst_idkey, vec![self.new_outbound_session(server_comm, dst_idkey).await]);
    }
    // put here for the borrow checker
    if need_new_session {
      let session = self.new_outbound_session(server_comm, dst_idkey).await;
      self.sessions.get_mut(dst_idkey).unwrap().push(session);
    }
    let sessions_list = self.sessions.get(dst_idkey).unwrap();
    &sessions_list[sessions_list.len() - 1]
  }

  fn find_active_session(
      &mut self,
      ciphertext: OlmMessage,
      sender: &'a str
  ) -> OlmSession {
    println!("sender: {:?}", sender);
    match ciphertext {
      OlmMessage::Message(_) => panic!("Not yet implemented"),
      OlmMessage::PreKey(prekey) => return self.new_inbound_session(prekey),
    }
  }

  pub async fn encrypt(
      &mut self,
      server_comm: &'a ServerComm,
      plaintext: &'a str,
      dst_idkey: &'a str
  ) -> OlmMessage {
    if self.toggle_off {
      return OlmMessage::from_type_and_ciphertext(1, plaintext.to_string()).unwrap();
    }
    self.encrypt_helper(server_comm, plaintext, dst_idkey).await
  }

  async fn encrypt_helper(
      &mut self,
      server_comm: &'a ServerComm,
      plaintext: &'a str,
      dst_idkey: &'a str
  ) -> OlmMessage {
    if dst_idkey == self.get_idkey() {
      self.message_queue.push(plaintext);
      return OlmMessage::from_type_and_ciphertext(1, "".to_string()).unwrap();
    }
    let session = self.get_active_session(server_comm, dst_idkey).await;
    session.encrypt(plaintext)
  }

  pub fn decrypt(
      &mut self,
      ciphertext: OlmMessage,
      sender: &'a str
  ) -> String {
    if self.toggle_off {
      return ciphertext.to_tuple().1;
    }
    self.decrypt_helper(ciphertext, sender)
  }

  fn decrypt_helper(
      &mut self,
      ciphertext: OlmMessage,
      sender: &'a str
  ) -> String {
    if sender == self.get_idkey() {
      // FIXME handle dos attack where client poses as "self" - this
      // unwrap will panic
      return self.message_queue.pop().unwrap().to_string();
    }
    let session = self.find_active_session(ciphertext.clone(), sender);
    match session.decrypt(ciphertext) {
      Ok(plaintext) => return plaintext,
      Err(err) => panic!("Error decrypting ciphertext: {:?}", err),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{OlmWrapper, NUM_OTKEYS};
  use olm_rs::session::OlmMessage;
  use crate::server_comm::ServerComm;

  #[test]
  fn test_ow_new() {
    let olm_wrapper = OlmWrapper::new(None);
    assert_eq!(olm_wrapper.toggle_off, false);
  }

  #[test]
  fn test_ow_idkey() {
    let olm_wrapper = OlmWrapper::new(None);
    println!("idkey: {:?}", olm_wrapper.get_idkey());
  }

  #[test]
  fn test_ow_gen_otkeys() {
    let olm_wrapper = OlmWrapper::new(None);
    let otkeys = olm_wrapper.generate_otkeys(None);
    assert_eq!(NUM_OTKEYS, otkeys.curve25519().len());
    println!("otkeys: {:?}", otkeys.curve25519());
  }

  #[test]
  fn test_ow_gen_otkeys_custom_num() {
    let num = 7;
    let olm_wrapper = OlmWrapper::new(None);
    let otkeys = olm_wrapper.generate_otkeys(Some(num));
    assert_eq!(num, otkeys.curve25519().len());
    println!("otkeys: {:?}", otkeys.curve25519());
  }

  #[tokio::test]
  async fn test_ow_dummy_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(Some(true));
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext: &str = "hello";
    let ciphertext = olm_wrapper.encrypt(&server_comm, plaintext, &idkey)
        .await.to_tuple().1;
    assert_eq!(plaintext, ciphertext);
  }

  #[tokio::test]
  async fn test_ow_self_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext: &str = "hello";
    let empty: &str = "";
    let ciphertext = olm_wrapper.encrypt(&server_comm, plaintext, &idkey)
        .await.to_tuple().1;
    assert_eq!(empty, ciphertext);
    assert_eq!(plaintext, olm_wrapper.message_queue.pop().unwrap());
  }

  #[test]
  fn test_ow_dummy_decrypt() {
    let mut olm_wrapper = OlmWrapper::new(Some(true));
    let idkey = olm_wrapper.get_idkey();
    let plaintext: &str = "hello";
    let ciphertext = OlmMessage::from_type_and_ciphertext(1, plaintext.to_string()).unwrap();
    let decrypted = olm_wrapper.decrypt(ciphertext, &idkey);
    assert_eq!(plaintext, decrypted);
  }

  #[tokio::test]
  async fn test_ow_self_decrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::new(None, None, idkey.clone());
    let plaintext: &str = "hello";
    let empty: &str = "";
    let ciphertext = olm_wrapper.encrypt(&server_comm, plaintext, &idkey).await;
    let decrypted = olm_wrapper.decrypt(ciphertext.clone(), &idkey);
    assert_eq!(empty, ciphertext.to_tuple().1);
    assert_eq!(plaintext, decrypted);
  }

  #[tokio::test]
  async fn test_ow_self_outbound_session() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let server_comm = ServerComm::init(None, None, &olm_wrapper).await;
    let session = olm_wrapper.new_outbound_session(&server_comm, &idkey).await;
    println!("New session: {:?}", session);
    println!("New session ID: {:?}", session.session_id());
    assert!(!session.has_received_message());
  }

  #[tokio::test]
  async fn test_ow_encrypt_and_decrypt() {
    let mut ow1 = OlmWrapper::new(None);
    let idkey1 = ow1.get_idkey();
    println!("idkey1: {:?}", idkey1);
    let sc1 = ServerComm::init(None, None, &ow1).await;

    let mut ow2 = OlmWrapper::new(None);
    let idkey2 = ow2.get_idkey();
    println!("idkey2: {:?}", idkey2);
    let _ = ServerComm::init(None, None, &ow2).await;

    let plaintext = "testing testing one two three";

    let ciphertext = ow1.encrypt(&sc1, plaintext, &idkey2).await;
    let decrypted = ow2.decrypt(ciphertext.clone(), &idkey1);

    assert_eq!(plaintext, decrypted);
  }
}
