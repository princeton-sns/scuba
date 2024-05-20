use crate::server_comm::ServerComm;
use async_condvar_fair::Condvar;
use olm_rs::account::{IdentityKeys, OlmAccount, OneTimeKeys};
use olm_rs::session::{OlmMessage, OlmSession, PreKeyMessage};
use parking_lot::Mutex;
use rand::RngCore;
use std::collections::{HashMap, VecDeque};
use std::mem;

// TODO sender-key optimization

const NUM_OTKEYS: usize = 20;

// TODO persist natively
pub struct Crypto {
    turn_encryption_off: bool,
    idkeys: IdentityKeys,
    // Wrap OlmAccount and MessageQueue in Mutex for Send/Sync
    pub account: Mutex<OlmAccount>,
    message_queue: Mutex<VecDeque<Vec<u8>>>,
    // Wrap entire HashMap in a Mutex for Send/Sync; this is ok because
    // any time sessions is accessed we have a &mut self - no deadlock
    // risk b/c only one &mut self can be helf at a time, anyway
    pub sessions: Mutex<HashMap<String, (bool, Vec<OlmSession>)>>,
    sessions_cv: Condvar,
}

// TODO impl Error enum

impl Crypto {
    pub fn new(turn_encryption_off: bool) -> Self {
        let account = Mutex::new(OlmAccount::new());
        let idkeys = account.lock().parsed_identity_keys();
        Self {
            turn_encryption_off,
            idkeys,
            account,
            message_queue: Mutex::new(VecDeque::new()),
            sessions: Mutex::new(HashMap::new()),
            sessions_cv: Condvar::new(),
        }
    }

    pub fn symmetric_encrypt(
        &self,
        mut pt: Vec<u8>,
    ) -> (Vec<u8>, [u8; 16], [u8; 32], [u8; 12]) {
        use aes_gcm::{
            aead::{AeadInPlace, KeyInit},
            Aes256Gcm,
            //Nonce, // Or `Aes128Gcm`
        };

        // Convert the plain text to bytes and short-circuit if encryption is
        // disabled.
        if self.turn_encryption_off {
            return (pt, [0; 16], [0; 32], [0; 12]);
        }

        let mut rng = rand::thread_rng();

        let key = Aes256Gcm::generate_key(&mut rng);

        let mut nonce = [0u8; 12];
        rng.fill_bytes(&mut nonce);

        // We may want to include additional context such as the sending user
        // in the associated data:
        let tag = Aes256Gcm::new(&key)
            .encrypt_in_place_detached(&nonce.into(), &[], &mut pt)
            .unwrap();

        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(key.as_slice());

        let mut tag_arr = [0u8; 16];
        tag_arr.copy_from_slice(tag.as_slice());

        (pt, tag_arr, key_arr, nonce)
    }

    pub fn symmetric_decrypt(
        &self,
        mut ct: Vec<u8>,
        key: [u8; 32],
        tag: [u8; 16],
        nonce: [u8; 12],
    ) -> Vec<u8> {
        use aes_gcm::{
            aead::{AeadInPlace, KeyInit},
            Aes256Gcm,
            //Nonce, // Or `Aes128Gcm`
        };

        if self.turn_encryption_off {
            return ct;
        }

        Aes256Gcm::new(&key.into())
            .decrypt_in_place_detached(&nonce.into(), &[], &mut ct, &tag.into())
            .unwrap();

        ct
    }

    pub fn generate_otkeys(&self, num: Option<usize>) -> OneTimeKeys {
        let account = self.account.lock();
        account.generate_one_time_keys(num.unwrap_or(NUM_OTKEYS));
        let otkeys = account.parsed_one_time_keys();
        account.mark_keys_as_published();
        otkeys
    }

    pub fn get_idkey(&self) -> String {
        self.idkeys.curve25519().to_string()
    }

