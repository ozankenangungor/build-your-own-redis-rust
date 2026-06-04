use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// The transaction a connection is in the middle of.
///
/// Unlike everything in the store, this belongs to one client alone: two
/// connections may each be inside a transaction without seeing the other's.
#[derive(Default)]
pub struct Transaction {
    /// The commands written down so far. `None` until `MULTI` opens the queue,
    /// which is what tells an open transaction from no transaction at all.
    queued: Option<Vec<Vec<Bytes>>>,
}

impl Transaction {
    pub fn is_open(&self) -> bool {
        self.queued.is_some()
    }

    /// Writes a command down to be run when `EXEC` arrives. The caller checks
    /// that a transaction is open first, since that is what tells it to queue
    /// the command rather than run it.
    pub fn queue(&mut self, command: Vec<Bytes>) {
        if let Some(queued) = &mut self.queued {
            queued.push(command);
        }
    }
}

/// Whether a command steers a transaction rather than takes part in one. These
/// are the commands that run even while everything else is being queued.
pub fn steers_a_transaction(command: &str) -> bool {
    matches!(command, "MULTI" | "EXEC")
}

/// Handles the commands that make up a transaction.
/// `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], transaction: &mut Transaction) -> Option<Value> {
    let reply = match command {
        "MULTI" => match args {
            [] if transaction.is_open() => nested_multi(),
            [] => {
                transaction.queued = Some(Vec::new());
                Value::SimpleString("OK".into())
            }
            _ => wrong_arity("multi"),
        },
        "EXEC" => match args {
            // Running what was queued is the next thing to build; closing the
            // transaction and replying with an empty array is all that happens
            // so far.
            [] => match transaction.queued.take() {
                Some(_queued) => Value::Array(Vec::new()),
                None => exec_without_multi(),
            },
            _ => wrong_arity("exec"),
        },
        _ => return None,
    };

    Some(reply)
}

fn nested_multi() -> Value {
    Value::Error("ERR MULTI calls can not be nested".into())
}

fn exec_without_multi() -> Value {
    Value::Error("ERR EXEC without MULTI".into())
}
