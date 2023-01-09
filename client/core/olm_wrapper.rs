use olm_rs::account::{OlmAccount, IdentityKeys, OneTimeKeys};

const NUM_OTKEYS : usize = 10;

pub struct OlmWrapper {
  toggle_off   : bool,
  idkeys       : IdentityKeys,
  account      : OlmAccount,
}

impl OlmWrapper {
  pub fn new(toggle_arg: Option<bool>) -> Self {
    let account = OlmAccount::new();
    Self {
      toggle_off: toggle_arg.unwrap_or(false),
      idkeys: account.parsed_identity_keys(),
      account,
    }
  }

  pub fn generate_otkeys(&self, num: Option<usize>) -> OneTimeKeys {
    self.account.generate_one_time_keys(num.unwrap_or(NUM_OTKEYS));
    let otkeys = self.account.parsed_one_time_keys();
    self.account.mark_keys_as_published();
    otkeys
  }

  pub fn get_curve25519_idkey(&self) -> String {
    self.idkeys.curve25519().to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::{OlmWrapper, NUM_OTKEYS};

  #[test]
  fn test_ow_init() {
    let olm_wrapper = OlmWrapper::new(None);
    assert_eq!(olm_wrapper.toggle_off, false);
  }

  #[test]
  fn test_ow_idkey() {
    let olm_wrapper = OlmWrapper::new(None);
    println!("idkey: {:?}", olm_wrapper.get_curve25519_idkey());
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
}
