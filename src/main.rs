mod aof;
mod commands;
mod config;
mod connection;
mod glob;
mod rdb;
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

    // Whatever was saved last time is where this server picks up, and it picks
    // up before it takes a caller. A file that cannot be read is not treated as
    // one that was never saved: coming up empty over a dataset that is there
    // would look for all the world like the data had gone.
    let store = Store::default();
    let loaded = rdb::load(&server.config, &store)?;
    eprintln!("loaded {loaded} keys from the saved dataset");

    // A server that records what it does needs somewhere to record it, and
    // needs it before the first client, not before the first write.
    if let Some(dir) = aof::prepare(&server.config)? {
        eprintln!("recording writes under {}", dir.display());
    }

    let listener = TcpListener::bind(("127.0.0.1", server.config.port)).await?;

    // Port zero leaves the choice to the operating system, so the port that was
    // settled on is the one to report, and the one to tell a master about.
    let port = listener.local_addr()?.port();
    eprintln!("listening on port {port}");

    // Following a master is its own conversation, held alongside the one with
    // this server's own clients rather than before it.
    if let Some(master) = server.config.replicaof.clone() {
        let following = Arc::clone(&server);
        let store = store.clone();

        tokio::spawn(async move {
            if let Err(e) = replica::follow(&master, port, store, &following).await {
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
