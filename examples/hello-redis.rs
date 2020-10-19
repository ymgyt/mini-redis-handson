use bytes::Bytes;
use mini_redis::client;
use tokio::sync::{mpsc, oneshot};

/// Multiple different commands are multiplexed over a single channel.
#[derive(Debug)]
enum Command {
    Get {
        key: String,
        resp: Responder<Option<Bytes>>,
    },
    Set {
        key: String,
        val: Vec<u8>,
        resp: Responder<()>,
    }
}

type Responder<T> = oneshot::Sender<mini_redis::Result<T>>;

#[tokio::main]
pub async fn main() {
    let (mut tx, mut rx) = mpsc::channel(32);

    let mut tx2 = tx.clone();

    let manager = tokio::spawn(async move {
        let mut client = client::connect("127.0.0.1:6379").await.unwrap();

        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::Get { key, resp } => {
                    let res = client.get(&key).await;

                    // Ignore errors
                    let _ = resp.send(res);
                }
                Command::Set { key, val,  resp } => {
                    let res = client.set(&key, val.into()).await;

                    // Ignore errors
                    let _ = resp.send(res);
                }
            }
        }
    });

    let t1 = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();
        let cmd = Command::Get {
            key: "hello".to_string(),
            resp: resp_tx,
        };

        if tx.send(cmd).await.is_err() {
           eprintln!("connection task shutdown");
            return;
        }

        let res = resp_rx.await;
        println!("GOT = {:?}", res);
    });

    let t2 = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();
        let cmd = Command::Set {
            key: "hello".to_owned(),
            val: b"bar".to_vec(),
            resp: resp_tx,
        };

        if tx2.send(cmd).await.is_err() {
            eprintln!("connection task shutdown");
            return;
        }

        let res = resp_rx.await;
        println!("GOT = {:?}", res)
    });

    t1.await.unwrap();
    t2.await.unwrap();
    manager.await.unwrap();
}