    async fn new_outbound_session<S: ServerComm>(
        &self,
        server_comm: &S,
        dst_idkey: &String,
    ) -> OlmSession {
        match server_comm.get_otkey_from_server(dst_idkey).await {
            Ok(dst_otkey) => {
                match self
                    .account
                    .lock()
                    .create_outbound_session(dst_idkey, &String::from(dst_otkey))
                {
                    Ok(new_session) => return new_session,
                    Err(err) => {
                        panic!("Error creating outbound session: {:?}", err)
                    }
                }
            }
            Err(err) => panic!("Error getting otkey from server: {:?}", err),
        }
    }

    fn new_inbound_session(&self, prekey_msg: &PreKeyMessage) -> OlmSession {
        match self
            .account
            .lock()
            .create_inbound_session(prekey_msg.clone())
        {
            Ok(new_session) => return new_session,
            Err(err) => panic!("Error creating inbound session: {:?}", err),
        }
    }

    // TODO how many sessions with the same session_id should
    // exist at one time? (for decrypting delayed messages)
    // -> currently infinite

    async fn get_outbound_session<R, S: ServerComm>(
        &self,
        server_comm: &S,
        dst_idkey: &String,
        f: impl FnOnce(&OlmSession) -> R,
    ) -> R {
        loop {
            let mut sessions = self.sessions.lock();
            // if sessions[dst_idkey] is None, make it Some([])
            let (is_fetching, sessions_list) = sessions
                .entry(dst_idkey.to_string())
                .or_insert_with(|| (false, Vec::new()));
            // if last session entry has received a message, it is valid
            // so we use it
            if !sessions_list.is_empty()
                && sessions_list[sessions_list.len() - 1].has_received_message()
            {
                return f(&sessions_list[sessions_list.len() - 1]);
            }
            // wait if another thread has already started fetching
            // otkeys and creating a new session, and then
            // re-execute the loop
            if *is_fetching {
                let _ = self.sessions_cv.wait(sessions).await;
            // fetch otkeys from the server and create a new
            // session
            } else {
                *is_fetching = true;
                mem::drop(sessions);
                let new_session = self.new_outbound_session(server_comm, dst_idkey).await;
                let mut sessions = self.sessions.lock();
                let (is_fetching, sessions_list) = sessions.get_mut(dst_idkey).unwrap();
                *is_fetching = false;
                sessions_list.push(new_session);
                self.sessions_cv.notify_all();
                return f(&sessions_list[sessions_list.len() - 1]);
            }
        }
    }

    fn get_inbound_session<R>(
        &self,
        sender: &String,
        ciphertext: &OlmMessage,
        f: impl FnOnce(&OlmSession) -> R,
    ) -> R {
        let mut sessions = self.sessions.lock();
        match ciphertext {
            OlmMessage::Message(_) => {
                if sessions.get(sender).is_none() {
                    panic!("No pairwise sessions exist for idkey {:?}", sender);
                } else {
                    let sessions_list = &mut sessions.get_mut(sender).unwrap().1;
                    f(&sessions_list[sessions_list.len() - 1])
                }
            }
            OlmMessage::PreKey(prekey) => {
                if sessions.get(sender).is_none() {
                    sessions.insert(
                        sender.to_string(),
                        (false, vec![self.new_inbound_session(&prekey)]),
                    );
                } else {
                    let new_session = self.new_inbound_session(&prekey);
                    sessions.get_mut(sender).unwrap().1.push(new_session);
                }
                let sessions_list = &sessions.get(sender).unwrap().1;
                f(&sessions_list[sessions_list.len() - 1])
            }
        }
    }

    fn try_all_sessions_decrypt(
        &self,
        sender: &String,
        ciphertext: &OlmMessage,
    ) -> Vec<u8> {
        // as long as get_inbound_session is called before this
        // function the result will never be None/empty
        let sessions = self.sessions.lock();
        let sessions_list = &sessions.get(sender).unwrap().1;

        // skip the len - 1'th session since that was already tried
        for session in sessions_list.iter().rev().skip(1) {
            match session.decrypt(ciphertext.clone()) {
                Ok(plaintext) => {
                    use base64::{engine::general_purpose, Engine as _};
                    return general_purpose::STANDARD_NO_PAD.decode(plaintext).unwrap();
                }
                _ => continue,
            }
        }
        panic!("No matching sessions were found");
    }

