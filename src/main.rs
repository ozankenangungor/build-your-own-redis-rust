mod aof;
mod channels;
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
mod users;

use anyhow::Result;
use commands::{Answer, Identity, Subscriptions, Transaction};
use config::Config;
use resp::Value;
use server::Server;
use std::sync::Arc;
use store::Store;
use tokio::net::TcpListener;

/// Does again what the recorded commands did, and says how many were done.
///
/// They are run as though a client had just sent them, which is the whole point
/// of keeping them word for word: whatever a command meant when it arrived, it
/// means the same here.
///
/// A command that comes back with an error is said aloud and passed over. It is
/// worth knowing about, since a record is written from commands that worked, but
/// stopping over one would cost every write recorded after it.
async fn replay(recorded: Vec<Value>, store: &Store, server: &Server) -> usize {
    // The transaction stands in for the connection a client would have had.
    // Nothing recorded ever opens one, since a transaction is written down as
    // the commands it ran rather than as itself.
    let mut transaction = Transaction::default();
    let mut subscriptions = Subscriptions::default();
    let mut identity = Identity::trusted();
    let mut replayed = 0;

    for command in recorded {
        match commands::run(
            command,
            store,
            &mut transaction,
            &mut subscriptions,
            &mut identity,
            server,
        )
        .await
        {
            Answer::Reply(Value::Error(said)) => {
                eprintln!("a recorded command was refused: {said}");
            }
            _ => replayed += 1,
        }
    }

    replayed
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_args()?;

    // Whatever was saved last time is where this server picks up, and it picks
    // up before it takes a caller. A file that cannot be read is not treated as
    // one that was never saved: coming up empty over a dataset that is there
    // would look for all the world like the data had gone.
    let store = Store::default();
    let loaded = rdb::load(&config, &store)?;
    eprintln!("loaded {loaded} keys from the saved dataset");

    // A server that records what it does needs somewhere to record it, and
    // needs it before the first client, not before the first write.
    let aof = aof::prepare(&config)?;
    let server = Arc::new(Server::new(config));

    // What was recorded last time is done again, in the order it was done in,
    // which leaves the store where the last run left it. The record is only
    // taken up afterwards: a command played back out of the file has no
    // business being written straight back into it.
    if let Some(aof) = aof {
        let replayed = replay(aof.recorded()?, &store, &server).await;
        eprintln!("replayed {replayed} recorded commands");

        eprintln!("recording writes in {}", aof.path().display());
        let _ = server.aof.set(aof);
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
