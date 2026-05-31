use super::{not_an_integer, text, wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{Blocked, Side, Store, WrongType};
use bytes::Bytes;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time;

/// Handles the commands that work on lists. `None` means the command belongs to
/// another module.
pub async fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
    let reply = match command {
        "RPUSH" => match args {
            [key, elements @ ..] if !elements.is_empty() => push(store, key, elements, Side::Right),
            _ => wrong_arity("rpush"),
        },
        "LPUSH" => match args {
            [key, elements @ ..] if !elements.is_empty() => push(store, key, elements, Side::Left),
            _ => wrong_arity("lpush"),
        },
        "LPOP" => match args {
            [key] => lpop(store, key, None),
            [key, count] => lpop(store, key, Some(count)),
            _ => wrong_arity("lpop"),
        },
        "BLPOP" => match args {
            [key, timeout] => blpop(store, key, timeout).await,
            _ => wrong_arity("blpop"),
        },
        "LLEN" => match args {
            [key] => match store.llen(key) {
                Ok(length) => Value::Integer(length as i64),
                Err(WrongType) => wrong_type(),
            },
            _ => wrong_arity("llen"),
        },
        "LRANGE" => match args {
            [key, start, stop] => lrange(store, key, start, stop),
            _ => wrong_arity("lrange"),
        },
        _ => return None,
    };

    Some(reply)
}

fn push(store: &Store, key: &Bytes, elements: &[Bytes], side: Side) -> Value {
    match store.push(key, elements, side) {
        Ok(length) => Value::Integer(length as i64),
        Err(WrongType) => wrong_type(),
    }
}

/// Without a count `LPOP` replies with a single element, with one it replies
/// with an array — even when that array holds a single element, or none.
fn lpop(store: &Store, key: &Bytes, count: Option<&Bytes>) -> Value {
    let Some(count) = count else {
        return match store.lpop(key, 1) {
            Ok(removed) => removed
                .into_iter()
                .next()
                .map_or(Value::Null, Value::BulkString),
            Err(WrongType) => wrong_type(),
        };
    };

    let Some(count) = text(count).and_then(|count| count.parse::<i64>().ok()) else {
        return not_an_integer();
    };
    let Ok(count) = usize::try_from(count) else {
        return Value::Error("ERR value is out of range, must be positive".into());
    };

    match store.lpop(key, count) {
        Ok(removed) => Value::Array(removed.into_iter().map(Value::BulkString).collect()),
        Err(WrongType) => wrong_type(),
    }
}

/// Pops the first element of `key`, waiting for one to arrive if the list is
/// empty. The reply names the list, since a later stage lets a client block on
/// several at once.
async fn blpop(store: &Store, key: &Bytes, timeout: &Bytes) -> Value {
    let timeout = match parse_timeout(timeout) {
        Ok(timeout) => timeout,
        Err(error) => return error,
    };

    let element = match store.blpop(key) {
        Ok(Blocked::Ready(element)) => Some(element),
        Ok(Blocked::Waiting(receiver)) => wait_for_element(receiver, timeout).await,
        Err(WrongType) => return wrong_type(),
    };

    match element {
        Some(element) => Value::Array(vec![
            Value::BulkString(key.clone()),
            Value::BulkString(element),
        ]),
        None => Value::NullArray,
    }
}

/// Reads a blocking command's timeout, given in seconds. `None` stands for the
/// timeout of zero, which asks to wait indefinitely.
fn parse_timeout(timeout: &Bytes) -> Result<Option<Duration>, Value> {
    let Some(seconds) = text(timeout).and_then(|timeout| timeout.parse::<f64>().ok()) else {
        return Err(bad_timeout());
    };
    if seconds < 0.0 {
        return Err(Value::Error("ERR timeout is negative".into()));
    }
    if seconds == 0.0 {
        return Ok(None);
    }

    // Rejects the values a `Duration` cannot hold, such as NaN or infinity.
    Duration::try_from_secs_f64(seconds)
        .map(Some)
        .map_err(|_| bad_timeout())
}

/// Dropping the receiver on a timeout also marks the queued waiter as gone, so
/// the store hands the element to the next client instead of losing it.
async fn wait_for_element(
    receiver: oneshot::Receiver<Bytes>,
    timeout: Option<Duration>,
) -> Option<Bytes> {
    match timeout {
        None => receiver.await.ok(),
        Some(timeout) => time::timeout(timeout, receiver).await.ok()?.ok(),
    }
}

fn lrange(store: &Store, key: &Bytes, start: &Bytes, stop: &Bytes) -> Value {
    let (Some(start), Some(stop)) = (parse_index(start), parse_index(stop)) else {
        return not_an_integer();
    };

    match store.lrange(key, start, stop) {
        Ok(elements) => Value::Array(elements.into_iter().map(Value::BulkString).collect()),
        Err(WrongType) => wrong_type(),
    }
}

fn parse_index(index: &Bytes) -> Option<i64> {
    text(index)?.parse().ok()
}

fn bad_timeout() -> Value {
    Value::Error("ERR timeout is not a float or out of range".into())
}
