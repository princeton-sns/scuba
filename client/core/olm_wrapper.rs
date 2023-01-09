use olm_rs::account::{OlmAccount, IdentityKeys, OneTimeKeys};
use olm_rs::session::OlmMessage;

const NUM_OTKEYS : usize = 10;

pub struct OlmWrapper<'a> {
  toggle_off   : bool,
  idkeys       : IdentityKeys,
  account      : OlmAccount,
  message_queue: Vec<&'a str>,
}

impl<'a> OlmWrapper<'a> {
  pub fn new(toggle_arg: Option<bool>) -> Self {
    let account = OlmAccount::new();
    Self {
      toggle_off: toggle_arg.unwrap_or(false),
      idkeys: account.parsed_identity_keys(),
      account,
      message_queue: Vec::new(),
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

  pub fn encrypt(
      &mut self,
      plaintext: &'a str,
      dst_idkey: &'a str
  ) -> OlmMessage {
    if self.toggle_off {
      return OlmMessage::from_type_and_ciphertext(1, plaintext.to_string()).unwrap();
    }
    self.encrypt_helper(plaintext, dst_idkey)
  }

  fn encrypt_helper(
      &mut self,
      plaintext: &'a str,
      dst_idkey: &'a str
  ) -> OlmMessage {
    if dst_idkey == self.get_idkey() {
      self.message_queue.push(plaintext);
      return OlmMessage::from_type_and_ciphertext(1, "".to_string()).unwrap();
    }
    // TODO
    OlmMessage::from_type_and_ciphertext(1, plaintext.to_string()).unwrap()
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
    // TODO
    ciphertext.to_tuple().1
  }
}

#[cfg(test)]
mod tests {
  use super::{OlmWrapper, NUM_OTKEYS};
  use olm_rs::session::OlmMessage;

  #[test]
  fn test_ow_init() {
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

  #[test]
  fn test_ow_dummy_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(Some(true));
    let idkey = olm_wrapper.get_idkey();
    let plaintext: &str = "hello";
    let ciphertext = olm_wrapper.encrypt(plaintext, &idkey).to_tuple().1;
    assert_eq!(plaintext, ciphertext);
  }

  #[test]
  fn test_ow_self_encrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let plaintext: &str = "hello";
    let empty: &str = "";
    let ciphertext = olm_wrapper.encrypt(plaintext, &idkey).to_tuple().1;
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

  #[test]
  fn test_ow_self_decrypt() {
    let mut olm_wrapper = OlmWrapper::new(None);
    let idkey = olm_wrapper.get_idkey();
    let plaintext: &str = "hello";
    let empty: &str = "";
    let ciphertext = olm_wrapper.encrypt(plaintext, &idkey);
    let decrypted = olm_wrapper.decrypt(ciphertext.clone(), &idkey);
    assert_eq!(empty, ciphertext.to_tuple().1);
    assert_eq!(plaintext, decrypted);
  }
}
