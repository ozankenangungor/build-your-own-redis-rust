use super::{not_an_integer, text, wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{Store, WrongType};
use bytes::Bytes;
use std::time::Duration;

/// Handles the commands that work on plain string values, plus `PING`.
/// `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
    let reply = match command {
        "PING" => Value::SimpleString("PONG".into()),
        "ECHO" => match args {
            [message] => Value::BulkString(message.clone()),
            _ => wrong_arity("echo"),
        },
        "SET" => match args {
            [key, value, options @ ..] => set(store, key, value, options),
            _ => wrong_arity("set"),
        },
        "GET" => match args {
            [key] => match store.get(key) {
                Ok(value) => value.map_or(Value::Null, Value::BulkString),
                Err(WrongType) => wrong_type(),
            },
            _ => wrong_arity("get"),
        },
        _ => return None,
    };

    Some(reply)
}

fn set(store: &Store, key: &Bytes, value: &Bytes, options: &[Bytes]) -> Value {
    match parse_expiry(options) {
        Ok(expires_in) => {
            store.set(key.clone(), value.clone(), expires_in);
            Value::SimpleString("OK".into())
        }
        Err(error) => error,
    }
}

/// Reads the trailing options of `SET`. Only the expiry ones are supported so
/// far, and the error replies match what real Redis sends.
fn parse_expiry(options: &[Bytes]) -> Result<Option<Duration>, Value> {
    let [unit, amount] = options else {
        return match options {
            [] => Ok(None),
            _ => Err(syntax_error()),
        };
    };

    let amount = text(amount)
        .and_then(|amount| amount.parse().ok())
        .ok_or_else(not_an_integer)?;

    match text(unit).map(str::to_uppercase).as_deref() {
        Some("EX") => Ok(Some(Duration::from_secs(amount))),
        Some("PX") => Ok(Some(Duration::from_millis(amount))),
        _ => Err(syntax_error()),
    }
}

fn syntax_error() -> Value {
    Value::Error("ERR syntax error".into())
}
