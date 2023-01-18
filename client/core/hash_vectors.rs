use std::collections::HashMap;
use std::collections::VecDeque;
use std::borrow::Borrow;

pub type DeviceId = String;
pub type Hash     = [u8; 32];
pub type Message  = String;

fn hash_message<BD: Borrow<DeviceId>>(
  prev_digest: Option<&Hash>,
  // TODO Sorted/Ord constraint?
  recipients: &mut Vec<BD>,
  message: Message,
) -> Hash {
  use sha2::Digest;

  let mut hasher = sha2::Sha256::new();

  if let Some(digest) = prev_digest {
    hasher.update(b"prev");
    hasher.update(digest);
  } else {
    hasher.update(b"no_prev");
  }

  for (i, r) in recipients.iter().enumerate() {
    hasher.update(&u64::to_be_bytes(i as u64));
    hasher.update(r.borrow().as_bytes());
  }

  hasher.update(b"message");
  hasher.update(message);

  let mut digest: Hash = [0; 32];
  hasher.finalize_into_reset((&mut digest).into());
  digest
}

#[derive(Debug, PartialEq)]
struct VectorEntry {
  local_seq: usize,
  digest: Hash,
}

#[derive(Debug, PartialEq)]
struct DeviceState {
  offset: usize,
  validated_local_seq: usize,
  vector: VecDeque<VectorEntry>,
}

impl Default for DeviceState {
  fn default() -> DeviceState {
    DeviceState {
      offset: 0,
      // We can initialize this to 0 as this points to the first
      // *non-validated* local sequence number
      validated_local_seq: 0,
      vector: VecDeque::new(),
    }
  }
}

#[derive(Debug, PartialEq)]
pub struct CommonPayload {
  recipients: Vec<DeviceId>,
  message: Message,
}

impl CommonPayload {
  fn new(recipients: Vec<DeviceId>, message: Message) -> Self {
    CommonPayload { recipients, message }
  }
}

#[derive(Debug, PartialEq)]
pub struct RecipientPayload {
  consistency_loopback: bool,
  validation_seq: Option<usize>,
  validation_digest: Option<Hash>,
}

impl RecipientPayload {
  fn new() -> RecipientPayload {
    RecipientPayload {
      consistency_loopback: false,
      validation_seq: None,
      validation_digest: None,
    }
  }

  fn set_consistency_loopback(&mut self) -> bool {
    let prev = self.consistency_loopback;
    self.consistency_loopback = true;
    prev
  }

  fn set_validation_seq(&mut self, validation_seq: usize) -> Option<usize> {
    let prev_seq = self.validation_seq;
    self.validation_seq = Some(validation_seq);
    prev_seq
  }

  fn set_validation_digest(&mut self, validation_digest: Hash) -> Option<Hash> {
    let prev_digest = self.validation_digest;
    self.validation_digest = Some(validation_digest);
    prev_digest
  }
}

#[derive(Debug)]
pub struct HashVectors {
  own_device: DeviceId,
  pending_messages: VecDeque<Hash>,
  vectors: HashMap<DeviceId, DeviceState>,
  local_seq: usize,
}

// TODO implement quorum for message
impl HashVectors {
  pub fn new(own_device: DeviceId) -> Self {
    let mut pending_messages = VecDeque::new();
    pending_messages.push_back(Hash::default());

    HashVectors {
      own_device,
      pending_messages,
      vectors: HashMap::new(),
      local_seq: 0,
    }
  }

