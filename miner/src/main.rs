use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;
use chrono::Local;
use futures_util::{SinkExt, StreamExt};

// TODO: 
// [ ] - change the ping to come from the server
// The parent process is responsible for the health of it's children
// node -> miner
// node -> pool -> miner

#[tokio::main]
async fn main() {
    let url = Url::parse("ws://127.0.0.1:8080").unwrap();
    
    let base_interval = 5; // Normal ping interval (seconds)
    let max_retries = 5;   // Quit after max failures

    let mut retry_count = 0;

    loop {
        match connect_async(&url).await {
            Ok((mut ws_stream, _)) => {
                println!(
                    "{} ✅ Connected to WebSocket server",
                    Local::now().format("%Y-%m-%d %H:%M:%S")
                );

                retry_count = 0; // Reset retry count on successful connection

                loop {
                    // Send a ping message
                    if let Err(e) = ws_stream.send(Message::Ping(vec![])).await {
                        println!(
                            "{} ❌ Failed to send ping: {}",
                            Local::now().format("%Y-%m-%d %H:%M:%S"),
                            e
                        );
                        break; // Connection issue, break to reconnect
                    }

                    println!(
                        "{} 📡 Sent a ping message",
                        Local::now().format("%Y-%m-%d %H:%M:%S")
                    );

                    // Wait for Pong response
                    match time::timeout(Duration::from_secs(5), ws_stream.next()).await {
                        Ok(Some(Ok(Message::Pong(_)))) => {
                            println!(
                                "{} 🏓 Received a Pong response",
                                Local::now().format("%Y-%m-%d %H:%M:%S")
                            );
                        }
                        _ => {
                            println!(
                                "{} ⚠️ No Pong received within timeout",
                                Local::now().format("%Y-%m-%d %H:%M:%S")
                            );
                        }
                    }

                    // Wait for the normal ping interval
                    time::sleep(Duration::from_secs(base_interval)).await;
                }
            }
            Err(e) => {
                println!(
                    "{} ❌ Failed to connect: {}",
                    Local::now().format("%Y-%m-%d %H:%M:%S"),
                    e
                );

                if retry_count >= max_retries {
                    println!(
                        "{} 🚨 Maximum retries reached. Exiting.",
                        Local::now().format("%Y-%m-%d %H:%M:%S")
                    );
                    return;
                }

                let backoff_delay = base_interval * 2_u64.pow(retry_count);
                println!(
                    "{} ⏳ Retrying connection in {} seconds...",
                    Local::now().format("%Y-%m-%d %H:%M:%S"),
                    backoff_delay
                );

                retry_count += 1;
                time::sleep(Duration::from_secs(backoff_delay)).await;
            }
        }
    }
}
