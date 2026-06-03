mod keys;
mod lists;
mod streams;
mod strings;
mod transactions;

pub use transactions::Transaction;

use crate::resp::Value;
use crate::store::Store;
use bytes::Bytes;

/// Runs one client command and produces the reply to send back.
///
/// Each module below claims the commands it knows and returns `None` for the
/// rest, so adding a command means touching only the module it belongs to.
pub async fn run(command: Value, store: &Store, transaction: &mut Transaction) -> Value {
    let Some(parts) = into_parts(command) else {
        return Value::Error("ERR expected an array of bulk strings".into());
    };
    let Some((name, args)) = parts.split_first() else {
        return Value::Error("ERR empty command".into());
    };

    // Command names are ASCII, so anything else is unknown by definition.
    let Some(uppercased) = text(name).map(str::to_uppercase) else {
        return unknown_command(name);
    };

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
    if let Some(reply) = transactions::run(&uppercased, args, transaction) {
        return reply;
    }

    unknown_command(name)
}

/// Clients send commands as an array of bulk strings; anything else is invalid.
fn into_parts(command: Value) -> Option<Vec<Bytes>> {
    let Value::Array(parts) = command else {
        return None;
    };

    parts
        .into_iter()
        .map(|part| match part {
            Value::BulkString(bytes) => Some(bytes),
            _ => None,
        })
        .collect()
}

/// Reads an argument as text. Command names, option keywords and numbers are
/// ASCII by definition, unlike the keys and values they sit next to.
fn text(argument: &[u8]) -> Option<&str> {
    str::from_utf8(argument).ok()
}

fn unknown_command(name: &[u8]) -> Value {
    Value::Error(format!(
        "ERR unknown command '{}'",
        String::from_utf8_lossy(name)
    ))
}

fn wrong_arity(command: &str) -> Value {
    Value::Error(format!(
        "ERR wrong number of arguments for '{command}' command"
    ))
}

fn not_an_integer() -> Value {
    Value::Error("ERR value is not an integer or out of range".into())
}

fn wrong_type() -> Value {
    Value::Error("WRONGTYPE Operation against a key holding the wrong kind of value".into())
}
