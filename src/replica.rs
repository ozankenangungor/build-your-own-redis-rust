use crate::config::Master;
use crate::resp::Value;
use anyhow::Result;
use bytes::Bytes;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Introduces this server to the master it was told to follow.
///
/// The two agree on how to go on in three steps: this greeting, then what the
/// replica can do, then a request for the master's history. Only the first is
/// spoken here so far.
pub async fn follow(master: &Master) -> Result<()> {
    let mut stream = TcpStream::connect((master.host.as_str(), master.port)).await?;

    // A master is spoken to the way any client speaks to one, in commands.
    stream.write_all(&command(&["PING"]).encode()).await?;

    Ok(())
}

/// Lays a command out the way clients send them, as an array of bulk strings.
fn command(parts: &[&str]) -> Value {
    Value::Array(
        parts
            .iter()
            .map(|part| Value::BulkString(Bytes::copy_from_slice(part.as_bytes())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_a_command_as_clients_send_them() {
        assert_eq!(command(&["PING"]).encode(), b"*1\r\n$4\r\nPING\r\n");
    }
}
