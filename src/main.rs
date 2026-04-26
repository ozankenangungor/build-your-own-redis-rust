use anyhow::Result;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (_stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");
    }
}
