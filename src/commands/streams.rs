use super::{wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{EntryId, Store, WrongType};

/// Handles the commands that work on streams. `None` means the command belongs
/// to another module.
pub fn run(command: &str, args: &[String], store: &Store) -> Option<Value> {
    let reply = match command {
        "XADD" => match args {
            // Fields come in pairs, and an entry needs at least one of them.
            [key, id, fields @ ..] if !fields.is_empty() && fields.len() % 2 == 0 => {
                xadd(store, key, id, fields)
            }
            _ => wrong_arity("xadd"),
        },
        _ => return None,
    };

    Some(reply)
}

fn xadd(store: &Store, key: &str, id: &str, fields: &[String]) -> Value {
    let Ok(id) = id.parse::<EntryId>() else {
        return invalid_id();
    };

    let fields = fields
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();

    match store.xadd(key, id, fields) {
        Ok(id) => Value::BulkString(id.to_string()),
        Err(WrongType) => wrong_type(),
    }
}

fn invalid_id() -> Value {
    Value::Error("ERR Invalid stream ID specified as stream command argument".into())
}
