use crate::commands::Transaction;
use crate::config::Config;
use crate::resp::Value;
use crate::store::Store;
use crate::{commands, resp};
use anyhow::Result;
use bytes::{Buf, BytesMut};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Reads commands from one client until it goes away, replying to each.
pub async fn serve(
    mut stream: TcpStream,
    addr: SocketAddr,
    store: Store,
    config: &Config,
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
            let reply = match resp::parse(&buf) {
                Ok(None) => break,
                Ok(Some((command, consumed))) => {
                    buf.advance(consumed);
                    commands::run(command, &store, &mut transaction, config).await
                }
                // Malformed input leaves the stream out of step, with no way to
                // tell where the next command starts, so say so and hang up.
                Err(error) => {
                    let reply = Value::Error(format!("ERR Protocol error: {error}"));
                    stream.write_all(&reply.encode()).await?;
                    return Ok(());
                }
            };

            stream.write_all(&reply.encode()).await?;
        }
    }
}
