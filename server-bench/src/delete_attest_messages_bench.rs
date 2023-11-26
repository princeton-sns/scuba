use serde::Serialize;
use std::collections::LinkedList;
use std::sync::Arc;

use clap::Parser;

#[derive(Parser, Debug)]
pub struct Opts {
    // This can also point to an attestation proxy
    #[arg(long)]
    shard_url: String,

    #[arg(long)]
    client_limit: usize,

    #[arg(long)]
    client_interval_us: Option<u64>,

    #[arg(long)]
    stats_file: Option<String>,
}

#[derive(Serialize)]
struct StatEntry {
    duration: u128,
    device_id: String,
    start_epoch: u64,
    end_epoch: u64,
    message_count: usize,
}

pub async fn run(opts: Opts) {
    let devices_str = std::env::vars()
        .find(|(var, _)| var == "NOISE_USERS")
        .map(|(_, val)| val)
        .unwrap();
    let devices: LinkedList<String> =
        devices_str.split(":").map(|s| s.to_string()).collect();
    let devices_len = devices.len();

    let devices_locked: Arc<tokio::sync::Mutex<LinkedList<String>>> =
        Arc::new(tokio::sync::Mutex::new(devices));
    let devices_notify = Arc::new(tokio::sync::Notify::new());
    let tasks = std::cmp::min(opts.client_limit, devices_len);

    let httpc = reqwest::Client::new();

    let (stat_send, mut stat_recv) = tokio::sync::mpsc::unbounded_channel();
    let arc_stat_send = Arc::new(stat_send);

    println!("Spawning {} tasks...", tasks);
    let mut handles = vec![];
    for i in 0..tasks {
        let devices_locked_cloned = devices_locked.clone();
        let devices_notify_cloned = devices_notify.clone();
        let httpc_cloned = httpc.clone();
        let shard_url_cloned = opts.shard_url.clone();
        let arc_stat_send_cloned = arc_stat_send.clone();
        let client_interval = opts
            .client_interval_us
            .map(|i| std::time::Duration::from_micros(i));

        let handle = tokio::spawn(async move {
            println!("Spawned task {}", i);
            loop {
                client_loop(
                    &httpc_cloned,
                    &devices_locked_cloned,
                    &devices_notify_cloned,
                    &shard_url_cloned,
                    &arc_stat_send_cloned,
                    client_interval,
                )
                .await;
            }
        });

        handles.push(Some(handle));
    }

    let mut stats_file = if let Some(path) = opts.stats_file {
        Some(tokio::fs::File::create(path).await.unwrap())
    } else {
        None
    };

    loop {
        use tokio::io::AsyncWriteExt;
        let entry = stat_recv.recv().await.unwrap();
        if let Some(ref mut f) = stats_file {
            // TODO: bufwriter?
            f.write_all(serde_json::to_string(&entry).unwrap().as_bytes())
                .await
                .unwrap();
            f.write_u8(b'\n').await.unwrap();
        }
    }
}

pub async fn get_device(
    queue: &tokio::sync::Mutex<LinkedList<String>>,
    notify: &tokio::sync::Notify,
) -> String {
    let mut lock_guard = queue.lock().await;

    while lock_guard.len() == 0 {
        let notified = notify.notified();
        std::mem::drop(lock_guard);
        notified.await;
        lock_guard = queue.lock().await;
    }

    // Take a device out of the list and drop the lock:
    let device_id = lock_guard.pop_front().unwrap();
    std::mem::drop(lock_guard);
    device_id
}

async fn client_loop(
    httpc: &reqwest::Client,
    queue: &tokio::sync::Mutex<LinkedList<String>>,
    notify: &tokio::sync::Notify,
    shard_url: &str,
    stat_tx: &tokio::sync::mpsc::UnboundedSender<StatEntry>,
    client_interval: Option<std::time::Duration>,
) {
    let device_id = get_device(queue, notify).await;
    let start = std::time::Instant::now();

    let messages_resp = httpc
        .delete(&format!("{}/inbox-bin", shard_url))
        .header("Authorization", &format!("Bearer {}", device_id))
        .send()
        .await
        .unwrap();

    let parsed: noise_server_lib::shard::client_protocol::MessageBatch =
        bincode::deserialize(&messages_resp.bytes().await.unwrap()).unwrap();

    let duration = std::time::Instant::now().duration_since(start);

    stat_tx
        .send(StatEntry {
            duration: duration.as_millis(),
            device_id: device_id.clone(),
            start_epoch: parsed.start_epoch_id,
            end_epoch: parsed.end_epoch_id,
            message_count: parsed
                .messages
                .iter()
                .map(|(_, m)| m.len())
                .sum::<usize>(),
        })
        .unwrap();

    if let Some(interval) = client_interval {
        tokio::time::sleep(interval.saturating_sub(duration)).await;
    }

    let mut lock_guard = queue.lock().await;
    lock_guard.push_back(device_id);
    std::mem::drop(lock_guard);
}
