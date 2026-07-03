use super::{not_an_integer, text, wrong_arity};
use crate::resp::Value;
use crate::server::Server;
use bytes::Bytes;
use std::time::{Duration, Instant};

/// A dataset holding nothing, in the format Redis saves its data in.
///
/// A file is a header naming the format's version, then the data, then a marker
/// for the end and a checksum of everything before it. With no data to write,
/// only the header and the marker are left, and a checksum of zero, which is
/// how the format says that nothing was checked.
const EMPTY_DATASET: &[u8] = b"REDIS0011\xff\0\0\0\0\0\0\0\0";

/// Handles the commands replicas send their master. `None` means the command
/// belongs to another module.
pub async fn run(command: &str, args: &[Bytes], server: &Server) -> Option<Value> {
    let reply = match command {
        // A replica introduces itself in pairs of a setting and its value. None
        // of them changes how this server answers yet, so all that is checked
        // is that they come in pairs.
        "REPLCONF" if args.len().is_multiple_of(2) => Value::SimpleString("OK".into()),
        "REPLCONF" => wrong_arity("replconf"),
        // A replica asks for a history and a place in it. This server keeps no
        // record of what it has already handed out, so it can only ever start
        // a replica afresh, whatever it asks for.
        "PSYNC" => match args {
            [_history, _offset] => {
                let replication = &server.replication;

                // Agreeing to start it afresh is only half the answer: what
                // the replica is to start from follows straight after.
                //
                // What goes out is an empty dataset rather than this server's
                // own, so a replica arriving after the store already holds
                // something starts out missing it, and only ever hears about
                // the changes that follow. Writing the store out as a file is
                // what would close that.
                Value::Sequence(vec![
                    Value::SimpleString(format!(
                        "FULLRESYNC {} {}",
                        replication.id,
                        replication.offset()
                    )),
                    Value::File(Bytes::from_static(EMPTY_DATASET)),
                ])
            }
            _ => wrong_arity("psync"),
        },
        // How many replicas have caught up with everything the master has been
        // told, waiting a while for them to say so.
        "WAIT" => match args {
            [replicas, timeout] => match (number(replicas), number(timeout)) {
                (Some(_), Some(timeout)) if timeout < 0 => negative_timeout(),
                (Some(wanted), Some(timeout)) => {
                    let caught_up = wait(server, wanted, timeout as u64).await;

                    Value::Integer(caught_up as i64)
                }
                _ => not_an_integer(),
            },
            _ => wrong_arity("wait"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Waits for `wanted` replicas to have taken in everything this master has
/// passed on, giving up after `timeout` milliseconds.
///
/// What is waited for is settled first: replicas that catch up with commands
/// sent after the waiting began are neither here nor there.
async fn wait(server: &Server, wanted: i64, timeout: u64) -> usize {
    let target = server.replication.offset();

    // With nothing yet handed out there is nothing to catch up on, so every
    // replica there is has caught up by definition.
    if target == 0 {
        return server.replicas.count();
    }

    // Replicas say how far they have got only when asked, and a replica with
    // nothing to do would otherwise never say.
    let asked = server
        .replicas
        .send(&as_command(&["REPLCONF", "GETACK", "*"]));
    server.replication.advance(asked);

    let deadline = Instant::now() + Duration::from_millis(timeout);

    loop {
        // Made before the count is taken, so that word arriving in between is
        // waited on rather than missed.
        let stirred = server.replicas.stirred();

        let caught_up = server.replicas.caught_up_to(target);
        if caught_up as i64 >= wanted {
            return caught_up;
        }

        let left = deadline.saturating_duration_since(Instant::now());
        if tokio::time::timeout(left, stirred).await.is_err() {
            return server.replicas.caught_up_to(target);
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

/// Reads an argument as a number, the way Redis reads the ones that count
/// things and measure time.
fn number(argument: &[u8]) -> Option<i64> {
    text(argument)?.parse().ok()
}

fn negative_timeout() -> Value {
    Value::Error("ERR timeout is negative".into())
}
