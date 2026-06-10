use super::{Command, wrong_arity};
use crate::resp::Value;
use bytes::Bytes;
use std::collections::HashSet;

/// The transaction a connection is in the middle of.
///
/// Unlike everything in the store, this belongs to one client alone: two
/// connections may each be inside a transaction without seeing the other's.
#[derive(Default)]
pub struct Transaction {
    /// The commands written down so far. `None` until `MULTI` opens the queue,
    /// which is what tells an open transaction from no transaction at all.
    queued: Option<Vec<Command>>,
    /// The keys `WATCH` was told to keep an eye on. Watching the same key twice
    /// is no different from watching it once, so a set is the right shape.
    ///
    /// Nothing consults these yet; holding an `EXEC` back over a key that
    /// changed is what they are being gathered for.
    watched: HashSet<Bytes>,
}

/// What the dispatcher is to do with a transaction command.
pub enum Outcome {
    Reply(Value),
    /// The commands `EXEC` had waiting, for the dispatcher to run in order.
    Execute(Vec<Command>),
}

impl Transaction {
    /// The commands waiting to be run, or `None` when no transaction is open.
    ///
    /// A connection only ever creates a transaction; steering it is the
    /// dispatcher's business, so the rest stays inside `commands`.
    pub(super) fn queued(&mut self) -> Option<&mut Vec<Command>> {
        self.queued.as_mut()
    }
}

/// Whether a command steers a transaction rather than takes part in one. These
/// are the commands that run even while everything else is being queued.
///
/// `WATCH` belongs here even though it opens no transaction: watching a key is
/// something a client does *before* `MULTI`, so queueing it would be too late
/// to be of any use.
pub fn steers_a_transaction(command: &str) -> bool {
    matches!(command, "MULTI" | "EXEC" | "DISCARD" | "WATCH")
}

/// Handles the commands that make up a transaction.
/// `None` means the command belongs to another module.
pub fn run(command: &Command, transaction: &mut Transaction) -> Option<Outcome> {
    let reply = match command.uppercased.as_str() {
        "MULTI" => match command.args.as_slice() {
            [] if transaction.queued.is_some() => nested_multi(),
            [] => {
                transaction.queued = Some(Vec::new());
                Value::SimpleString("OK".into())
            }
            _ => wrong_arity("multi"),
        },
        "EXEC" => match command.args.as_slice() {
            [] => match transaction.queued.take() {
                Some(queued) => return Some(Outcome::Execute(queued)),
                None => exec_without_multi(),
            },
            _ => wrong_arity("exec"),
        },
        "WATCH" => match command.args.as_slice() {
            [] => wrong_arity("watch"),
            // A transaction is already queueing commands by this point, so
            // watching now would be asking to be told about changes that the
            // client has no way left to act on.
            _ if transaction.queued.is_some() => watch_inside_multi(),
            keys => {
                transaction.watched.extend(keys.iter().cloned());
                Value::SimpleString("OK".into())
            }
        },
        "DISCARD" => match command.args.as_slice() {
            // Dropping the queue is the whole of it: none of those commands
            // ever reached the store.
            [] => match transaction.queued.take() {
                Some(_) => Value::SimpleString("OK".into()),
                None => discard_without_multi(),
            },
            _ => wrong_arity("discard"),
        },
        _ => return None,
    };

    Some(Outcome::Reply(reply))
}

fn nested_multi() -> Value {
    Value::Error("ERR MULTI calls can not be nested".into())
}

fn watch_inside_multi() -> Value {
    Value::Error("ERR WATCH inside MULTI is not allowed".into())
}

fn exec_without_multi() -> Value {
    Value::Error("ERR EXEC without MULTI".into())
}

fn discard_without_multi() -> Value {
    Value::Error("ERR DISCARD without MULTI".into())
}
