use super::wrong_arity;
use crate::glob::Pattern;
use crate::resp::Value;
use crate::store::{Kind, Store};
use bytes::Bytes;

/// Handles the commands that work on a key whatever it holds. `None` means the
/// command belongs to another module.
pub fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
    let reply = match command {
        "TYPE" => match args {
            [key] => Value::SimpleString(type_name(store.kind(key)).into()),
            _ => wrong_arity("type"),
        },
        // Every key the pattern asks for, in no order in particular: Redis
        // walks its table, and what that turns up is nobody's to rely on.
        "KEYS" => match args {
            [pattern] => {
                let pattern = Pattern::parse(pattern);

                Value::Array(
                    store
                        .keys(|key| pattern.matches(key))
                        .into_iter()
                        .map(Value::BulkString)
                        .collect(),
                )
            }
            _ => wrong_arity("keys"),
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
