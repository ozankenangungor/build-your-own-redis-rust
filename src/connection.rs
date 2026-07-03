use crate::commands::{Answer, Transaction};
use crate::resp::Value;
use crate::server::Server;
use crate::store::Store;
use crate::{commands, resp};
use anyhow::Result;
use bytes::{Buf, Bytes, BytesMut};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
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
                    let (changes, reached) = server.replicas.add();

                    return keep_up_to_date(stream, changes, reached, server).await;
                }
            }
        }
    }
}

/// Passes on everything the master is told to change, for as long as the
/// replica is there to hear it.
///
/// A replica says only one thing back, and only when asked: how far along the
/// stream it has got. Listening for it is also how a replica that has gone
/// away is found out.
async fn keep_up_to_date(
    mut stream: TcpStream,
    mut changes: mpsc::UnboundedReceiver<Bytes>,
    reached: Arc<AtomicU64>,
    server: &Server,
) -> Result<()> {
    let mut heard = BytesMut::with_capacity(64);

    loop {
        tokio::select! {
            change = changes.recv() => {
                let Some(change) = change else {
                    return Ok(());
                };

                stream.write_all(&change).await?;
            }
            read = stream.read_buf(&mut heard) => {
                if read? == 0 {
                    return Ok(());
                }

                while let Some((said, consumed)) = resp::parse(&heard)? {
                    heard.advance(consumed);

                    if let Some(offset) = how_far(&said) {
                        server.replicas.reached(&reached, offset);
                    }
                }
            }
        }
    }
}

/// Reads how far a replica says it has got, out of what it said back.
fn how_far(said: &Value) -> Option<u64> {
    let Value::Array(parts) = said else {
        return None;
    };
    let [
        Value::BulkString(name),
        Value::BulkString(ack),
        Value::BulkString(offset),
    ] = parts.as_slice()
    else {
        return None;
    };

    if !name.eq_ignore_ascii_case(b"REPLCONF") || !ack.eq_ignore_ascii_case(b"ACK") {
        return None;
    }

    str::from_utf8(offset).ok()?.parse().ok()
}
