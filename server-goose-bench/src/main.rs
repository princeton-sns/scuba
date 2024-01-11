/**
 * Notes: there must be better ways to do half of these things. Things to fix:
 * - have multiple transactions in a scenario over the user username in the
 *   system
 * - reuse request generators in complex patterns like deletepostdelete
 * - make username generation based on number of users
 */
use goose::prelude::*;

use rand::seq::SliceRandom;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};

use scuba_server_lib::shard::client_protocol::{
    EncryptedCommonPayload, EncryptedOutboxMessage, EncryptedPerRecipientPayload,
};

// Horribly unsafe AND unsound, never do this, this WILL break, it's terrible.
static mut USERNAMES: Option<Vec<String>> = None;
static mut COMMON_PAYLOADS: Option<HashMap<String, EncryptedOutboxMessage>> = None;
static COMMON_PAYLOAD_LEN: AtomicUsize = AtomicUsize::new(usize::MAX);
static INDIVIDUAL_PAYLOAD_LEN: AtomicUsize = AtomicUsize::new(usize::MAX);

static GOOSE_USER_COUNT: AtomicUsize = AtomicUsize::new(0);

#[tokio::main]
async fn main() -> Result<(), GooseError> {
    let g = GooseAttack::initialize()?;

    COMMON_PAYLOAD_LEN.store(
        std::env::vars()
            .find(|(var, _)| var == "NOISE_CMPLDLEN")
            .map(|(_, val)| val)
            .and_then(|val| val.parse::<usize>().ok())
            .unwrap_or(12),
        Ordering::Relaxed,
    );

    INDIVIDUAL_PAYLOAD_LEN.store(
        std::env::vars()
            .find(|(var, _)| var == "NOISE_IDPLDLEN")
            .map(|(_, val)| val)
            .and_then(|val| val.parse::<usize>().ok())
            .unwrap_or(42),
        Ordering::Relaxed,
    );

    let usernames_str = std::env::vars()
        .find(|(var, _)| var == "NOISE_USERS")
        .map(|(_, val)| val)
        .unwrap();
    let usernames: Vec<String> =
        usernames_str.split(":").map(|s| s.to_string()).collect();
    unsafe { USERNAMES = Some(usernames.clone()) };

    let friends_str = std::env::vars()
        .find(|(var, _)| var == "NOISE_FRIENDS")
        .map(|(_, val)| val)
        .unwrap_or_else(|| "".to_string());

    if friends_str != "" {
        println!("Running with individual payloads per sender name!");
        let sender_friends: Vec<Vec<String>> = friends_str
            .split(":")
            .map(|s| s.split(",").map(|s| s.to_string()).collect())
            .collect();
        let common_payloads: HashMap<String, EncryptedOutboxMessage> = usernames
            .into_iter()
            .zip(sender_friends.into_iter())
            .map(|(sender, friends)| {
                (
                    sender.clone(),
                    construct_message(
                        &sender,
                        friends.into_iter().map(|f| Cow::Owned(f)),
                    ),
                )
            })
            .collect();
        unsafe { COMMON_PAYLOADS = Some(common_payloads) };
    } else {
        println!("Running with individual payloads!");
    }

    g.register_scenario(
        scenario!("BenchmarkScenario")
            .register_transaction(
                transaction!(set_username)
                    .set_name("generate username")
                    .set_on_start(),
            )
            .register_transaction(transaction!(post_message).set_name("post request")),
    )
    // This is important to avoid constant redirects and overloading any
    // single shard. The server will automatically direct individual users
    // to the right shards given their chosen user id.
    .set_default(GooseDefault::StickyFollow, true)
    .unwrap()
    //.register_scenario(
    //    scenario!("DeleteOneMailbox")
    //        .register_transaction(transaction!(set_username).set_name("generate
    // username"))        .register_transaction(transaction!(delete_mailbox).
    // set_name("delete mailbox")),
    //)
    //.register_scenario(
    //    scenario!("GetOneMailbox")
    //        .register_transaction(transaction!(set_username).set_name("generate
    // username"))        .register_transaction(transaction!(get_mailbox).set_name("
    // get mailbox")),
    //)
    //.register_scenario(
    //    scenario!("DeletePostGetDelete")
    //        .register_transaction(transaction!(set_username).set_name("generate
    // username"))        .register_transaction(transaction!(delete_mailbox).
    // set_name("delete mailbox"))        .register_transaction(transaction!
    // (post_message).set_name("post request"))
    //        .register_transaction(transaction!(get_mailbox).set_name("get mailbox"))
    //        .register_transaction(transaction!(delete_mailbox).set_name("delete
    // mailbox")),
    //)
    //.register_scenario(
    //    scenario!("ValidateGetMessageAfterPost")
    //        .register_transaction(transaction!(set_username).set_name("generate
    // username"))        .register_transaction(transaction!(loop_message).set_name("
    // loop message")),
    //)
    .execute()
    .await?;

    Ok(())
}

