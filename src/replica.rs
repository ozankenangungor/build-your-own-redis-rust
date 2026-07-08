use crate::commands::{self, Transaction};
use crate::config::Master;
use crate::resp::{self, Value};
use crate::server::Server;
use crate::store::Store;
use anyhow::{Result, bail, ensure};
use bytes::{Buf, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Follows the master this server was told to follow, for as long as it is
/// there to be followed.
///
/// The two first agree on how to go on, in three steps: a greeting, then what
/// the replica is and can do, then a request for the master's history. What
/// comes back is the dataset to start from, and after it every command the
/// master is given.
///
/// The port is the one this server ended up listening on, which is not always
/// the one it was asked for, and is what the master will reach it on.
pub async fn follow(master: &Master, port: u16, store: Store, server: &Server) -> Result<()> {
    let mut conversation = Conversation::connect(master).await?;

    conversation.say(&["PING"], "PONG").await?;
    // Where to find this replica, which the master keeps for reporting rather
    // than for replicating.
    conversation
        .say(&["REPLCONF", "listening-port", &port.to_string()], "OK")
        .await?;
    // What this replica can do. Claiming `psync2` says it can pick a history
    // back up where it left off rather than asking for all of it again.
    conversation
        .say(&["REPLCONF", "capa", "psync2"], "OK")
        .await?;

    // Having followed no one before, the replica knows neither whose history to
    // ask for nor where in it to start: `?` and `-1` say so, and ask for all
    // of it.
    let reply = conversation.ask(&["PSYNC", "?", "-1"]).await?;
    let Value::SimpleString(agreement) = &reply else {
        bail!("asked to sync and heard {reply:?}");
    };
    // The master names the history it is handing over and how far along that
    // history the handover is. What comes next carries on from there, so that
    // is where this replica starts counting.
    let handover = agreement
        .strip_prefix("FULLRESYNC ")
        .and_then(|handover| handover.split_once(' '));
    let Some((_history, from)) = handover else {
        bail!("asked to sync and was told '{agreement}'");
    };
    let from: u64 = from.parse()?;

    // What follows the agreement is the master's whole dataset. It is always
    // empty for now, so there is nothing in it to take on.
    let dataset = conversation.take_file().await?;

    eprintln!(
        "following the master at {}:{}, from {} bytes of dataset",
        master.host,
        master.port,
        dataset.len(),
    );

    conversation.keep_up(from, store, server).await
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
        let reply = self.ask(command).await?;
        ensure!(
            reply == Value::SimpleString(expected.to_string()),
            "said {command:?} and heard {reply:?} rather than '{expected}'",
        );

        Ok(())
    }

    /// Says one command and hands back the answer, for when what the master
    /// will say is not known in advance.
    async fn ask(&mut self, command: &[&str]) -> Result<Value> {
        self.stream.write_all(&as_command(command).encode()).await?;

        Ok(self.hear().await?.0)
    }

    /// Reads one value, reading more from the master until a whole one arrives,
    /// along with the number of bytes it took up.
    ///
    /// The length is what a replica counts to say how far it has got, so it is
    /// measured here rather than worked out again from the value.
    async fn hear(&mut self) -> Result<(Value, u64)> {
        loop {
            if let Some((reply, consumed)) = resp::parse(&self.heard)? {
                self.heard.advance(consumed);
                return Ok((reply, consumed as u64));
            }

            self.fill().await?;
        }
    }

    /// Takes in the file the master sends once it has agreed to sync.
    ///
    /// It is laid out like a bulk string but for the CRLF it does not end in,
    /// so where it ends has to be worked out from the length rather than found.
    async fn take_file(&mut self) -> Result<Bytes> {
        let (length, header) = loop {
            if let Some(found) = file_header(&self.heard)? {
                break found;
            }

            self.fill().await?;
        };

        while self.heard.len() < header + length {
            self.fill().await?;
        }

        self.heard.advance(header);

        Ok(self.heard.split_to(length).freeze())
    }

    /// Takes in what the master is told to change, for as long as it keeps
    /// telling.
    ///
    /// Only one thing is ever said back. The master is not waiting on replies,
    /// and one sent unasked would be read as something else entirely.
    async fn keep_up(&mut self, from: u64, store: Store, server: &Server) -> Result<()> {
        // A master could open a transaction as any client could, so the
        // connection keeps one the way every other connection does.
        let mut transaction = Transaction::default();
        // How far along the master's history this replica is. It starts where
        // the handover left off, so that what it reports and what the master
        // counts are the same numbers.
        let mut offset = from;

        loop {
            let (command, length) = self.hear().await?;

            // Asked how far it has got, a replica says so. This is the one
            // command it answers rather than carries out.
            if asks_how_far(&command) {
                self.stream
                    .write_all(&as_command(&["REPLCONF", "ACK", &offset.to_string()]).encode())
                    .await?;
            } else {
                commands::run(command, &store, &mut transaction, server).await;
            }

            // Counted after the fact, so that what a replica reports is how far
            // it had got when the asking reached it.
            offset += length;
        }
    }

    /// Reads more from the master, which has nothing more to say only when it
    /// has gone.
    async fn fill(&mut self) -> Result<()> {
        if self.stream.read_buf(&mut self.heard).await? == 0 {
            bail!("the master hung up");
        }

        Ok(())
    }
}

