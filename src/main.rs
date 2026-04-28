use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");

        // Each connection gets its own task, so a slow client cannot keep the
        // server from accepting the next one.
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let mut buf = [0u8; 512];

    loop {
        // The command is ignored for now; every request gets a hardcoded PONG.
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            println!("connection closed by client");
            return Ok(());
        }

        stream.write_all(b"+PONG\r\n").await?;
    }
}
