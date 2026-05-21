mod keys;
mod lists;
mod streams;
mod strings;

use crate::resp::Value;
use crate::store::Store;

/// Runs one client command and produces the reply to send back.
///
/// Each module below claims the commands it knows and returns `None` for the
/// rest, so adding a command means touching only the module it belongs to.
pub async fn run(command: Value, store: &Store) -> Value {
    let Some(parts) = into_parts(command) else {
        return Value::Error("ERR expected an array of bulk strings".into());
    };
    let Some((name, args)) = parts.split_first() else {
        return Value::Error("ERR empty command".into());
    };

    let uppercased = name.to_uppercase();

    if let Some(reply) = strings::run(&uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = lists::run(&uppercased, args, store).await {
        return reply;
    }
    if let Some(reply) = streams::run(&uppercased, args, store).await {
        return reply;
    }
    if let Some(reply) = keys::run(&uppercased, args, store) {
        return reply;
    }

    Value::Error(format!("ERR unknown command '{name}'"))
}

/// Clients send commands as an array of bulk strings; anything else is invalid.
fn into_parts(command: Value) -> Option<Vec<String>> {
    let Value::Array(parts) = command else {
        return None;
    };

    parts
        .into_iter()
        .map(|part| match part {
            Value::BulkString(text) => Some(text),
            _ => None,
        })
        .collect()
}

pub(crate) fn wrong_arity(command: &str) -> Value {
    Value::Error(format!(
        "ERR wrong number of arguments for '{command}' command"
    ))
}

pub(crate) fn not_an_integer() -> Value {
    Value::Error("ERR value is not an integer or out of range".into())
}

pub(crate) fn wrong_type() -> Value {
    Value::Error("WRONGTYPE Operation against a key holding the wrong kind of value".into())
}
