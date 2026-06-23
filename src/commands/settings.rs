use super::{text, wrong_arity};
use crate::resp::Value;
use crate::server::Server;
use bytes::Bytes;

/// Handles the commands that ask after how the server was set up, rather than
/// after what it is holding. `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], server: &Server) -> Option<Value> {
    let reply = match command {
        // `CONFIG` covers several commands in one, told apart by the word after
        // it. Only reading settings is supported; changing them is not.
        "CONFIG" => match args.split_first() {
            Some((action, names)) if action.eq_ignore_ascii_case(b"GET") => match names {
                [] => wrong_arity("config|get"),
                names => Value::Array(settings(names, server)),
            },
            Some((action, _)) => unknown_action(action),
            None => wrong_arity("config"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Looks up the settings asked for, as the pairs of a name and its value that
/// `CONFIG GET` answers with.
///
/// A setting this server does not have contributes nothing rather than an
/// error, so asking for one alongside others still yields the others.
fn settings(names: &[Bytes], server: &Server) -> Vec<Value> {
    let mut pairs = Vec::with_capacity(names.len() * 2);

    for name in names {
        // A name spelled in bytes that are not text is not one this server has.
        let Some((name, value)) = text(name).and_then(|name| server.config.setting(name)) else {
            continue;
        };

        // The name goes back as Redis spells it rather than as it was asked
        // for, so that a client can match the answer against what it expects.
        pairs.push(Value::BulkString(Bytes::from_static(name.as_bytes())));
        pairs.push(Value::BulkString(Bytes::copy_from_slice(value.as_bytes())));
    }

    pairs
}

fn unknown_action(action: &[u8]) -> Value {
    Value::Error(format!(
        "ERR Unknown CONFIG subcommand or wrong number of arguments for '{}'",
        String::from_utf8_lossy(action)
    ))
}
