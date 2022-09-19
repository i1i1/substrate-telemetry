use std::sync::Arc;

use fdlimit::raise_fd_limit;
use futures::stream::{FuturesUnordered, TryStreamExt};
use test_utils::server::{CoreProcess, ShardProcess};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    raise_fd_limit().unwrap();
    let host = std::env::var("HOST").expect("Set HOST variable first");

    let ntasks = 1000;
    let per_task = 10;

    let start = std::time::Instant::now();

    let shards = (0..ntasks)
        .map(|i| {
            let x = ShardProcess::from_host(format!("{host}:8001"));
            async move {
                let res = x.connect_multiple_nodes(per_task).await.map(|x| {
                    x.into_iter()
                        .map(Mutex::new)
                        .map(Arc::new)
                        .collect::<Vec<_>>()
                });
                eprintln!("{i}");
                res
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let _feeds = (0..500)
        .map(|i| {
            let x = CoreProcess::from_host(format!("{host}:8000"));
            async move {
                let res = x.connect_multiple_feeds(per_task).await;
                eprintln!("Feed {i}");
                res
            }
        })
        .collect::<FuturesUnordered<_>>()
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

    for _ in 0..100 {
        eprintln!("Sending");
        let chunk_size = 100;
        shards
            .chunks(chunk_size)
            .enumerate()
            .map(|(i, chunk)| {
                let shards = chunk.iter().cloned().collect::<Vec<_>>();
                async move {
                    for (j, shard) in shards.into_iter().enumerate() {
                        let n = chunk_size * i + j;
                        shard.lock().await.0.send_json_text(serde_json::json!({
                            "id": 1, // message ID, not node ID. Can be the same for all.
                            "ts":"2021-07-12T10:37:47.714666+01:00",
                            "payload": {
                                "authority":true,
                                "chain":"Local Testnet",
                                "config":"",
                                "genesis_hash": "0x91b171bb158e2d3848fa23a9f1c25182fb8e20313b2c1eb49219da7a70ce90c3",
                                "implementation":"Substrate Node",
                                "msg":"system.connected",
                                "name": format!("Alice {n}"),
                                "network_id":"12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp",
                                "startup_time":"1625565542717",
                                "version":"2.0.0-07a1af348-aarch64-macos"
                            }
                        })).unwrap();
                    }
                }
            })
            .map(tokio::spawn)
            .collect::<FuturesUnordered<_>>()
            .try_collect::<Vec<_>>()
            .await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    for _ in 0..1_000 {
        // 100secs
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
