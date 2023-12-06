use rkyv::{bytecheck, Archive, CheckBytes};
use serde::{Deserialize, Serialize};
use std::borrow::{Borrow, Cow};
use std::collections::BTreeMap;

// -------------------------------------------------------------------------
// HTTP API Types

#[derive(Archive, CheckBytes, Debug, Serialize, Deserialize, Clone)]
#[archive(check_bytes)]
#[serde(transparent)]
pub struct EncryptedCommonPayload(pub Vec<u8>);

#[derive(Archive, CheckBytes, Debug, Serialize, Deserialize, Clone)]
#[archive(check_bytes)]
pub struct EncryptedPerRecipientPayload {
    pub c_type: usize,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptedOutboxMessage {
    pub enc_common: EncryptedCommonPayload,
    pub enc_recipients: BTreeMap<String, EncryptedPerRecipientPayload>,
}

#[derive(Archive, CheckBytes, Debug, Serialize, Deserialize, Clone)]
#[archive(check_bytes)]
pub struct EncryptedInboxMessage {
    pub sender: String,
    // Must be sorted in key order
    pub recipients: Vec<String>,
    pub enc_common: EncryptedCommonPayload,
    pub enc_recipient: EncryptedPerRecipientPayload,
    pub seq_id: u128,
}

// -------------------------------------------------------------------------
// EventSource Messages

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessageBatch<'a> {
    pub start_epoch_id: u64,
    pub end_epoch_id: u64,
    pub messages: Cow<'a, Vec<(u64, Cow<'a, Vec<EncryptedInboxMessage>>)>>,
    pub attestation: Vec<u8>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OtkeyRequest {
    pub device_id: String,
    pub needs: usize,
}

// -------------------------------------------------------------------------
// Attestation payload layout

#[derive(Clone)]
pub struct AttestationMessage<'a> {
    pub seq_id: u128,
    pub per_recipient: BTreeMap<&'a str, Option<(usize, &'a [u8])>>,
    pub common: &'a [u8],
}

impl<'a> AttestationMessage<'a> {
    pub fn from_inbox(
        ibmsg: &'a EncryptedInboxMessage,
        recipient: &'a str,
    ) -> Self {
        let mut per_recipient = BTreeMap::new();
        for rcpt in ibmsg.recipients.iter() {
            per_recipient.insert(
                rcpt.as_str(),
                if rcpt == recipient {
                    Some((
                        ibmsg.enc_recipient.c_type,
                        ibmsg.enc_recipient.ciphertext.as_slice(),
                    ))
                } else {
                    None
                },
            );
        }

        AttestationMessage {
            seq_id: ibmsg.seq_id,
            per_recipient,
            common: &ibmsg.enc_common.0,
        }
    }
}

#[derive(Clone, Debug)]
// Format:
// - u128: first_epoch
// - u128: next_epoch
// - [u8; 32]: messages_digest
pub struct AttestationData([u8; 64]);

impl AttestationData {
    fn message_hash_table<'a, T: Borrow<AttestationMessage<'a>>>(
        messages: impl Iterator<Item = T>,
    ) -> impl Iterator<Item = (u128, Vec<([u8; 32], [u8; 32])>, [u8; 32])> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();

