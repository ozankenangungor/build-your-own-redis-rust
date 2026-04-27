use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");

        if let Err(e) = handle_connection(stream).await {
            eprintln!("connection error: {e}");
        }
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let mut buf = [0u8; 512];

    // The command is ignored for now; every request gets a hardcoded PONG.
    stream.read(&mut buf).await?;
    stream.write_all(b"+PONG\r\n").await?;

    Ok(())
}