    pub async fn session_encrypt<S: ServerComm>(
        &self,
        server_comm: &S,
        dst_idkey: &String,
        plaintext: Vec<u8>,
    ) -> (usize, Vec<u8>) {
        if self.turn_encryption_off {
            return (1, plaintext);
        }
        self.session_encrypt_helper(server_comm, dst_idkey, plaintext)
            .await
    }

    async fn session_encrypt_helper<S: ServerComm>(
        &self,
        server_comm: &S,
        dst_idkey: &String,
        plaintext: Vec<u8>,
    ) -> (usize, Vec<u8>) {
        if *dst_idkey == self.get_idkey() {
            self.message_queue.lock().push_front(plaintext);
            return (1, Vec::<u8>::new());
        }
        use base64::{engine::general_purpose, Engine as _};
        let encoded = &general_purpose::STANDARD_NO_PAD.encode(plaintext);
        let (c_type, ciphertext) = self
            .get_outbound_session(server_comm, dst_idkey, |session| {
                session
                    .encrypt(encoded)
                    //&bincode::deserialize::<String>(&plaintext).unwrap())
                    .to_tuple()
            })
            .await;
        (c_type.into(), ciphertext.into())
    }

    pub fn session_decrypt(
        &self,
        sender: &String,
        c_type: usize,
        ciphertext: Vec<u8>,
    ) -> Vec<u8> {
        if self.turn_encryption_off {
            return ciphertext;
        }
        self.session_decrypt_helper(
            sender,
            &OlmMessage::from_type_and_ciphertext(
                c_type,
                String::from_utf8(ciphertext).unwrap(),
            )
            .unwrap(),
        )
    }

