use super::wrong_arity;
use crate::resp::Value;
use crate::store::{Kind, Store};

/// Handles the commands that work on a key whatever it holds. `None` means the
/// command belongs to another module.
pub fn run(command: &str, args: &[String], store: &Store) -> Option<Value> {
    let reply = match command {
        "TYPE" => match args {
            [key] => Value::SimpleString(type_name(store.kind(key)).into()),
            _ => wrong_arity("type"),
        },
        _ => return None,
    };

    Some(reply)
}

/// The names Redis reports for each kind of value.
fn type_name(kind: Kind) -> &'static str {
    match kind {
        Kind::None => "none",
        Kind::String => "string",
        Kind::List => "list",
        Kind::Stream => "stream",
    }
}
