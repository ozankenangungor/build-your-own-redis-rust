mod info;
mod keys;
mod lists;
mod streams;
mod strings;
mod transactions;

pub use transactions::Transaction;

use crate::resp::Value;
use crate::server::Server;
use crate::store::Store;
use bytes::Bytes;
use transactions::Outcome;

/// One command as the client sent it.
struct Command {
    /// The name uppercased, since command names are case insensitive.
    uppercased: String,
    /// The name as the client spelled it, which is what error messages echo.
    name: Bytes,
    args: Vec<Bytes>,
}

/// Runs one client command and produces the reply to send back.
pub async fn run(
    command: Value,
    store: &Store,
    transaction: &mut Transaction,
    server: &Server,
) -> Value {
    let command = match Command::parse(command) {
        Ok(command) => command,
        Err(reply) => return reply,
    };

    // Inside a transaction a command is only written down. Nothing runs until
    // `EXEC`, so the store must not see it yet.
    if !transactions::steers_a_transaction(&command.uppercased)
        && let Some(queued) = transaction.queued()
    {
        queued.push(command);
        return Value::SimpleString("QUEUED".into());
    }

    match transactions::steer(&command, transaction, store) {
        Some(Outcome::Reply(reply)) => reply,
        Some(Outcome::Execute(queued)) => execute(queued, store, transaction, server).await,
        None => dispatch(&command, store, transaction, server).await,
    }
}

/// Runs the commands a transaction had queued, gathering their replies into the
/// array `EXEC` answers with.
///
/// The commands that steer a transaction are never queued, so nothing here can
/// open or execute another one: this runs one layer below `run` rather than
/// back through it.
async fn execute(
    queued: Vec<Command>,
    store: &Store,
    transaction: &mut Transaction,
    server: &Server,
) -> Value {
    let mut replies = Vec::with_capacity(queued.len());

    for command in &queued {
        replies.push(dispatch(command, store, transaction, server).await);
    }

    Value::Array(replies)
}

/// Runs one command, against the store or against the connection's own state.
///
/// Each module below claims the commands it knows and returns `None` for the
/// rest, so adding a command means touching only the module it belongs to.
async fn dispatch(
    command: &Command,
    store: &Store,
    transaction: &mut Transaction,
    server: &Server,
) -> Value {
    let Command {
        uppercased,
        name,
        args,
    } = command;

    if let Some(reply) = transactions::run(uppercased, args, transaction) {
        return reply;
    }
    if let Some(reply) = strings::run(uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = lists::run(uppercased, args, store).await {
        return reply;
    }
    if let Some(reply) = streams::run(uppercased, args, store).await {
        return reply;
    }
    if let Some(reply) = keys::run(uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = info::run(uppercased, args, server) {
        return reply;
    }

    unknown_command(name)
}

impl Command {
    /// Reads a command as clients send them: an array of bulk strings, the
    /// first of which names the command. The error reply says what was wrong.
    fn parse(command: Value) -> Result<Self, Value> {
        let Some(parts) = into_parts(command) else {
            return Err(Value::Error("ERR expected an array of bulk strings".into()));
        };
        let Some((name, args)) = parts.split_first() else {
            return Err(Value::Error("ERR empty command".into()));
        };
        // Command names are ASCII, so anything else is unknown by definition.
        let Some(uppercased) = text(name).map(str::to_uppercase) else {
            return Err(unknown_command(name));
        };

        Ok(Self {
            uppercased,
            name: name.clone(),
            args: args.to_vec(),
        })
    }
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