  pub fn prepare_message(
      &mut self,
      mut recipients: Vec<DeviceId>,
      message: Message,
  ) -> (CommonPayload, HashMap<DeviceId, RecipientPayload>) {
    let mut consistency_loopback = true;
    for recipient in recipients.iter() {
      if self.own_device.eq(recipient) {
        consistency_loopback = false;
        break;
      }
    }

    if consistency_loopback {
      recipients.push(self.own_device.clone());
    }

    recipients.clone().as_mut_slice().sort();
    self.register_message(recipients.clone(), message.clone());

    let mut recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    for recipient in recipients.iter() {
      let mut recipient_payload = RecipientPayload::new();

      if self.own_device.eq(recipient) {
        recipient_payload.set_consistency_loopback();
      }

      match self.get_validation_payload(recipient) {
        Some(validation_payload) => {
          recipient_payload.set_validation_seq(validation_payload.0);
          recipient_payload.set_validation_digest(validation_payload.1);
        },
        None => log::debug!("no validation payload"),
      }

      recipient_payloads.insert(recipient.to_string(), recipient_payload);
    }

    (CommonPayload::new(recipients, message), recipient_payloads)
  }

  fn register_message(
      &mut self,
      mut recipients: Vec<DeviceId>,
      message: Message,
  ) {
    let message_hash_entry = hash_message(
      Some(self.pending_messages.back().unwrap()),
      &mut recipients,
      message
    );
    
    self.pending_messages.push_back(message_hash_entry);
  }

  //fn insert_message() {}
  //fn validate_vector() {}
  //fn validate_trim_vector() {}

  fn get_validation_payload(&self, recipient: &DeviceId) -> Option<(usize, Hash)> {
    let recipient_vector = self.vectors.get(recipient)?;
    Some((
      recipient_vector.offset + recipient_vector.vector.len() - 1,
      recipient_vector.vector.back()?.digest.clone(),
    ))
  }
}

#[cfg(test)]
mod test {
  use super::{Hash, HashVectors, hash_message, DeviceId, CommonPayload, RecipientPayload};
  use std::collections::HashMap;
  use std::collections::VecDeque;

  #[test]
  fn test_new() {
    let idkey = String::from("0");
    let hash_vectors = HashVectors::new(idkey.clone());

    assert_eq!(hash_vectors.own_device, idkey);
    assert_eq!(hash_vectors.pending_messages, VecDeque::from(vec![Hash::default()]));
    assert_eq!(hash_vectors.vectors, HashMap::new());
    assert_eq!(hash_vectors.local_seq, 0);
  }

  #[test]
  fn test_register_message() {
    let idkey = String::from("0");
    let mut hash_vectors = HashVectors::new(idkey.clone());
    let mut recipients = vec![idkey];

    let message_1 = String::from("first registered message");
    let hashed_message_1 = hash_message(
      Some(hash_vectors.pending_messages.back().unwrap()),
      &mut recipients,
      message_1.clone()
    );

    hash_vectors.register_message(recipients.clone(), message_1);

    let pending_messages_1 = VecDeque::from(vec![Hash::default(), hashed_message_1]);
    assert_eq!(hash_vectors.pending_messages, pending_messages_1);

    let message_2 = String::from("second registered message");
    let hashed_message_2 = hash_message(
      Some(hash_vectors.pending_messages.back().unwrap()),
      &mut recipients,
      message_2.clone()
    );

    hash_vectors.register_message(recipients, message_2);

    let pending_messages_2 = VecDeque::from(vec![Hash::default(), hashed_message_1, hashed_message_2]);
    assert_eq!(hash_vectors.pending_messages, pending_messages_2);
  }

  #[test]
  fn test_get_validation_payload_empty() {
    let idkey = String::from("0");
    let hash_vectors = HashVectors::new(idkey.clone());
    let validation_payload = hash_vectors.get_validation_payload(&idkey);
    assert_eq!(validation_payload, None);
  }

  #[test]
  fn test_prepare_first_message() {
    let idkey = String::from("0");
    let mut hash_vectors = HashVectors::new(idkey.clone());
    let recipients = vec![idkey.clone()];
    let message = String::from("prepared message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());
    assert_eq!(common_payload, CommonPayload::new(recipients, message));

    let mut expected_recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    expected_recipient_payloads.insert(idkey, RecipientPayload {
      consistency_loopback: true,
      validation_seq: None,
      validation_digest: None,
    });
    assert_eq!(recipient_payloads, expected_recipient_payloads);
  }
}
