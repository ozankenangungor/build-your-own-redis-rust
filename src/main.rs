mod resp;
mod store;

use anyhow::Result;
use bytes::{Buf, BytesMut};
use resp::Value;
use std::time::Duration;
use store::{Side, Store, WrongType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    let store = Store::default();

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");

        // Each connection gets its own task, so a slow client cannot keep the
        // server from accepting the next one. Cloning the store shares it.
        let store = store.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, store).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, store: Store) -> Result<()> {
    let mut buf = BytesMut::with_capacity(1024);

    loop {
        if stream.read_buf(&mut buf).await? == 0 {
            println!("connection closed by client");
            return Ok(());
        }

        // A read may carry several commands at once, or only part of one.
        while let Some((command, consumed)) = resp::parse(&buf)? {
            buf.advance(consumed);

            let reply = run(command, &store);
            stream.write_all(reply.encode().as_bytes()).await?;
        }
    }
}

fn run(command: Value, store: &Store) -> Value {
    let Some(parts) = into_parts(command) else {
        return Value::Error("ERR expected an array of bulk strings".into());
    };
    let Some((name, args)) = parts.split_first() else {
        return Value::Error("ERR empty command".into());
    };

    match name.to_uppercase().as_str() {
        "PING" => Value::SimpleString("PONG".into()),
        "ECHO" => match args {
            [message] => Value::BulkString(message.clone()),
            _ => wrong_arity("echo"),
        },
        "SET" => match args {
            [key, value, options @ ..] => match parse_expiry(options) {
                Ok(expires_in) => {
                    store.set(key.clone(), value.clone(), expires_in);
                    Value::SimpleString("OK".into())
                }
                Err(error) => error,
            },
            _ => wrong_arity("set"),
        },
        "GET" => match args {
            [key] => match store.get(key) {
                Ok(value) => value.map_or(Value::Null, Value::BulkString),
                Err(WrongType) => wrong_type(),
            },
            _ => wrong_arity("get"),
        },
        "RPUSH" => match args {
            [key, elements @ ..] if !elements.is_empty() => push(store, key, elements, Side::Right),
            _ => wrong_arity("rpush"),
        },
        "LPUSH" => match args {
            [key, elements @ ..] if !elements.is_empty() => push(store, key, elements, Side::Left),
            _ => wrong_arity("lpush"),
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
        _ => Value::Error(format!("ERR unknown command '{name}'")),
    }
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

fn push(store: &Store, key: &str, elements: &[String], side: Side) -> Value {
    match store.push(key, elements, side) {
        Ok(length) => Value::Integer(length as i64),
        Err(WrongType) => wrong_type(),
    }
}

fn lrange(store: &Store, key: &str, start: &str, stop: &str) -> Value {
    let (Ok(start), Ok(stop)) = (start.parse(), stop.parse()) else {
        return not_an_integer();
    };

    match store.lrange(key, start, stop) {
        Ok(elements) => Value::Array(elements.into_iter().map(Value::BulkString).collect()),
        Err(WrongType) => wrong_type(),
    }
}

/// Reads the trailing options of `SET`. Only the expiry ones are supported so
/// far, and the error replies match what real Redis sends.
fn parse_expiry(options: &[String]) -> Result<Option<Duration>, Value> {
    let [unit, amount] = options else {
        return match options {
            [] => Ok(None),
            _ => Err(Value::Error("ERR syntax error".into())),
        };
    };

    let amount = amount.parse().map_err(|_| not_an_integer())?;

    match unit.to_uppercase().as_str() {
        "EX" => Ok(Some(Duration::from_secs(amount))),
        "PX" => Ok(Some(Duration::from_millis(amount))),
        _ => Err(Value::Error("ERR syntax error".into())),
    }
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
