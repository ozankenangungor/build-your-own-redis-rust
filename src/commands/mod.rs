mod acl;
mod geo;
mod info;
mod keys;
mod lists;
mod pubsub;
mod replication;
mod settings;
mod sorted_sets;
mod streams;
mod strings;
mod transactions;

pub use acl::Identity;
pub use pubsub::Subscriptions;
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

/// What a command leaves the connection it came in on.
pub enum Answer {
    /// Send this back, and go on serving whoever sent it.
    Reply(Value),
    /// Send this back, after which the connection is a replica's: it carries
    /// what the master is told to change and nothing is expected in return.
    Replica(Value),
}

/// Runs one client command and produces the reply to send back.
pub async fn run(
    command: Value,
    store: &Store,
    transaction: &mut Transaction,
    subscriptions: &mut Subscriptions,
    identity: &mut Identity,
    server: &Server,
) -> Answer {
    let command = match Command::parse(command) {
        Ok(command) => command,
        Err(reply) => return Answer::Reply(reply),
    };

    // A client the server has not been told who it is does nothing on the
    // server's behalf, save what it needs to say who it is. This comes first of
    // all: everything below is work, and work is what is being withheld.
    if !identity.is_authenticated() && !acl::allowed_while_out(&command.uppercased) {
        return Answer::Reply(acl::needs_authenticating());
    }

    // A client listening on a channel is in a conversation of another kind, and
    // only the commands that steer that conversation are open to it. This comes
    // before anything else so that nothing is queued or run that the client was
    // in no position to ask for.
    if subscriptions.listening() && !pubsub::allowed_while_listening(&command.uppercased) {
        return Answer::Reply(pubsub::out_of_context(&command.uppercased));
    }

    // Inside a transaction a command is only written down. Nothing runs until
    // `EXEC`, so the store must not see it yet.
    if !transactions::steers_a_transaction(&command.uppercased)
        && let Some(queued) = transaction.queued()
    {
        queued.push(command);
        return Answer::Reply(Value::SimpleString("QUEUED".into()));
    }

    let reply = match transactions::steer(&command, transaction, store) {
        Some(Outcome::Reply(reply)) => reply,
        Some(Outcome::Execute(queued)) => {
            execute(queued, store, transaction, subscriptions, identity, server).await
        }
        None => {
            dispatch(
                &command,
                store,
                transaction,
                subscriptions,
                identity,
                server,
            )
            .await
        }
    };

    // A replica that has been handed the dataset is no longer a client. What
    // becomes of the connection is the dispatcher's to decide, since the
    // command modules answer without knowing what carried them.
    if command.uppercased == "PSYNC" && !matches!(reply, Value::Error(_)) {
        return Answer::Replica(reply);
    }

    Answer::Reply(reply)
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
    subscriptions: &mut Subscriptions,
    identity: &mut Identity,
    server: &Server,
) -> Value {
    let mut replies = Vec::with_capacity(queued.len());

    for command in &queued {
        replies.push(dispatch(command, store, transaction, subscriptions, identity, server).await);
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
    subscriptions: &mut Subscriptions,
    identity: &mut Identity,
    server: &Server,
) -> Value {
    let reply = run_against(command, store, transaction, subscriptions, identity, server).await;

    // A command that failed changed nothing, and one that changed nothing is
    // nothing to pass on or write down.
    if matches!(reply, Value::Error(_)) {
        return reply;
    }
    let Some(written) = written_down(command, &reply) else {
        return reply;
    };

    // The record of the write is made before its reply goes out, so that a
    // client is never told a write took when nothing knows of it but memory.
    if let Some(aof) = server.aof.get()
        && let Err(e) = aof.record(&written).await
    {
        eprintln!("could not record a command: {e:#}");
        return unrecorded();
    }

    // Replicas are told what changed only once it has, and only by the server
    // the change was asked of: a replica passes nothing on of its own.
    if server.config.replicaof.is_none() {
        let passed_on = server.replicas.send(&written);
        server.replication.advance(passed_on);
    }

    reply
}

/// Hands the command to the module that knows it.
async fn run_against(
    command: &Command,
    store: &Store,
    transaction: &mut Transaction,
    subscriptions: &mut Subscriptions,
    identity: &mut Identity,
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
    // Before the rest, since a client that is listening is answered differently
    // on a command another module would otherwise claim.
    if let Some(reply) = pubsub::run(uppercased, args, subscriptions, &server.channels) {
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
    if let Some(reply) = sorted_sets::run(uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = geo::run(uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = keys::run(uppercased, args, store) {
        return reply;
    }
    if let Some(reply) = acl::run(uppercased, args, &server.users, identity) {
        return reply;
    }
    if let Some(reply) = info::run(uppercased, args, server) {
        return reply;
    }
    if let Some(reply) = settings::run(uppercased, args, server) {
        return reply;
    }
    if let Some(reply) = replication::run(uppercased, args, server).await {
        return reply;
    }

    unknown_command(name)
}

/// What a command that changed the store leaves for the replicas and the
/// record: the words that would bring another server to the same state.
///
/// Usually the command as it came in. `BLPOP` is the exception, and the reason
/// this answers with words rather than with yes or no: passed on as it stands it
/// would leave whoever replayed it waiting on a list, when what happened here
/// was that an element went. Redis passes on what a command did rather than the
/// command itself, and what this one did was an `LPOP`.
///
/// `None` covers both the commands that change nothing and the `BLPOP` that
/// waited out its timeout and took nothing with it.
fn written_down(command: &Command, reply: &Value) -> Option<Value> {
    match command.uppercased.as_str() {
        "SET" | "INCR" | "RPUSH" | "LPUSH" | "LPOP" | "XADD" | "ZADD" | "ZREM" | "GEOADD" => {
            Some(command.as_sent())
        }
        // Naming an element is the only sign one went: a timeout answers with
        // nothing at all.
        "BLPOP" => match (command.args.first(), reply) {
            (Some(key), Value::Array(_)) => Some(Value::Array(vec![
                Value::BulkString(Bytes::from_static(b"LPOP")),
                Value::BulkString(key.clone()),
            ])),
            _ => None,
        },
        _ => None,
    }
}

impl Command {
    /// The command as it came in, to be handed on to replicas word for word.
    fn as_sent(&self) -> Value {
        let mut parts = Vec::with_capacity(self.args.len() + 1);

        parts.push(Value::BulkString(self.name.clone()));
        parts.extend(self.args.iter().cloned().map(Value::BulkString));

        Value::Array(parts)
    }

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

/// What a client is told when its write could not be written down.
///
/// The store has already been changed by this point, so this is not the whole
/// truth: the write took, and only the record of it did not. Saying so anyway
/// is the lesser wrong, since a client told a write succeeded will go on
/// believing it survived a restart, and this one will not have.
fn unrecorded() -> Value {
    Value::Error("MISCONF Errors writing to the AOF file".into())
}

fn wrong_type() -> Value {
    Value::Error("WRONGTYPE Operation against a key holding the wrong kind of value".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(words: &[&str]) -> Command {
        let parts = words
            .iter()
            .map(|word| Value::BulkString(Bytes::copy_from_slice(word.as_bytes())))
            .collect();

        Command::parse(Value::Array(parts)).expect("a command of bulk strings")
    }

    /// What a command is written down as, given what it answered.
    fn written(words: &[&str], reply: Value) -> Option<Vec<String>> {
        let Value::Array(parts) = written_down(&command(words), &reply)? else {
            panic!("a command is written down as an array");
        };

        Some(
            parts
                .iter()
                .map(|part| match part {
                    Value::BulkString(word) => String::from_utf8_lossy(word).into_owned(),
                    _ => panic!("a command is written down in bulk strings"),
                })
                .collect(),
        )
    }

    #[test]
    fn writes_a_write_down_as_it_came_in() {
        for words in [
            vec!["SET", "foo", "bar"],
            vec!["INCR", "counter"],
            vec!["RPUSH", "list", "a"],
            vec!["LPOP", "list"],
            vec!["ZADD", "racers", "8.0", "Sam"],
        ] {
            assert_eq!(
                written(&words, Value::SimpleString("OK".into())),
                Some(words.iter().map(|word| (*word).to_string()).collect()),
                "{words:?}"
            );
        }
    }

    #[test]
    fn writes_a_write_down_under_the_name_the_client_spelled() {
        // The words go out as they came in, since a replica reads them the way
        // any other client would.
        assert_eq!(
            written(&["set", "foo", "bar"], Value::SimpleString("OK".into())),
            Some(vec!["set".into(), "foo".into(), "bar".into()])
        );
    }

    #[test]
    fn writes_nothing_down_for_a_command_that_changes_nothing() {
        for words in [
            vec!["GET", "foo"],
            vec!["PING"],
            vec!["TYPE", "foo"],
            vec!["LRANGE", "list", "0", "-1"],
            vec!["SUBSCRIBE", "channel"],
        ] {
            assert_eq!(written(&words, Value::Null), None, "{words:?}");
        }
    }

    #[test]
    fn writes_a_blpop_down_as_the_lpop_it_amounted_to() {
        // Written down word for word it would leave whoever replayed it waiting
        // on a list. What it did was take the first element off one.
        let reply = Value::Array(vec![
            Value::BulkString(Bytes::from_static(b"list")),
            Value::BulkString(Bytes::from_static(b"a")),
        ]);

        assert_eq!(
            written(&["BLPOP", "list", "0"], reply),
            Some(vec!["LPOP".into(), "list".into()])
        );
    }

    #[test]
    fn writes_nothing_down_for_a_blpop_that_waited_and_took_nothing() {
        assert_eq!(written(&["BLPOP", "list", "0.1"], Value::NullArray), None);
    }
}
