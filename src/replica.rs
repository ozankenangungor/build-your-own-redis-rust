use crate::config::Master;
use crate::resp::{self, Value};
use anyhow::{Result, bail, ensure};
use bytes::{Buf, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Introduces this server to the master it was told to follow.
///
/// The two agree on how to go on in three steps: a greeting, then what the
/// replica is and can do, then a request for the master's history. Only the
/// last is still unspoken.
///
/// The port is the one this server ended up listening on, which is not always
/// the one it was asked for, and is what the master will reach it on.
pub async fn follow(master: &Master, port: u16) -> Result<()> {
    let mut master = Conversation::connect(master).await?;

    master.say(&["PING"], "PONG").await?;
    // Where to find this replica, which the master keeps for reporting rather
    // than for replicating.
    master
        .say(&["REPLCONF", "listening-port", &port.to_string()], "OK")
        .await?;
    // What this replica can do. Claiming `psync2` says it can pick a history
    // back up where it left off rather than asking for all of it again.
    master.say(&["REPLCONF", "capa", "psync2"], "OK").await?;

    Ok(())
}

/// One replica talking to one master, a command at a time.
struct Conversation {
    stream: TcpStream,
    /// What the master has said that has not been made sense of yet. It outlives
    /// each reply, since a read may carry part of the next one.
    heard: BytesMut,
}

impl Conversation {
    async fn connect(master: &Master) -> Result<Self> {
        let stream = TcpStream::connect((master.host.as_str(), master.port)).await?;

        Ok(Self {
            stream,
            heard: BytesMut::with_capacity(1024),
        })
    }

    /// Says one command and waits for the master to answer as it should. A
    /// master that answers otherwise is not one this replica can follow.
    async fn say(&mut self, command: &[&str], expected: &str) -> Result<()> {
        self.stream.write_all(&as_command(command).encode()).await?;

        let reply = self.hear().await?;
        ensure!(
            reply == Value::SimpleString(expected.to_string()),
            "said {command:?} and heard {reply:?} rather than '{expected}'",
        );

        Ok(())
    }

    /// Reads one reply, reading more from the master until a whole one arrives.
    async fn hear(&mut self) -> Result<Value> {
        loop {
            if let Some((reply, consumed)) = resp::parse(&self.heard)? {
                self.heard.advance(consumed);
                return Ok(reply);
            }

            if self.stream.read_buf(&mut self.heard).await? == 0 {
                bail!("the master hung up mid-sentence");
            }
        }
    }
}

/// Lays a command out the way clients send them, as an array of bulk strings.
fn as_command(parts: &[&str]) -> Value {
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
        assert_eq!(as_command(&["PING"]).encode(), b"*1\r\n$4\r\nPING\r\n");
    }

    #[test]
    fn lays_out_a_command_with_arguments() {
        assert_eq!(
            as_command(&["REPLCONF", "capa", "psync2"]).encode(),
            b"*3\r\n$8\r\nREPLCONF\r\n$4\r\ncapa\r\n$6\r\npsync2\r\n",
        );
    }
}
