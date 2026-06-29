use super::wrong_arity;
use crate::resp::Value;
use crate::server::Server;
use bytes::Bytes;

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

                Value::SimpleString(format!(
                    "FULLRESYNC {} {}",
                    replication.id, replication.offset
                ))
            }
            _ => wrong_arity("psync"),
        },
        _ => return None,
    };

    Some(reply)
}
