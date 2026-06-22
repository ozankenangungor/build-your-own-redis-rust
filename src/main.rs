mod commands;
mod config;
mod connection;
mod resp;
mod store;

use anyhow::Result;
use config::Config;
use std::sync::Arc;
use store::Store;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Arc::new(Config::from_args()?);

    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    let store = Store::default();

    // Port zero leaves the choice to the operating system, so report the port
    // that was settled on rather than the one that was asked for.
    eprintln!("listening on port {}", listener.local_addr()?.port());

    loop {
        // One failed accept, say because the process is out of file
        // descriptors, is no reason to take the whole server down with it.
        let (stream, addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                eprintln!("failed to accept a connection: {e}");
                continue;
            }
        };
        eprintln!("accepted a connection from {addr}");

        // Each connection gets its own task, so a slow client cannot keep the
        // server from accepting the next one. Cloning shares the one store and
        // the one set of settings rather than copying them.
        let store = store.clone();
        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = connection::serve(stream, addr, store, &config).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}