#[derive(Serialize)]
struct Username(String);

async fn set_username(user: &mut GooseUser) -> TransactionResult {
    let deterministic_users = std::env::vars()
        .find(|(var, _)| var == "DET_USERS")
        .map(|(_, content)| content != "")
        .unwrap_or(false);

    let username = if deterministic_users {
        (unsafe { USERNAMES.as_ref().unwrap() })
            .get(GOOSE_USER_COUNT.fetch_add(1, Ordering::Relaxed))
            .unwrap()
            .clone()
    } else {
        (unsafe { USERNAMES.as_ref().unwrap() })
            .choose(&mut rand::thread_rng())
            .unwrap()
            .clone()
    };

    user.set_session_data(Username(username));

    Ok(())
}

fn construct_message<'a>(
    _sender: &str,
    friends: impl Iterator<Item = Cow<'a, str>>,
) -> EncryptedOutboxMessage {
    let common_payload = std::iter::repeat(b'X' as u8)
        .take(COMMON_PAYLOAD_LEN.load(Ordering::Relaxed))
        .collect::<Vec<u8>>();

    let recipient_payloads: BTreeMap<String, EncryptedPerRecipientPayload> = friends
        .map(|f| {
            let ciphertext = std::iter::repeat(b'X' as u8)
                .take(INDIVIDUAL_PAYLOAD_LEN.load(Ordering::Relaxed))
                .collect::<Vec<u8>>();
            (
                f.into_owned(),
                EncryptedPerRecipientPayload {
                    c_type: 0,
                    ciphertext,
                },
            )
        })
        .collect();

    EncryptedOutboxMessage {
        enc_common: EncryptedCommonPayload(common_payload),
        enc_recipients: recipient_payloads,
    }
}

async fn post_message(user: &mut GooseUser) -> TransactionResult {
    let sender = &user.get_session_data::<Username>().unwrap().0;

    let mut headers = HeaderMap::new();
    let auth_name = "Bearer ".to_owned() + &sender;
    headers.insert("Authorization", HeaderValue::from_str(&auth_name).unwrap());
    headers.insert(
        "Content-Type",
        HeaderValue::from_str("application/json").unwrap(),
    );

    let request_builder = user
        // JSON:
        // .get_request_builder(&GooseMethod::Post, "/message")?
        .get_request_builder(&GooseMethod::Post, "/message-bin")?
        .headers(headers);

    let msg: Cow<'static, EncryptedOutboxMessage> = unsafe {
        COMMON_PAYLOADS
            .as_ref()
            .map(|sender_map| Cow::Borrowed(sender_map.get(sender).unwrap()))
            .unwrap_or_else(|| {
                Cow::Owned(construct_message(
                    sender,
                    [Cow::Borrowed(sender.as_str())].into_iter(),
                ))
            })
    };

    // JSON:
    // let goose_request = GooseRequest::builder()
    //     .method(GooseMethod::Post)
    //     .path("/message")
    //     .set_request_builder(request_builder.
    // json(Borrow::<EncryptedOutboxMessage>::borrow(&msg)))     .build();
    let mut ll = std::collections::LinkedList::new();
    ll.push_back(msg.into_owned());
    let serialized = bincode::serialize(&ll).unwrap();
    //panic!("Serialized: {:?}", serialized);
    let goose_request = GooseRequest::builder()
        .method(GooseMethod::Post)
        .path("/message-bin")
        .set_request_builder(request_builder.body(serialized))
        .build();

    let _goose_metrics = user.request(goose_request).await;

    Ok(())
}
