use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// Handles the commands that make up a transaction.
/// `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes]) -> Option<Value> {
    let reply = match command {
        "MULTI" => match args {
            [] => Value::SimpleString("OK".into()),
            _ => wrong_arity("multi"),
        },
        "EXEC" => match args {
            [] => Value::Error("ERR EXEC without MULTI".into()),
            _ => wrong_arity("exec"),
        },
        _ => return None,
    };

    Some(reply)
}