        messages.map(move |message| {
            let recipients_digests = message
                .borrow()
                .per_recipient
                .iter()
                .map(|(rcpt, ciphertext)| {
                    // First, recipient:
                    hasher.update(&u128::to_le_bytes(message.borrow().seq_id));
                    hasher.update(rcpt.as_bytes());
                    let mut recipient_digest = [0; 32];
                    hasher.finalize_into_reset((&mut recipient_digest).into());

                    // Second, if provided, ciphertext:
                    let mut ciphertext_digest = [0; 32];
                    if let Some((c_type, c)) = ciphertext {
                        hasher.update(&usize::to_le_bytes(*c_type));
                        hasher.update(c);
                        hasher.finalize_into_reset(
                            (&mut ciphertext_digest).into(),
                        );
                    }

                    (recipient_digest, ciphertext_digest)
                })
                .collect();

            let mut payload_digest = [0; 32];
            hasher.update(&message.borrow().common);
            hasher.finalize_into_reset((&mut payload_digest).into());

            (message.borrow().seq_id, recipients_digests, payload_digest)
        })
    }

    pub fn from_inbox_msgs<'a, T: Borrow<EncryptedInboxMessage>>(
        client_id: &str,
        first_seq: u128,
        next_seq: u128,
        messages: impl Iterator<Item = T>,
    ) -> AttestationData {
        let owned_messages: Vec<_> =
            messages.map(|msg| msg.borrow().clone()).collect();

        Self::from_inbox_msgs_int(
            client_id,
            first_seq,
            next_seq,
            Self::message_hash_table(
                owned_messages.iter().map(|ibmsg| {
                    AttestationMessage::from_inbox(ibmsg, client_id)
                }),
            ),
        )
    }

    pub fn from_att_msgs(
        client_id: &str,
        first_seq: u128,
        next_seq: u128,
        messages: Vec<AttestationMessage>,
    ) -> AttestationData {
        Self::from_inbox_msgs_int(
            client_id,
            first_seq,
            next_seq,
            Self::message_hash_table(messages.into_iter()),
        )
    }

    fn from_inbox_msgs_int<
        T: Borrow<(u128, Vec<([u8; 32], [u8; 32])>, [u8; 32])>,
    >(
        client_id: &str,
        first_seq: u128,
        next_seq: u128,
        message_hash_table: impl Iterator<Item = T>,
    ) -> AttestationData {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();

        hasher.update(client_id.as_bytes());

        message_hash_table.for_each(|b| {
            let (epoch_id, per_recipient, payload_digest) = b.borrow();
            hasher.update(&u128::to_le_bytes(*epoch_id));
            for (rcpt_id_digest, rcpt_ciphertext_digest) in per_recipient {
                hasher.update(rcpt_id_digest);
                hasher.update(rcpt_ciphertext_digest);
            }
            hasher.update(&payload_digest);
        });

        let mut attestation_messages_hash = [0; 32];
        hasher.finalize_into((&mut attestation_messages_hash).into());

        let mut attestation_data = [0; 64];
        attestation_data[0..16].copy_from_slice(&u128::to_le_bytes(first_seq));
        attestation_data[16..32].copy_from_slice(&u128::to_le_bytes(next_seq));
        attestation_data[32..].copy_from_slice(&attestation_messages_hash);
        AttestationData(attestation_data)
    }

    pub fn attest(
        &self,
        attestation_key: &ed25519_dalek::Keypair,
    ) -> Attestation {
        use ed25519_dalek::Signer;
        let signature = attestation_key.sign(&self.0);

        let mut attestation = [0; 64 + 64];
        attestation[0..64].copy_from_slice(&self.0);
        attestation[64..].copy_from_slice(&signature.to_bytes());
        Attestation(attestation)
    }

    // TODO: use produce_claim_from_att_msgs internally
    pub fn produce_claim_from_ibmsgs(
        &self,
        client_id: String,
        first_seq: u128,
        next_seq: u128,
        messages: Vec<EncryptedInboxMessage>,
        claim_message_idx: Option<usize>,
        attestation: Attestation,
    ) -> AttestationClaim {
        if let Some(idx) = claim_message_idx {
            assert!(idx < messages.len());
        }

        // Generate the hash-table to be passed alongside the attestation:
        let message_hash_table: Vec<_> =
            Self::message_hash_table(messages.iter().map(|ibmsg| {
                AttestationMessage::from_inbox(ibmsg, client_id.as_str())
            }))
            .collect();

        // Sanity check that the claim we're producing is supported by the
        // passed attestation:
        let attestation_data = Self::from_inbox_msgs_int(
            &client_id,
            first_seq,
            next_seq,
            message_hash_table.iter(),
        );
        println!("Att data: {:x?}", attestation_data);
        println!("Attestation: {:x?}", attestation);
        assert!(attestation_data.0[..] == attestation.0[..64]);

        // Extract the server signature from the attestation:
        let mut signature = [0; 64];
        signature.copy_from_slice(&attestation.0[64..]);

        AttestationClaim {
            client_id,
            first_seq,
            next_seq,
            message_hash_table,
            message_idx: claim_message_idx,
            attestation: signature,
        }
    }

    pub fn produce_claim_from_att_msgs(
        &self,
        client_id: String,
        first_seq: u128,
        next_seq: u128,
        messages: Vec<AttestationMessage>,
        claim_message_idx: Option<usize>,
        attestation: Attestation,
    ) -> AttestationClaim {
        if let Some(idx) = claim_message_idx {
            assert!(idx < messages.len());
        }

        // Generate the hash-table to be passed alongside the attestation:
        let message_hash_table: Vec<_> =
            Self::message_hash_table(messages.into_iter()).collect();

        // Sanity check that the claim we're producing is supported by the
        // passed attestation:
        let attestation_data = Self::from_inbox_msgs_int(
            &client_id,
            first_seq,
            next_seq,
            message_hash_table.iter(),
        );
        println!("Att data: {:x?}", attestation_data);
        println!("Attestation: {:x?}", attestation);
        assert!(attestation_data.0[..] == attestation.0[..64]);

        // Extract the server signature from the attestation:
        let mut signature = [0; 64];
        signature.copy_from_slice(&attestation.0[64..]);

        AttestationClaim {
            client_id,
            first_seq,
            next_seq,
            message_hash_table,
            message_idx: claim_message_idx,
            attestation: signature,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AttestationClaim {
    pub client_id: String,
    pub first_seq: u128,
    pub next_seq: u128,
    pub message_hash_table: Vec<(u128, Vec<([u8; 32], [u8; 32])>, [u8; 32])>,
    // Index into the message_hash_table pointing to the message at fault
    pub message_idx: Option<usize>,
    pub attestation: [u8; 64],
}

impl AttestationClaim {
    pub fn attestation(&self) -> Attestation {
        let attestation_data = AttestationData::from_inbox_msgs_int(
            &self.client_id,
            self.first_seq,
            self.next_seq,
            self.message_hash_table.iter(),
        );
        let mut attestation_buf = [0; 64 + 64];
        attestation_buf[..64].copy_from_slice(&attestation_data.0);
        attestation_buf[64..].copy_from_slice(&self.attestation);
        Attestation(attestation_buf)
    }

    pub fn supports(
        &self,
        other: &AttestationClaim,
        public_key: &ed25519_dalek::PublicKey,
    ) -> bool {
        // Two claims support each other when
        // - one contains a message at a sequence number that the other does
        //   not, despite the other client being in the first's recipient list
        //   (positive + negative claim), or
        // - both contain a message with an identical sequence number but
        //   differing receipients or common payload (positive + positive), or
        // - both contain a message with an identical sequence number, and both
        //   contain the per-recipient ciphertext for a given recipient (the
        //   recipient digest matches and the following ciphertext is different
        //   but non-null for both), which is a special case of (positive +
        //   positive) used with sender-issued attestations.
        //
        // For simpler handling, "sort" the two claims by whichever contains
        // a potentially conflicting message reference. It's invalid for
        // neither of them to point to a message:
        let (claim_p, claim_pn) = if self.message_idx.is_some() {
            (self, other)
        } else {
            (other, self)
        };

        let p_idx = if let Some(p) = &claim_p.message_idx {
            p
        } else {
            // At least one positive claim required!
            return false;
        };

        // Need to handle positive + positive & positive + negative
        // differently:
        if let Some(pn_idx) = &claim_pn.message_idx {
            // Positive + positive claim! Compare sequence numbers. If they
            // match, recipients and payload must be identical.
            let (p_seqid, p_recipients, p_payload) =
                &claim_p.message_hash_table[*p_idx];
            let (pn_seqid, pn_recipients, pn_payload) =
                &claim_pn.message_hash_table[*pn_idx];

            let p_recipient_id_digests: Vec<[u8; 32]> = p_recipients
                .iter()
                .map(|(id_digest, _ciphertext_digest)| *id_digest)
                .collect();
            let pn_recipient_id_digests: Vec<[u8; 32]> = p_recipients
                .iter()
                .map(|(id_digest, _ciphertext_digest)| *id_digest)
                .collect();

            if p_seqid != pn_seqid {
                // The sequence ids don't match, this claim is worthless.
                return false;
            } else if p_recipient_id_digests == pn_recipient_id_digests
                && p_payload == pn_payload
            {
                // The sequence id, list of recipients and common payloads
                // match. These claims don't _yet_ support each other. However,
                // if there exists a message for which both of the attestations
                // provide a non-zero per-recipient ciphertext, and those don't
                // match, then the server must've manipulated the per-recipient
                // ciphertext:
                let per_recipient_ciphertext_mismatch = p_recipients
                    .iter()
                    .zip(pn_recipients.iter())
                    .find(
                        |(
                            (_p_rcpt_id, p_rcpt_ciphertext),
                            (_pn_recipient_id, pn_rcpt_ciphertext),
                        )| {
                            *p_rcpt_ciphertext != [0_u8; 32]
                                && *pn_rcpt_ciphertext != [0_u8; 32]
                                && p_rcpt_ciphertext != pn_rcpt_ciphertext
                        },
                    )
                    .is_some();

                if !per_recipient_ciphertext_mismatch {
                    // The per-recipient ciphertexts match, or either one of the
                    // attestations doesn't contain it. Attestations don't
                    // support each other!
                    return false;
                }
            }

            // Claims support each other, but individual claims not
            // verified yet!
        } else {
            // Positive + negative claim! The positive claim can only be
            // supported by the negative claim if the offending message's
            // sequence number is contained within the negative claim's
            // sequence space, so verify that.
            let (p_seqid, p_recipients, _p_payload) =
                &claim_p.message_hash_table[*p_idx];

            if *p_seqid < claim_pn.first_seq || *p_seqid >= claim_pn.next_seq {
                return false;
            }

            // claim_p's message lies in claim_pn's sequence space, now we need
            // to verify that claim_p should indeed have been received by
            // claim_pn. We can do this by ensuring that claim_pn's client_id is
            // in the recipients of claim_p's message. We do this by generating
            // the salted hash of the client_id first:
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&u128::to_le_bytes(*p_seqid));
            hasher.update(claim_pn.client_id.as_bytes());
            let mut claim_pn_client_id_digest = [0; 32];
            hasher.finalize_into_reset((&mut claim_pn_client_id_digest).into());

            let recipient_found = p_recipients
                .iter()
                .find(|(p_rcpt_id_digest, _p_rcpt_ciphertext_digest)| {
                    *p_rcpt_id_digest == claim_pn_client_id_digest
                })
                .is_some();

            if !recipient_found {
                // claim_pn's client_id isn't in the recipients list, so the
                // negative claim can't be used here.
                return false;
            }

            // Now, loop over claim_pn's message to ensure that it indeed
            // does not contain a message with the offending sequence
            // number:
            if claim_pn
                .message_hash_table
                .iter()
                .find(|(seqid, _, _)| *seqid == *p_seqid)
                .is_some()
            {
                return false;
            }

            // Claims support each other, but individual claims not
            // verified yet!
        }

        // Verify each of the invidual claim's cryptographic validity. If
        // they are both valid, we hold evidence that the server has behaved
        // incorrectly.
        //
        // It is fine to use verify_trusted here, as we're trusting the
        // attestation payload hash which is constructed based on the claim
        // data by `attestation()`.
        claim_p.attestation().verify_trusted(public_key)
            && claim_pn.attestation().verify_trusted(public_key)
    }
}

#[derive(Clone, Debug)]
pub struct Attestation([u8; 64 + 64]);

impl Attestation {
    pub fn into_arr(self) -> [u8; 64 + 64] {
        self.0
    }

    pub fn from_arr(arr: [u8; 64 + 64]) -> Self {
        Attestation(arr)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() != 64 + 64 {
            Err(())
        } else {
            let mut buf = [0; 64 + 64];
            buf.copy_from_slice(bytes);
            Ok(Attestation(buf))
        }
    }

    pub fn decode_base64(s: &str) -> Result<Self, ()> {
        use base64::{engine::general_purpose, Engine as _};

        if let Ok(vec) = general_purpose::STANDARD_NO_PAD.decode(s) {
            if vec.len() != 64 + 64 {
                Err(())
            } else {
                let mut buf = [0; 64 + 64];
                buf.copy_from_slice(&vec);
                Ok(Attestation(buf))
            }
        } else {
            Err(())
        }
    }

    pub fn encode_base64(&self) -> String {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::STANDARD_NO_PAD.encode(&self.0)
    }

    pub fn first_epoch(&self) -> u64 {
        u64::from_le_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5],
            self.0[6], self.0[7],
        ])
    }

    pub fn next_epoch(&self) -> u64 {
        u64::from_le_bytes([
            self.0[8], self.0[9], self.0[10], self.0[11], self.0[12],
            self.0[13], self.0[14], self.0[15],
        ])
    }

    pub fn verify(
        &self,
        data: &AttestationData,
        public_key: &ed25519_dalek::PublicKey,
    ) -> bool {
        data.0 == self.0[0..64] && self.verify_trusted(public_key)
    }

    pub fn verify_trusted(
        &self,
        public_key: &ed25519_dalek::PublicKey,
    ) -> bool {
        use ed25519_dalek::Verifier;
        let signature =
            ed25519_dalek::Signature::from_bytes(&self.0[64..]).unwrap();
        public_key.verify(&self.0[..64], &signature).is_ok()
    }
}

