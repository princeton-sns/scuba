use std::collections::HashMap;
use std::collections::VecDeque;
use std::borrow::Borrow;

pub type DeviceId = String;
pub type Hash     = [u8; 32];
pub type Message  = String;

#[derive(Debug)]
pub enum Error {
  InvalidRecipientsOrder,
  TooFewRecipients,
  MissingSelfRecipient,
  OwnMessageInvalidReordered,
}

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

impl VectorEntry {
  fn new(local_seq: usize, digest: Hash) -> Self {
    VectorEntry { local_seq, digest }
  }
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

  fn set_consistency_loopback(&mut self, consistency_loopback: bool) -> bool {
    let prev = self.consistency_loopback;
    self.consistency_loopback = consistency_loopback;
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

  // TODO Vec<DeviceId> -> Vec<&DeviceId>

  pub fn prepare_message(
      &mut self,
      mut recipients: Vec<DeviceId>,
      message: Message,
  ) -> (CommonPayload, HashMap<DeviceId, RecipientPayload>) {
    let mut consistency_loopback = true;
    for recipient in recipients.iter() {
      if self.own_device == *recipient {
        consistency_loopback = false;
        break;
      }
    }

    if consistency_loopback {
      recipients.push(self.own_device.clone());
    }

    recipients.sort();
    self.register_message(recipients.clone(), message.clone());

    let mut recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    for recipient in recipients.iter() {
      recipient_payloads.insert(
          recipient.to_string(),
          self.construct_payload(recipient, consistency_loopback)
      );
    }

    (CommonPayload::new(recipients, message), recipient_payloads)
  }

  fn construct_payload(
      &self,
      recipient: &DeviceId,
      consistency_loopback: bool,
  ) -> RecipientPayload {
    let mut recipient_payload = RecipientPayload::new();

    if self.own_device == *recipient {
      recipient_payload.set_consistency_loopback(consistency_loopback);
    }

    match self.get_validation_payload(recipient) {
      Some(validation_payload) => {
        recipient_payload.set_validation_seq(validation_payload.0);
        recipient_payload.set_validation_digest(validation_payload.1);
      },
      None => log::debug!("no validation payload"),
    }

    recipient_payload
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

  fn parse_message(
      &mut self,
      sender: &DeviceId,
      common_payload: CommonPayload,
      recipient_payload: &RecipientPayload,
  ) -> Result<Option<(usize, Message)>, Error> {
    println!("common_payload: {:?}", common_payload);
    println!("recipient_payload: {:?}", recipient_payload);

    // Recipients list is sorted sending-side
    if !common_payload.recipients.iter().is_sorted() {
      return Err(Error::InvalidRecipientsOrder);
    }

    match self.insert_message(
        sender,
        common_payload.recipients,
        common_payload.message.clone(),
    ) {
      Ok(local_seq) => {
        //let num_trimmed_entries = self.validate_trim_vector(
        //    sender,
        //    recipient_payload.validation_seq,
        //    recipient_payload.validation_digest,
        //);
        //log::debug!("num_trimmed_entries: {:?}", num_trimmed_entries);

        if *sender == self.own_device && recipient_payload.consistency_loopback {
          // This message has been sent back to us just for the consistency
          // validation, discard it
          return Ok(None);
        }

        return Ok(Some((local_seq, common_payload.message)));
      },
      Err(err) => return Err(err),
    }
  }

  fn insert_message(
      &mut self,
      sender: &DeviceId,
      mut recipients: Vec<DeviceId>,
      message: Message,
  ) -> Result<usize, Error> {
    if recipients.len() < 1 {
      return Err(Error::TooFewRecipients);
    }
    let cloned_recipients = recipients.clone();
    let mut recipients_iter = cloned_recipients.iter();
    if !recipients_iter.clone().is_sorted() {
      return Err(Error::InvalidRecipientsOrder);
    }
    if !recipients_iter.any(|x| x == &self.own_device) {
      return Err(Error::MissingSelfRecipient);
    }

    // If this message was sent by us, ensure that it matches the
    // head of the pending_messages queue. If it does not, the
    // server must have reordered it or changed its contents or
    // recipients
    if *sender == self.own_device {
      // We must have at least two elements in the VecDeque: the
      // base (default) hash and the resulting (expected) message hash
      let mut pending_messages_iter = self.pending_messages.iter();
      let base_hash = pending_messages_iter.next().unwrap();
      let expected_hash = pending_messages_iter
          .next()
          .ok_or(Error::OwnMessageInvalidReordered)?;

      let calculated_hash = hash_message(
        Some(base_hash),
        &mut recipients,
        message.clone(),
      );

      if *expected_hash != calculated_hash {
        return Err(Error::OwnMessageInvalidReordered);
      }

      self.pending_messages.pop_front();
    }

    // Assign this message a sequence number from the device-global
    // sequence space
    let local_seq = self.local_seq;
    self.local_seq += 1;

    // Hash the message in the context of all its recipients'
    // pairwise hash vectors
    for recipient in recipients_iter.filter(
        |r| **r != self.own_device) {
      let vector = self.vectors.entry(recipient.to_string())
          .or_insert_with(|| DeviceState::default());

      let message_hash_entry = hash_message(
        vector.vector.back().map(|entry| &entry.digest),
        &mut recipients,
        message.clone(),
      );

      vector.vector.push_back(VectorEntry::new(local_seq, message_hash_entry));
    }

    Ok(local_seq)
  }

  fn get_validation_payload(&self, recipient: &DeviceId) -> Option<(usize, Hash)> {
    let recipient_vector = self.vectors.get(recipient)?;
    Some((
      recipient_vector.offset + recipient_vector.vector.len() - 1,
      recipient_vector.vector.back()?.digest.clone(),
    ))
  }

  //fn validate_vector() {}
  //fn validate_trim_vector() {}
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

    let message_1 = String::from("first message");
    let hashed_message_1 = hash_message(
      Some(hash_vectors.pending_messages.back().unwrap()),
      &mut recipients,
      message_1.clone()
    );

    hash_vectors.register_message(recipients.clone(), message_1);

    let pending_messages_1 = VecDeque::from(vec![Hash::default(), hashed_message_1]);
    assert_eq!(hash_vectors.pending_messages, pending_messages_1);

    let message_2 = String::from("second message");
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
  fn test_prepare_first_message_to_self_only() {
    let idkey = String::from("0");
    let mut hash_vectors = HashVectors::new(idkey.clone());
    let recipients = vec![idkey.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());
    assert_eq!(common_payload, CommonPayload::new(recipients, message));

    let mut expected_recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    expected_recipient_payloads.insert(idkey, RecipientPayload {
      consistency_loopback: false,
      validation_seq: None,
      validation_digest: None,
    });
    assert_eq!(recipient_payloads, expected_recipient_payloads);
  }

  #[test]
  fn test_prepare_first_message_to_self_and_others() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors = HashVectors::new(idkey_0.clone());
    let recipients = vec![idkey_0.clone(), idkey_1.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());
    assert_eq!(common_payload, CommonPayload::new(recipients, message));

    let mut expected_recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    expected_recipient_payloads.insert(idkey_0, RecipientPayload {
      consistency_loopback: false,
      validation_seq: None,
      validation_digest: None,
    });
    expected_recipient_payloads.insert(idkey_1, RecipientPayload {
      consistency_loopback: false,
      validation_seq: None,
      validation_digest: None,
    });
    assert_eq!(recipient_payloads, expected_recipient_payloads);
  }

  #[test]
  fn test_prepare_first_message_to_others_only() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors = HashVectors::new(idkey_0.clone());
    let recipients = vec![idkey_1.clone()];
    let loopback_recipients = vec![idkey_0.clone(), idkey_1.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());
    assert_eq!(common_payload, CommonPayload::new(loopback_recipients, message));

    let mut expected_recipient_payloads = HashMap::<DeviceId, RecipientPayload>::new();
    expected_recipient_payloads.insert(idkey_0, RecipientPayload {
      consistency_loopback: true,
      validation_seq: None,
      validation_digest: None,
    });
    expected_recipient_payloads.insert(idkey_1, RecipientPayload {
      consistency_loopback: false,
      validation_seq: None,
      validation_digest: None,
    });
    assert_eq!(recipient_payloads, expected_recipient_payloads);
  }

