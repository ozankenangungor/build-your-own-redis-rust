use crate::store::Store;
use crate::{commands, resp};
use anyhow::Result;
use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Reads commands from one client until it goes away, replying to each.
pub async fn serve(mut stream: TcpStream, store: Store) -> Result<()> {
    let mut buf = BytesMut::with_capacity(1024);

    loop {
        if stream.read_buf(&mut buf).await? == 0 {
            println!("connection closed by client");
            return Ok(());
        }

        // A read may carry several commands at once, or only part of one.
        while let Some((command, consumed)) = resp::parse(&buf)? {
            buf.advance(consumed);

            let reply = commands::run(command, &store).await;
            stream.write_all(reply.encode().as_bytes()).await?;
        }
    }
}
