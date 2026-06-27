use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// Handles the commands replicas send their master. `None` means the command
/// belongs to another module.
pub fn run(command: &str, args: &[Bytes]) -> Option<Value> {
    let reply = match command {
        // A replica introduces itself in pairs of a setting and its value. None
        // of them changes how this server answers yet, so all that is checked
        // is that they come in pairs.
        "REPLCONF" if args.len().is_multiple_of(2) => Value::SimpleString("OK".into()),
        "REPLCONF" => wrong_arity("replconf"),
        _ => return None,
    };

    Some(reply)
}
