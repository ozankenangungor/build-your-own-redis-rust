use crate::commands::{Answer, Transaction};
use crate::resp::Value;
use crate::server::Server;
use crate::store::Store;
use crate::{commands, resp};
use anyhow::Result;
use bytes::{Buf, Bytes, BytesMut};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Reads commands from one client until it goes away, replying to each.
pub async fn serve(
    mut stream: TcpStream,
    addr: SocketAddr,
    store: Store,
    server: &Server,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(1024);
    // The transaction lives as long as this connection and no longer, so a
    // client that hangs up mid-transaction leaves nothing behind.
    let mut transaction = Transaction::default();

    loop {
        if stream.read_buf(&mut buf).await? == 0 {
            eprintln!("{addr} closed the connection");
            return Ok(());
        }

        // A read may carry several commands at once, or only part of one.
        loop {
            let answer = match resp::parse(&buf) {
                Ok(None) => break,
                Ok(Some((command, consumed))) => {
                    buf.advance(consumed);
                    commands::run(command, &store, &mut transaction, server).await
                }
                // Malformed input leaves the stream out of step, with no way to
                // tell where the next command starts, so say so and hang up.
                Err(error) => {
                    let reply = Value::Error(format!("ERR Protocol error: {error}"));
                    stream.write_all(&reply.encode()).await?;
                    return Ok(());
                }
            };

            match answer {
                Answer::Reply(reply) => stream.write_all(&reply.encode()).await?,
                Answer::Replica(reply) => {
                    stream.write_all(&reply.encode()).await?;
                    eprintln!("{addr} is now a replica");

                    // Taking on the replica before returning means no change
                    // can slip through between the dataset going out and the
                    // connection starting to carry them.
                    return keep_up_to_date(stream, server.replicas.add()).await;
                }
            }
        }
    }
}

/// Passes on everything the master is told to change, for as long as the
/// replica is there to hear it.
///
/// A replica says nothing back, so this only ever writes.
async fn keep_up_to_date(
    mut stream: TcpStream,
    mut changes: mpsc::UnboundedReceiver<Bytes>,
) -> Result<()> {
    while let Some(change) = changes.recv().await {
        stream.write_all(&change).await?;
    }

    Ok(())
}