#[cfg(test)]
mod test {
    use super::{
        AttestationData, AttestationMessage, EncryptedCommonPayload,
        EncryptedInboxMessage, EncryptedPerRecipientPayload,
    };
    use std::collections::BTreeMap;

    fn get_keypair() -> ed25519_dalek::Keypair {
        use core::mem;
        use ed25519_dalek::Keypair;
        use rand::rngs::OsRng;
        use std::fs::{File, OpenOptions};
        use std::io::{Read, Write};
        use std::path::Path;

        // Read server attestation private key or generate a new one
        let keypair_path = Path::new("./attestation-key");

        if let Ok(mut keypair_file) = File::open(keypair_path) {
            let mut keypair_bytes = [0; 64];
            assert!(keypair_file.read(&mut keypair_bytes).unwrap() == 64);
            Keypair::from_bytes(&keypair_bytes).unwrap()
        } else {
            let mut csprng = OsRng {};
            let keypair = Keypair::generate(&mut csprng);
            let mut keypair_file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(keypair_path)
                .unwrap();
            keypair_file.write(&keypair.to_bytes()).unwrap();
            keypair_file.flush().unwrap();
            mem::drop(keypair_file);
            keypair
        }
    }

    #[test]
    fn test_attest_valid() {
        let keypair = get_keypair();

        let ibmsg_client_a = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![4, 5, 6, 7],
            },
            seq_id: 42,
        };

        let ibmsg_client_b = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![8, 9, 10, 11],
            },
            seq_id: 42,
        };

        let client_a_attdata = AttestationData::from_inbox_msgs(
            "a",
            0,
            100,
            [&ibmsg_client_a].into_iter(),
        );
        let client_a_attestation = client_a_attdata.attest(&keypair);

        let client_b_attdata = AttestationData::from_inbox_msgs(
            "b",
            0,
            100,
            [&ibmsg_client_b].into_iter(),
        );
        let client_b_attestation = client_b_attdata.attest(&keypair);

        let client_a_claim = client_a_attdata.produce_claim_from_ibmsgs(
            "a".to_string(),
            0,
            100,
            vec![ibmsg_client_a],
            Some(0),
            client_a_attestation,
        );

        let client_b_pos_claim = client_b_attdata.produce_claim_from_ibmsgs(
            "b".to_string(),
            0,
            100,
            vec![ibmsg_client_b.clone()],
            Some(0),
            client_b_attestation.clone(),
        );

        let client_b_neg_claim = client_b_attdata.produce_claim_from_ibmsgs(
            "b".to_string(),
            0,
            100,
            vec![ibmsg_client_b],
            None,
            client_b_attestation,
        );

        assert!(
            client_a_claim.supports(&client_b_pos_claim, &keypair.public)
                == false
        );
        assert!(
            client_a_claim.supports(&client_b_neg_claim, &keypair.public)
                == false
        );
    }

    #[test]
    fn test_attest_postive_negative() {
        let keypair = get_keypair();

        let ibmsg_client_a = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![4, 5, 6, 7],
            },
            seq_id: 42,
        };

        let client_a_attdata = AttestationData::from_inbox_msgs(
            "a",
            0,
            100,
            [&ibmsg_client_a].into_iter(),
        );
        let client_a_attestation = client_a_attdata.attest(&keypair);

        let client_b_attdata = AttestationData::from_inbox_msgs(
            "b",
            0,
            100,
            <[&EncryptedInboxMessage; 0] as IntoIterator>::into_iter([]),
        );
        let client_b_attestation = client_b_attdata.attest(&keypair);

        let client_a_claim = client_a_attdata.produce_claim_from_ibmsgs(
            "a".to_string(),
            0,
            100,
            vec![ibmsg_client_a],
            Some(0),
            client_a_attestation,
        );

        let client_b_neg_claim = client_b_attdata.produce_claim_from_ibmsgs(
            "b".to_string(),
            0,
            100,
            vec![],
            None,
            client_b_attestation,
        );

        assert!(client_a_claim.supports(&client_b_neg_claim, &keypair.public));
    }

    #[test]
    fn test_attest_positive_positive_common() {
        let keypair = get_keypair();

        let ibmsg_client_a = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![4, 5, 6, 7],
            },
            seq_id: 42,
        };

        let ibmsg_client_b = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 99]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![8, 9, 10, 11],
            },
            seq_id: 42,
        };

        let client_a_attdata = AttestationData::from_inbox_msgs(
            "a",
            0,
            100,
            [&ibmsg_client_a].into_iter(),
        );
        let client_a_attestation = client_a_attdata.attest(&keypair);

        let client_b_attdata = AttestationData::from_inbox_msgs(
            "b",
            0,
            100,
            [&ibmsg_client_b].into_iter(),
        );
        let client_b_attestation = client_b_attdata.attest(&keypair);

        let client_a_claim = client_a_attdata.produce_claim_from_ibmsgs(
            "a".to_string(),
            0,
            100,
            vec![ibmsg_client_a],
            Some(0),
            client_a_attestation,
        );

        let client_b_claim = client_b_attdata.produce_claim_from_ibmsgs(
            "b".to_string(),
            0,
            100,
            vec![ibmsg_client_b.clone()],
            Some(0),
            client_b_attestation.clone(),
        );

        assert!(client_a_claim.supports(&client_b_claim, &keypair.public));
    }

    #[test]
    fn test_attest_positive_positive_per_rcpt() {
        let keypair = get_keypair();

        let ibmsg_client_a = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![4, 5, 6, 7],
            },
            seq_id: 42,
        };

        let client_a_attdata = AttestationData::from_inbox_msgs(
            "a",
            0,
            100,
            [&ibmsg_client_a].into_iter(),
        );

        let client_a_attestation = client_a_attdata.attest(&keypair);

        let mut sender_att_msg_per_rcpt = BTreeMap::new();
        sender_att_msg_per_rcpt.insert("a", Some((0, &[4, 5, 6, 8][..])));
        sender_att_msg_per_rcpt.insert("b", Some((0, &[8, 9, 10, 11][..])));
        let sender_att_msg = AttestationMessage {
            seq_id: 42,
            per_recipient: sender_att_msg_per_rcpt,
            common: &[0, 1, 2, 3][..],
        };
        let sender_attdata = AttestationData::from_att_msgs(
            "c",
            42,
            43,
            vec![sender_att_msg.clone()],
        );

        let sender_attestation = sender_attdata.attest(&keypair);

        let client_a_claim = client_a_attdata.produce_claim_from_ibmsgs(
            "a".to_string(),
            0,
            100,
            vec![ibmsg_client_a],
            Some(0),
            client_a_attestation,
        );

        let sender_claim = sender_attdata.produce_claim_from_att_msgs(
            "c".to_string(),
            42,
            43,
            vec![sender_att_msg],
            Some(0),
            sender_attestation.clone(),
        );

        assert!(client_a_claim.supports(&sender_claim, &keypair.public));
    }

    #[test]
    fn test_attest_positive_positive_per_rcpt_valid() {
        let keypair = get_keypair();

        let ibmsg_client_a = EncryptedInboxMessage {
            sender: "c".to_string(),
            recipients: vec!["a".to_string(), "b".to_string()],
            enc_common: EncryptedCommonPayload(vec![0, 1, 2, 3]),
            enc_recipient: EncryptedPerRecipientPayload {
                c_type: 0,
                ciphertext: vec![4, 5, 6, 7],
            },
            seq_id: 42,
        };

        let client_a_attdata = AttestationData::from_inbox_msgs(
            "a",
            0,
            100,
            [&ibmsg_client_a].into_iter(),
        );

        let client_a_attestation = client_a_attdata.attest(&keypair);

        let mut sender_att_msg_per_rcpt = BTreeMap::new();
        sender_att_msg_per_rcpt.insert("a", Some((0, &[4, 5, 6, 7][..])));
        sender_att_msg_per_rcpt.insert("b", Some((0, &[8, 9, 10, 11][..])));
        let sender_att_msg = AttestationMessage {
            seq_id: 42,
            per_recipient: sender_att_msg_per_rcpt,
            common: &[0, 1, 2, 3][..],
        };
        let sender_attdata = AttestationData::from_att_msgs(
            "c",
            42,
            43,
            vec![sender_att_msg.clone()],
        );

        let sender_attestation = sender_attdata.attest(&keypair);

        let client_a_claim = client_a_attdata.produce_claim_from_ibmsgs(
            "a".to_string(),
            0,
            100,
            vec![ibmsg_client_a],
            Some(0),
            client_a_attestation,
        );

        let sender_claim = sender_attdata.produce_claim_from_att_msgs(
            "c".to_string(),
            42,
            43,
            vec![sender_att_msg],
            Some(0),
            sender_attestation.clone(),
        );

        assert!(
            client_a_claim.supports(&sender_claim, &keypair.public) == false
        );
    }
}
