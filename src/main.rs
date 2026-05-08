mod commands;
mod connection;
mod resp;
mod store;

use anyhow::Result;
use store::Store;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    let store = Store::default();

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");

        // Each connection gets its own task, so a slow client cannot keep the
        // server from accepting the next one. Cloning the store shares it.
        let store = store.clone();
        tokio::spawn(async move {
            if let Err(e) = connection::serve(stream, store).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}
