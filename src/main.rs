mod resp;
mod store;

use anyhow::Result;
use bytes::{Buf, BytesMut};
use resp::Value;
use store::Store;
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
            [key, value] => {
                store.set(key.clone(), value.clone());
                Value::SimpleString("OK".into())
            }
            _ => wrong_arity("set"),
        },
        "GET" => match args {
            [key] => store.get(key).map_or(Value::Null, Value::BulkString),
            _ => wrong_arity("get"),
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

fn wrong_arity(command: &str) -> Value {
    Value::Error(format!(
        "ERR wrong number of arguments for '{command}' command"
    ))
}
