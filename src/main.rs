mod resp;

use anyhow::Result;
use bytes::{Buf, BytesMut};
use resp::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (stream, addr) = listener.accept().await?;
        println!("accepted new connection from {addr}");

        // Each connection gets its own task, so a slow client cannot keep the
        // server from accepting the next one.
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                eprintln!("connection error: {e}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let mut buf = BytesMut::with_capacity(1024);

    loop {
        if stream.read_buf(&mut buf).await? == 0 {
            println!("connection closed by client");
            return Ok(());
        }

        // A read may carry several commands at once, or only part of one.
        while let Some((command, consumed)) = resp::parse(&buf)? {
            buf.advance(consumed);

            let reply = run(command);
            stream.write_all(reply.encode().as_bytes()).await?;
        }
    }
}

fn run(command: Value) -> Value {
    let Value::Array(parts) = command else {
        return Value::Error("ERR expected a command as an array".into());
    };

    let mut parts = parts.into_iter();
    let Some(Value::BulkString(name)) = parts.next() else {
        return Value::Error("ERR expected a command name".into());
    };

    match name.to_uppercase().as_str() {
        "PING" => Value::SimpleString("PONG".into()),
        "ECHO" => match parts.next() {
            Some(Value::BulkString(message)) => Value::BulkString(message),
            _ => Value::Error("ERR wrong number of arguments for 'echo' command".into()),
        },
        _ => Value::Error(format!("ERR unknown command '{name}'")),
    }
}
