use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// The transaction a connection is in the middle of.
///
/// Unlike everything in the store, this belongs to one client alone: two
/// connections may each be inside a transaction without seeing the other's.
#[derive(Default)]
pub struct Transaction {
    open: bool,
}

/// Handles the commands that make up a transaction.
/// `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], transaction: &mut Transaction) -> Option<Value> {
    let reply = match command {
        "MULTI" => match args {
            [] if transaction.open => nested_multi(),
            [] => {
                transaction.open = true;
                Value::SimpleString("OK".into())
            }
            _ => wrong_arity("multi"),
        },
        "EXEC" => match args {
            [] if transaction.open => {
                transaction.open = false;
                Value::Array(Vec::new())
            }
            [] => exec_without_multi(),
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