    fn session_decrypt_helper(
        &self,
        sender: &String,
        ciphertext: &OlmMessage,
    ) -> Vec<u8> {
        if *sender == self.get_idkey() {
            // FIXME handle dos attack where client poses as "self" -
            // this unwrap will panic
            return self.message_queue.lock().pop_back().unwrap();
        }
        let res = self.get_inbound_session(sender, ciphertext, |session| {
            session.decrypt(ciphertext.clone())
        });

        match res {
            Ok(plaintext) => {
                use base64::{engine::general_purpose, Engine as _};
                return general_purpose::STANDARD_NO_PAD.decode(plaintext).unwrap();
            }
            Err(err) => {
                match ciphertext {
                    // iterate through all sessions in case this message was
                    // delayed
                    OlmMessage::Message(_) => {
                        self.try_all_sessions_decrypt(sender, ciphertext)
                    }
                    OlmMessage::PreKey(_) => {
                        panic!(
                            "Error creating inbound session from prekey message: {:?}",
                            err
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Crypto, NUM_OTKEYS};
    use crate::core::stream_client::StreamClient;
    use std::sync::Arc;

    #[test]
    fn test_new() {
        let crypto = Crypto::new(false);
        assert_eq!(crypto.turn_encryption_off, false);
    }

    #[test]
    fn test_idkey() {
        let crypto = Crypto::new(false);
        println!("idkey: {:?}", crypto.get_idkey());
    }

    #[test]
    fn test_gen_otkeys() {
        let crypto = Crypto::new(false);
        let otkeys = crypto.generate_otkeys(None);
        assert_eq!(NUM_OTKEYS, otkeys.curve25519().len());
        println!("otkeys: {:?}", otkeys.curve25519());
    }

    #[test]
    fn test_gen_otkeys_custom_num() {
        let num = 7;
        let crypto = Crypto::new(false);
        let otkeys = crypto.generate_otkeys(Some(num));
        assert_eq!(num, otkeys.curve25519().len());
        println!("otkeys: {:?}", otkeys.curve25519());
    }

    /*
        #[tokio::test]
        async fn test_dummy_encrypt() {
            let (client, mut receiver) = StreamClient::new();
            let arc_client = Arc::new(client);

            let crypto = Crypto::new(true);
            let idkey = crypto.get_idkey();
            //let server_comm = ServerComm::new(None, None, idkey.clone(), None);
            let plaintext = bincode::serialize(&String::from("hello")).unwrap();
            let (_, ciphertext) = crypto.session_encrypt(&receiver, &idkey, plaintext).await;
            assert_eq!(plaintext, ciphertext);
        }

        #[tokio::test]
        async fn test_self_encrypt() {
            let crypto = Crypto::new(false);
            let idkey = crypto.get_idkey();
            let server_comm = ServerComm::new(None, None, idkey.clone(), None);
            let plaintext = bincode::serialize(&String::from("hello")).unwrap();
            let empty = bincode::serialize(&String::from("")).unwrap();
            let (_, ciphertext) = crypto.session_encrypt(&server_comm, &idkey, plaintext).await;
            assert_eq!(empty, ciphertext);
            assert_eq!(plaintext, crypto.message_queue.lock().pop_back().unwrap());
        }

        #[test]
        fn test_dummy_decrypt() {
            let crypto = Crypto::new(true);
            let idkey = crypto.get_idkey();
            let plaintext = bincode::serialize(&String::from("hello")).unwrap();
            let decrypted = crypto.session_decrypt(&idkey, 1, plaintext);
            assert_eq!(plaintext, decrypted);
        }

        #[tokio::test]
        async fn test_self_decrypt() {
            let crypto = Crypto::new(false);
            let idkey = crypto.get_idkey();
            let server_comm = ServerComm::new(None, None, idkey.clone(), None);
            let plaintext = bincode::serialize(&String::from("hello")).unwrap();
            let empty = bincode::serialize(&String::from("")).unwrap();
            let (c_type, ciphertext) = crypto.session_encrypt(&server_comm, &idkey, plaintext).await;
            let decrypted = crypto.session_decrypt(&idkey, c_type, ciphertext);
            assert_eq!(empty, ciphertext);
            assert_eq!(plaintext, decrypted);
        }

        #[tokio::test]
        async fn test_self_outbound_session() {
            let crypto = Crypto::new(false);
            let idkey = crypto.get_idkey();
            let server_comm = ServerComm::init(None, None, &crypto).await;
            let session = crypto.new_outbound_session(&server_comm, &idkey).await;
            println!("New session: {:?}", session);
            println!("New session ID: {:?}", session.session_id());
            assert!(!session.has_received_message());
        }

        #[tokio::test]
        async fn test_encrypt_and_decrypt_once() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let _ = ServerComm::init(None, None, &ow2).await;

            let plaintext = String::from("testing testing one two three");

            let (c_type, ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &plaintext).await;
            let decrypted = ow2.session_decrypt(&idkey1, c_type, &ciphertext);

            assert_eq!(plaintext, decrypted);
        }

        #[tokio::test]
        async fn test_get_session_init() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let _ = ServerComm::init(None, None, &ow2).await;

            let plaintext = "testing testing one two three";

            // 1 -> 2
            assert_eq!(None, ow1.sessions.lock().get(&idkey2));
            assert_eq!(None, ow2.sessions.lock().get(&idkey1));

            ow1.get_outbound_session(&sc1, &idkey2, |ob_session| {
                let ciphertext = ob_session.session_encrypt(plaintext);

                // using prekey
                ow2.get_inbound_session(&idkey1, &ciphertext, |ib_session| {
                    assert_eq!(ob_session.session_id(), ib_session.session_id());

                    // NOTE taking any lock in the callbacks of either
                    // get_outbound_session() or get_inbound_session() will
                    // result in a deadlock since they hold onto sessions locks
                    // until the callback arguments _finish running_
                    //
                    // This is not publicly-exposed behavior, so users of the
                    // library will not run into this nor should they really
                    // think about it
                });
            })
            .await;

            let ow1_sessions = ow1.sessions.lock();
            let ow2_sessions = ow2.sessions.lock();
            let ow1_session_list = ow1_sessions.get(&idkey2);
            let ow2_session_list = ow2_sessions.get(&idkey1);

            assert_ne!(None, ow1_session_list);
            assert_ne!(None, ow2_session_list);
            assert_eq!(ow1_session_list.unwrap().1.len(), 1);
            assert_eq!(ow2_session_list.unwrap().1.len(), 1);
        }

        #[tokio::test]
        async fn test_get_session_without_received_msg() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let _ = ServerComm::init(None, None, &ow2).await;

            let plaintext = "testing testing one two three";

            // 1 -> 2
            assert_eq!(None, ow1.sessions.lock().get(&idkey2));
            assert_eq!(None, ow2.sessions.lock().get(&idkey1));

            let mut first_ob_id: String = Default::default();
            let mut first_ib_id: String = Default::default();

            ow1.get_outbound_session(&sc1, &idkey2, |first_ob_session| {
                let ciphertext = first_ob_session.session_encrypt(plaintext);
                // using prekey
                ow2.get_inbound_session(&idkey1, &ciphertext, |first_ib_session| {
                    // decrypt() sets flag for has_received_message()
                    let decrypted = first_ib_session.session_decrypt(ciphertext.clone()).unwrap();
                    assert_eq!(plaintext, decrypted);

                    first_ob_id = first_ob_session.session_id().clone();
                    first_ib_id = first_ib_session.session_id().clone();
                });
            })
            .await;

            assert_eq!(first_ob_id, first_ib_id);

            // 1 -> 2 again
            ow1.get_outbound_session(&sc1, &idkey2, |second_ob_session| {
                let ciphertext = second_ob_session.session_encrypt(plaintext);
                // using prekey
                ow2.get_inbound_session(&idkey1, &ciphertext, |second_ib_session| {
                    let second_ob_id = second_ob_session.session_id().clone();
                    let second_ib_id = second_ib_session.session_id().clone();

                    assert_eq!(second_ob_id, second_ib_id);
                    assert_ne!(first_ob_id, second_ob_id);
                    assert_ne!(first_ib_id, second_ib_id);
                });
            })
            .await;
        }

        #[tokio::test]
        async fn test_get_session_with_received_msg() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let sc2 = ServerComm::init(None, None, &ow2).await;

            let plaintext = "testing testing one two three";

            // 1 -> 2
            assert_eq!(None, ow1.sessions.lock().get(&idkey2));
            assert_eq!(None, ow2.sessions.lock().get(&idkey1));

            let mut first_ob_id: String = Default::default();
            let mut first_ib_id: String = Default::default();

            ow1.get_outbound_session(&sc1, &idkey2, |first_ob_session| {
                let first_ciphertext = first_ob_session.session_encrypt(plaintext);
                // using prekey
                ow2.get_inbound_session(&idkey1, &first_ciphertext, |first_ib_session| {
                    // decrypt() sets flag for has_received_message()
                    let decrypted = first_ib_session.session_decrypt(first_ciphertext.clone()).unwrap();
                    assert_eq!(plaintext, decrypted);

                    first_ob_id = first_ob_session.session_id().clone();
                    first_ib_id = first_ib_session.session_id().clone();
                });
            })
            .await;

            // 2 -> 1
            ow2.get_outbound_session(&sc2, &idkey1, |second_ob_session| {
                let second_ciphertext = second_ob_session.session_encrypt(plaintext);
                // using message
                ow1.get_inbound_session(&idkey2, &second_ciphertext, |second_ib_session| {
                    let second_ob_id = second_ob_session.session_id().clone();
                    let second_ib_id = second_ib_session.session_id().clone();

                    assert_eq!(first_ob_id, first_ib_id);
                    assert_eq!(second_ob_id, second_ib_id);
                    assert_eq!(first_ob_id, second_ib_id);
                    assert_eq!(first_ib_id, second_ob_id);
                });
            })
            .await;
        }

        #[tokio::test]
        async fn test_encrypt_and_decrypt_without_received_msg() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let _ = ServerComm::init(None, None, &ow2).await;

            // 1 -> 2
            let first_plaintext = String::from("testing testing one two three");
            let (first_ctype, first_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &first_plaintext).await;
            let first_decrypted = ow2.session_decrypt(&idkey1, first_ctype, &first_ciphertext);
            assert_eq!(first_plaintext, first_decrypted);

            // 1 -> 2
            let second_plaintext = String::from("three two one testing testing");
            let (second_ctype, second_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &second_plaintext).await;
            let second_decrypted = ow2.session_decrypt(&idkey1, second_ctype, &second_ciphertext);
            assert_eq!(second_plaintext, second_decrypted);
        }

        #[tokio::test]
        async fn test_encrypt_and_decrypt_with_received_msg() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let sc2 = ServerComm::init(None, None, &ow2).await;

            // 1 -> 2
            let first_plaintext = String::from("testing testing one two three");
            let (first_ctype, first_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &first_plaintext).await;
            let first_decrypted = ow2.session_decrypt(&idkey1, first_ctype, &first_ciphertext);
            assert_eq!(first_plaintext, first_decrypted);

            // 2 -> 1
            let second_plaintext = String::from("three two one testing testing");
            let (second_ctype, second_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &second_plaintext).await;
            let second_decrypted = ow1.session_decrypt(&idkey2, second_ctype, &second_ciphertext);
            assert_eq!(second_plaintext, second_decrypted);
        }

        #[tokio::test]
        async fn test_delayed_message() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let sc2 = ServerComm::init(None, None, &ow2).await;

            // encrypt 1 -> 2 and "send" (decrypt)
            let first_plaintext = String::from("testing testing one two three");
            let (first_ctype, first_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &first_plaintext).await;
            let first_decrypted = ow2.session_decrypt(&idkey1, first_ctype, &first_ciphertext);
            assert_eq!(first_plaintext, first_decrypted);

            // encrypt another 1 -> 2 without "sending" (decrypting) - uses a diff session
            // b/c has not yet received a response
            let second_plaintext = String::from("three two one testing testing");
            let (second_ctype, second_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &second_plaintext).await;

            // encrypt 2 -> 1 and "send" (decrypt)
            let third_plaintext = String::from("one testing three testing two");
            let (third_ctype, third_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &third_plaintext).await;
            let third_decrypted = ow1.session_decrypt(&idkey2, third_ctype, &third_ciphertext);
            assert_eq!(third_plaintext, third_decrypted);

            // "send" (decrypt) second message
            let second_decrypted = ow2.session_decrypt(&idkey1, second_ctype, &second_ciphertext);
            assert_eq!(second_plaintext, second_decrypted);
        }

        #[tokio::test]
        async fn test_very_delayed_message() {
            let ow1 = Crypto::new(false);
            let idkey1 = ow1.get_idkey();
            println!("idkey1: {:?}", idkey1);
            let sc1 = ServerComm::init(None, None, &ow1).await;

            let ow2 = Crypto::new(false);
            let idkey2 = ow2.get_idkey();
            println!("idkey2: {:?}", idkey2);
            let sc2 = ServerComm::init(None, None, &ow2).await;

            let plaintext = String::from("testing testing one two three");

            // encrypt 1 -> 2 and "send" (decrypt)
            let (first_ctype, first_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &plaintext).await;
            let first_decrypted = ow2.session_decrypt(&idkey1, first_ctype, &first_ciphertext);
            assert_eq!(plaintext, first_decrypted);

            // encrypt another 1 -> 2 without "sending" (decrypting) - uses a diff session
            // b/c has not yet received a response
            let (second_ctype, second_ciphertext) = ow1.session_encrypt(&sc1, &idkey2, &plaintext).await;

            // encrypt 2 -> 1 and "send" (decrypt)
            let (third_ctype, third_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &plaintext).await;
            let third_decrypted = ow1.session_decrypt(&idkey2, third_ctype, &third_ciphertext);
            assert_eq!(plaintext, third_decrypted);

            // encrypt another 2 -> 1 and "send" (decrypt)
            let (fourth_ctype, fourth_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &plaintext).await;
            let fourth_decrypted = ow1.session_decrypt(&idkey2, fourth_ctype, &fourth_ciphertext);
            assert_eq!(plaintext, fourth_decrypted);

            // encrypt another 2 -> 1 and "send" (decrypt)
            let (fifth_ctype, fifth_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &plaintext).await;
            let fifth_decrypted = ow1.session_decrypt(&idkey2, fifth_ctype, &fifth_ciphertext);
            assert_eq!(plaintext, fifth_decrypted);

            // encrypt another 2 -> 1 and "send" (decrypt)
            let (sixth_ctype, sixth_ciphertext) = ow2.session_encrypt(&sc2, &idkey1, &plaintext).await;
            let sixth_decrypted = ow1.session_decrypt(&idkey2, sixth_ctype, &sixth_ciphertext);
            assert_eq!(plaintext, sixth_decrypted);

            // "send" (decrypt) second message
            let second_decrypted = ow2.session_decrypt(&idkey1, second_ctype, &second_ciphertext);
            assert_eq!(plaintext, second_decrypted);
        }

        // TODO add test that stresses adding two sessions at once
    */

    /*
    use crate::core::PerRecipientPayload;
    use crate::crypto::Crypto;
    use crate::hash_vectors::{CommonPayload, ValidationPayload};

    #[test]
    fn test_symmetric_encrypt_and_decrypt() {
        let crypto1 = Crypto::new(false);
        let idkey1 = crypto1.get_idkey();
        let crypto2 = Crypto::new(false);
        let idkey2 = crypto2.get_idkey();

        let message = String::from("testingtesting123");
        let cp = CommonPayload::new(
            vec![idkey1.clone(), idkey2.clone()],
            message.clone(),
        );
        let (ct, key, iv) =
            crypto1.symmetric_encrypt(CommonPayload::to_string(&cp));

        let pt = crypto2.symmetric_decrypt(ct, key, iv);
        let common_payload = CommonPayload::from_string(pt);

        assert_eq!(common_payload.message().to_string(), message);
    }
    */

    //#[tokio::test]
    //async fn test_complete_encryption_and_decryption() {
    //    let crypto1 = Crypto::new(false);
    //    let idkey1 = crypto1.get_idkey();
    //    let crypto2 = Crypto::new(false);
    //    let idkey2 = crypto2.get_idkey();

    //    let recipients = vec![idkey1.clone(), idkey2.clone()];

    //    let message = String::from("testingtesting123");

    //    let cp = CommonPayload::new(
    //        recipients.clone(),
    //        message.clone(),
    //    );
    //    let (ct, key, iv) =
    // crypto1.symmetric_encrypt(CommonPayload::to_string(&cp));

    //    let mut batch = Batch::new();

    //    let vp1 = ValidationPayload::dummy(recipients.clone(),
    // message.clone());    let vp2 =
    // ValidationPayload::dummy(recipients.clone(), message.clone());

    //    let pr1_string = PerRecipientPayload::new_and_to_string(vp1, key,
    // iv);    let pr2_string =
    // PerRecipientPayload::new_and_to_string(vp2, key, iv);

    //    //let (ctype1, pr1_ct) = crypto1.session_encrypt(
    //    //
    //    //).await;
    //}
}
