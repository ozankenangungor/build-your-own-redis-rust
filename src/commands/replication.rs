use super::wrong_arity;
use crate::resp::Value;
use crate::server::Server;
use bytes::Bytes;

/// A dataset holding nothing, in the format Redis saves its data in.
///
/// A file is a header naming the format's version, then the data, then a marker
/// for the end and a checksum of everything before it. With no data to write,
/// only the header and the marker are left, and a checksum of zero, which is
/// how the format says that nothing was checked.
const EMPTY_DATASET: &[u8] = b"REDIS0011\xff\0\0\0\0\0\0\0\0";

/// Handles the commands replicas send their master. `None` means the command
/// belongs to another module.
pub fn run(command: &str, args: &[Bytes], server: &Server) -> Option<Value> {
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
                Value::Sequence(vec![
                    Value::SimpleString(format!(
                        "FULLRESYNC {} {}",
                        replication.id, replication.offset
                    )),
                    Value::File(Bytes::from_static(EMPTY_DATASET)),
                ])
            }
            _ => wrong_arity("psync"),
        },
        _ => return None,
    };

    Some(reply)
}