  //#[test]
  //fn test_prepare_message_to_self_only() {}

  //#[test]
  //fn test_prepare_message_to_self_and_others() {}

  //#[test]
  //fn test_prepare_message_to_others_only() {}

  #[test]
  fn test_insert_first_message_to_self_only() {
    let idkey = String::from("0");
    let mut hash_vectors = HashVectors::new(idkey.clone());
    let recipients = vec![idkey.clone()];
    let message = String::from("test message");

    let (_, _) = hash_vectors.prepare_message(recipients.clone(), message.clone());

    match hash_vectors.insert_message(&idkey, recipients, message) {
      Ok(seq) => assert_eq!(seq, 0),
      Err(err) => panic!("Error inserting message: {:?}", err),
    }
  }

  #[test]
  fn test_insert_first_message_to_self_and_others() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors = HashVectors::new(idkey_0.clone());
    let recipients = vec![idkey_0.clone(), idkey_1.clone()];
    let message = String::from("test message");

    let (_, _) = hash_vectors.prepare_message(recipients.clone(), message.clone());

    match hash_vectors.insert_message(&idkey_0, recipients, message) {
      Ok(seq) => assert_eq!(seq, 0),
      Err(err) => panic!("Error inserting message: {:?}", err),
    }
  }

  #[test]
  fn test_insert_first_message_to_others_only() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors_0 = HashVectors::new(idkey_0.clone());
    let mut hash_vectors_1 = HashVectors::new(idkey_1.clone());
    let recipients = vec![idkey_1.clone()];
    let message = String::from("test message");

    let (_, _) = hash_vectors_0.prepare_message(recipients.clone(), message.clone());

    match hash_vectors_1.insert_message(&idkey_0, recipients, message) {
      Ok(seq) => assert_eq!(seq, 0),
      Err(err) => panic!("Error inserting message: {:?}", err),
    }
  }

  //#[test]
  //fn test_insert_message_to_self_only() {}

  //#[test]
  //fn test_insert_message_to_self_and_others() {}

  //#[test]
  //fn test_insert_message_to_others_only() {}

  //#[test]
  //fn test_get_validation_payload() {}

  #[test]
  fn test_parse_first_message_to_self_only() {
    let idkey = String::from("0");
    let mut hash_vectors = HashVectors::new(idkey.clone());
    let recipients = vec![idkey.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());

    assert_eq!(hash_vectors.parse_message(&idkey, common_payload, recipient_payloads.get(&idkey).unwrap()).unwrap(), Some((0, message)));
  }

  #[test]
  fn test_parse_first_message_to_self_and_others() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors = HashVectors::new(idkey_0.clone());
    let recipients = vec![idkey_0.clone(), idkey_1.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors.prepare_message(recipients.clone(), message.clone());

    assert_eq!(hash_vectors.parse_message(&idkey_0, common_payload, recipient_payloads.get(&idkey_1).unwrap()).unwrap(), Some((0, message)));
  }

  #[test]
  fn test_parse_first_message_to_others_only() {
    let idkey_0 = String::from("0");
    let idkey_1 = String::from("1");
    let mut hash_vectors_0 = HashVectors::new(idkey_0.clone());
    let mut hash_vectors_1 = HashVectors::new(idkey_1.clone());
    let recipients = vec![idkey_1.clone()];
    let message = String::from("test message");

    let (common_payload, recipient_payloads) = hash_vectors_0.prepare_message(recipients.clone(), message.clone());

    assert_eq!(hash_vectors_1.parse_message(&idkey_0, common_payload, recipient_payloads.get(&idkey_1).unwrap()).unwrap(), Some((0, message)));
  }
}
