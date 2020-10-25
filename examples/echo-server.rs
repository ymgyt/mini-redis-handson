#![allow(dead_code)]
use tokio::io;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6142").await.unwrap();

    loop {
        let (socket, _) = listener.accept().await?;

        // echo_io_copy(socket).await;
        echo_manual_copy(socket).await;
    }
}

async fn echo_manual_copy(mut socket: tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::spawn(async move {
        let mut buf = vec![0; 1024];

        loop {
            match socket.read(&mut buf).await {
                // return value of Ok(0) signified that the remote has closed.
                Ok(0) => return,
                Ok(n) => {
                    if socket.write_all(&buf[..n]).await.is_err() {
                        eprintln!("write error");
                        return;
                    }
                }
                Err(_) => {
                    return;
                }
            }
        }
    });
}

async fn echo_io_copy(mut socket: tokio::net::TcpStream) {
    tokio::spawn(async move {
        let (mut rd, mut wr) = socket.split();

        if io::copy(&mut rd, &mut wr).await.is_err() {
            eprintln!("failed to copy");
        }
    });
}
