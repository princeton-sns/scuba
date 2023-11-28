use actix_web::http::header::TryIntoHeaderValue;
use actix_web::{delete, web, HttpResponse, Responder};
use tokio::time::{sleep, Duration};

use crate::shard;

async fn delete_messages(
    state: web::Data<AttProxyState>,
    auth: web::Header<shard::BearerToken>,
) -> crate::shard::client_protocol::MessageBatch<'static> {
    let bearer_token = auth.into_inner();
    let device_id = bearer_token.token().to_string();
    let bucket =
        crate::shard::hash_into_bucket(&device_id, state.shard_map.len(), true);
    let inbox_shard_url = &state.shard_map[bucket].0;

    let messages_resp = state
        .httpc
        .delete(&format!("{}/inbox-bin", inbox_shard_url))
        .header("Authorization", bearer_token.try_into_value().unwrap())
        .send()
        .await
        .unwrap();

    let parsed: shard::inbox::DeviceMessages =
        bincode::deserialize(&messages_resp.bytes().await.unwrap()).unwrap();

    let attestation =
        crate::shard::client_protocol::AttestationData::from_inbox_epochs(
            &device_id,
            parsed.from_epoch,
            parsed.to_epoch,
            parsed.messages.iter().flat_map(|(_, msgs)| msgs.iter()),
        )
        .attest(&state.attestation_key)
        .into_arr();

    crate::shard::client_protocol::MessageBatch {
        start_epoch_id: parsed.from_epoch,
        end_epoch_id: parsed.to_epoch,
        messages: std::borrow::Cow::Owned(
            parsed
                .messages
                .into_iter()
                .map(|(e, msgs)| (e, std::borrow::Cow::Owned(msgs)))
                .collect(),
        ),
        attestation: attestation.to_vec(),
    }
}

#[delete("/inbox-bin")]
async fn delete_messages_bin(
    state: web::Data<AttProxyState>,
    auth: web::Header<shard::BearerToken>,
) -> impl Responder {
    HttpResponse::Ok()
        .body(bincode::serialize(&delete_messages(state, auth).await).unwrap())
}

#[delete("/inbox")]
async fn delete_messages_json(
    state: web::Data<AttProxyState>,
    auth: web::Header<shard::BearerToken>,
) -> impl Responder {
    web::Json(delete_messages(state, auth).await)
}

struct AttProxyState {
    httpc: reqwest::Client,
    attestation_key: ed25519_dalek::Keypair,
    shard_map: Vec<(String, Option<String>)>,
}

pub async fn init(
    sequencer_base_url: &str,
) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::Path;

    // Read server attestation private key or generate a new one
    let keypair_path = Path::new("./attestation-key");
    let keypair = if let Ok(mut keypair_file) = File::open(keypair_path) {
        let mut keypair_bytes = [0; 64];
        assert!(keypair_file.read(&mut keypair_bytes).unwrap() == 64);
        Keypair::from_bytes(&keypair_bytes).unwrap()
    } else {
        let mut csprng = OsRng {};
        let keypair = Keypair::generate(&mut csprng);
        let mut keypair_file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(keypair_path)
            .unwrap();
        keypair_file.write(&keypair.to_bytes()).unwrap();
        keypair_file.flush().unwrap();
        std::mem::drop(keypair_file);
        keypair
    };

    {
        use base64::{engine::general_purpose, Engine as _};
        let encoded =
            general_purpose::STANDARD_NO_PAD.encode(&keypair.public.to_bytes());
        println!("Loaded attestation key, public key: {}", encoded);
    }

    let httpc = reqwest::Client::new();

    let mut shard_map_opt: Option<crate::sequencer::SequencerShardMap> = None;
    println!("Trying to retrieve shard map, this will loop until the expected number of shards have registered...");
    while shard_map_opt.is_none() {
        let resp = httpc
            .get(format!("{}/shard-map", sequencer_base_url))
            .send()
            .await;

        match resp {
            Ok(res) if res.status().is_success() => {
                shard_map_opt = Some(
                    res.json::<crate::sequencer::SequencerShardMap>()
                        .await
                        .unwrap(),
                );
            }
            _ => {
                print!(".");
                std::io::stdout().flush().unwrap();
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
    println!("Retrieved shard map!");

    let state = web::Data::new(AttProxyState {
        httpc,
        attestation_key: keypair,
        shard_map: shard_map_opt.unwrap().shards,
    });

    Box::new(move |service_config: &mut web::ServiceConfig| {
        service_config
            .app_data(state.clone())
            .service(delete_messages_json)
            .service(delete_messages_bin);
    })
}
