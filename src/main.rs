mod commands;
mod config;
mod connection;
mod replica;
mod replicas;
mod resp;
mod server;
mod store;

use anyhow::Result;
use config::Config;
use server::Server;
use std::sync::Arc;
use store::Store;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let server = Arc::new(Server::new(Config::from_args()?));

    let listener = TcpListener::bind(("127.0.0.1", server.config.port)).await?;
    let store = Store::default();

    // Port zero leaves the choice to the operating system, so the port that was
    // settled on is the one to report, and the one to tell a master about.
    let port = listener.local_addr()?.port();
    eprintln!("listening on port {port}");

    // Following a master is its own conversation, held alongside the one with
    // this server's own clients rather than before it.
    if let Some(master) = server.config.replicaof.clone() {
        tokio::spawn(async move {
            if let Err(e) = replica::follow(&master, port).await {
                eprintln!(
                    "could not follow the master at {}:{}: {e}",
                    master.host, master.port
                );
            }
        });
    }

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
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            if let Err(e) = connection::serve(stream, addr, store, &server).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}