/// Reads the length off the front of a file, along with how many bytes the
/// length itself took up. `None` means the length is not all there yet.
fn file_header(heard: &[u8]) -> Result<Option<(usize, usize)>> {
    let Some(end) = heard.windows(2).position(|pair| pair == b"\r\n") else {
        return Ok(None);
    };

    let Some(length) = heard[..end].strip_prefix(b"$") else {
        bail!(
            "expected a file, found {:?}",
            String::from_utf8_lossy(&heard[..end])
        );
    };

    Ok(Some((str::from_utf8(length)?.parse()?, end + 2)))
}

/// Whether the master is asking the replica how much of the stream it has
/// taken in, rather than telling it something.
fn asks_how_far(command: &Value) -> bool {
    let Value::Array(parts) = command else {
        return false;
    };
    let [Value::BulkString(name), Value::BulkString(option), ..] = parts.as_slice() else {
        return false;
    };

    name.eq_ignore_ascii_case(b"REPLCONF") && option.eq_ignore_ascii_case(b"GETACK")
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
    fn knows_when_it_is_being_asked_how_far_it_has_got() {
        assert!(asks_how_far(&as_command(&["REPLCONF", "GETACK", "*"])));
        assert!(asks_how_far(&as_command(&["replconf", "getack", "*"])));
    }

    #[test]
    fn knows_the_rest_is_to_be_carried_out_rather_than_answered() {
        for command in [
            vec!["REPLCONF", "listening-port", "6380"],
            vec!["SET", "key", "value"],
            vec!["PING"],
            vec!["REPLCONF"],
        ] {
            assert!(!asks_how_far(&as_command(&command)), "{command:?}");
        }
    }

    #[test]
    fn reads_the_length_off_the_front_of_a_file() {
        assert_eq!(file_header(b"$18\r\nREDIS").unwrap(), Some((18, 5)));
    }

    #[test]
    fn waits_for_the_whole_of_a_file_length() {
        assert_eq!(file_header(b"$1").unwrap(), None);
        assert_eq!(file_header(b"$18\r").unwrap(), None);
    }

    #[test]
    fn turns_down_what_is_not_a_file_at_all() {
        assert!(file_header(b"+OK\r\n").is_err());
    }

    #[test]
    fn lays_out_a_command_with_arguments() {
        assert_eq!(
            as_command(&["REPLCONF", "capa", "psync2"]).encode(),
            b"*3\r\n$8\r\nREPLCONF\r\n$4\r\ncapa\r\n$6\r\npsync2\r\n",
        );
    }
}
