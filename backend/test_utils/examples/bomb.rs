use std::sync::Arc;

use chrono::prelude::*;
use clap::Parser;
use fdlimit::raise_fd_limit;
use futures::stream::{FuturesUnordered, TryStreamExt};
use test_utils::server::{channels, CoreProcess, ShardProcess};
use tokio::sync::Mutex;
use zzz::prelude::*;

#[derive(clap::Parser, Debug)]
struct Args {
    /// Number of shards (nodes which will submit some info)
    #[clap(short, long, value_parser)]
    shards: usize,
    // Number of feeds (subscribers like browser tabs)
    #[clap(short, long, value_parser)]
    feeds: usize,
    // Number of connections to open, per task
    #[clap(short, long, value_parser, default_value_t = 10)]
    connections_per_task: usize,

    /// Telemetry host
    #[clap(value_parser)]
    host: String,
}

async fn send_msg(
    shards: &[Arc<Mutex<(channels::ShardSender, channels::ShardReceiver)>>],
    mut get_msg: impl FnMut(usize) -> serde_json::Value + Send + Copy + 'static,
) {
    let chunk_size = 100;
    shards
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            let shards = chunk.iter().cloned().collect::<Vec<_>>();
            async move {
                for (j, shard) in shards.into_iter().enumerate() {
                    let n = chunk_size * i + j;
                    shard.lock().await.0.send_json_text(get_msg(n)).unwrap();
                }
            }
        })
        .map(tokio::spawn)
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>()
        .await
        .unwrap();
}

#[tokio::main]
async fn main() {
    let Args {
        shards,
        feeds,
        connections_per_task,
        host,
    } = Args::parse();

    raise_fd_limit().unwrap();

    let start = std::time::Instant::now();

    let shards = (0..shards / connections_per_task)
        .map(|_| {
            let x = ShardProcess::from_host(format!("{host}:8001"));
            async move {
                x.connect_multiple_nodes(connections_per_task)
                    .await
                    .map(|x| {
                        x.into_iter()
                            .map(Mutex::new)
                            .map(Arc::new)
                            .collect::<Vec<_>>()
                    })
            }
        })
        .collect::<FuturesUnordered<_>>()
        .progress()
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let _feeds = (0..feeds / connections_per_task)
        .map(|_| {
            let x = CoreProcess::from_host(format!("{host}:8000"));
            async move { x.connect_multiple_feeds(connections_per_task).await }
        })
        .collect::<FuturesUnordered<_>>()
        .progress()
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .into_iter()
        .map(|mut feeds| async move {
            for (f, _) in &mut feeds {
                f.send_command("subscribe", "Local Testnet").unwrap();
            }
            loop {
                for (_, f) in &mut feeds {
                    f.recv_feed_messages_once().await.unwrap();
                }
            }
        })
        .map(tokio::spawn);

    eprintln!("Sending. Initing took {:?}", start.elapsed());

    // loop {
    eprintln!("Sending new nodes");
    send_msg(&shards, |n| {
        serde_json::json!({
            "id": 1,
            "ts": Utc::now().to_rfc3339(),
            "payload": {
                "authority": true,
                "chain": "Local Testnet",
                "config": "",
                "genesis_hash": "0x91b171bb158e2d3848fa23a9f1c25182fb8e20313b2c1eb49219da7a70ce90c3",
                "implementation": "Substrate Node",
                "msg": "system.connected",
                "name": format!("Alice {n}"),
                "network_id": "12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp",
                "startup_time": "1625565542717",
                "version": "2.0.0-07a1af348-aarch64-macos"
            }
        })
    }).await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    for i in 0.. {
        eprintln!("Sending update {i}");
        send_msg(&shards, move |_| {
            serde_json::json!({
                "id": 1,
                "ts": Utc::now().to_rfc3339(),
                "payload": {
                    "bandwidth_download": i,
                    "bandwidth_upload": 2 * i,
                    "msg": "system.interval",
                    "peers": 1,
                },
            })
        })
        .await;
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    }
}
