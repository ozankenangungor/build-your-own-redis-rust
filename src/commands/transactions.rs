use super::{Command, wrong_arity};
use crate::resp::Value;
use crate::store::Store;
use bytes::Bytes;
use std::collections::HashMap;
use std::mem;

/// The transaction a connection is in the middle of.
///
/// Unlike everything in the store, this belongs to one client alone: two
/// connections may each be inside a transaction without seeing the other's.
#[derive(Default)]
pub struct Transaction {
    /// The commands written down so far. `None` until `MULTI` opens the queue,
    /// which is what tells an open transaction from no transaction at all.
    queued: Option<Vec<Command>>,
    /// The keys `WATCH` was told to keep an eye on, each with the version it
    /// held at the time. `None` records a key that was not there, which is a
    /// state to be changed away from like any other.
    watched: HashMap<Bytes, Option<u64>>,
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
/// to be of any use. `UNWATCH` does not, and is queued like any other command.
pub fn steers_a_transaction(command: &str) -> bool {
    matches!(command, "MULTI" | "EXEC" | "DISCARD" | "WATCH")
}

/// Handles the commands that steer a transaction, which are the ones a
/// transaction never queues. `None` means the command is not one of them.
pub fn steer(command: &Command, transaction: &mut Transaction, store: &Store) -> Option<Outcome> {
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
                Some(queued) => {
                    // The keys were watched for the sake of this transaction,
                    // so the watch ends with it either way.
                    let watched = mem::take(&mut transaction.watched);

                    if store.unchanged(&watched) {
                        return Some(Outcome::Execute(queued));
                    }

                    // Something moved under the transaction, so none of it
                    // runs. The null array tells this apart from a transaction
                    // that ran and had nothing to say.
                    Value::NullArray
                }
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
                for key in keys {
                    // Watching a key a second time keeps the first reading, so
                    // that a change already missed is not forgotten.
                    transaction
                        .watched
                        .entry(key.clone())
                        .or_insert_with(|| store.version(key));
                }

                Value::SimpleString("OK".into())
            }
        },
        "DISCARD" => match command.args.as_slice() {
            // None of the queued commands ever reached the store, so dropping
            // them is all the undoing there is to do.
            [] => match transaction.queued.take() {
                Some(_) => {
                    // The watches were taken out for the transaction being
                    // thrown away, and go with it, exactly as at `EXEC`.
                    transaction.watched.clear();
                    Value::SimpleString("OK".into())
                }
                None => discard_without_multi(),
            },
            _ => wrong_arity("discard"),
        },
        _ => return None,
    };

    Some(Outcome::Reply(reply))
}

/// Handles the transaction commands that take part in a transaction rather
/// than steer it, and so are queued like the commands of any other family.
pub fn run(command: &str, args: &[Bytes], transaction: &mut Transaction) -> Option<Value> {
    let reply = match command {
        "UNWATCH" => match args {
            // Watching nothing is a fine thing to stop doing, so there is no
            // counterpart here to `EXEC without MULTI`.
            [] => {
                transaction.watched.clear();
                Value::SimpleString("OK".into())
            }
            _ => wrong_arity("unwatch"),
        },
        _ => return None,
    };

    Some(reply)
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
