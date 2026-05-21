use super::{wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{EntryId, RequestedId, Store, StreamEntry, WrongType, XaddError};

/// Handles the commands that work on streams. `None` means the command belongs
/// to another module.
pub fn run(command: &str, args: &[String], store: &Store) -> Option<Value> {
    let reply = match command {
        "XADD" => match args {
            // Fields come in pairs, and an entry needs at least one of them.
            [key, id, fields @ ..] if !fields.is_empty() && fields.len().is_multiple_of(2) => {
                xadd(store, key, id, fields)
            }
            _ => wrong_arity("xadd"),
        },
        "XRANGE" => match args {
            [key, start, end] => xrange(store, key, start, end),
            _ => wrong_arity("xrange"),
        },
        "XREAD" => match args {
            [streams, rest @ ..] if streams.eq_ignore_ascii_case("STREAMS") => xread(store, rest),
            [] => wrong_arity("xread"),
            _ => Value::Error("ERR syntax error".into()),
        },
        _ => return None,
    };

    Some(reply)
}

fn xadd(store: &Store, key: &str, id: &str, fields: &[String]) -> Value {
    let Ok(id) = id.parse::<RequestedId>() else {
        return invalid_id();
    };

    let fields = fields
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();

    match store.xadd(key, id, fields) {
        Ok(id) => Value::BulkString(id.to_string()),
        Err(XaddError::WrongType) => wrong_type(),
        Err(XaddError::NotAboveZero) => {
            Value::Error("ERR The ID specified in XADD must be greater than 0-0".into())
        }
        Err(XaddError::NotAboveTop) => Value::Error(
            "ERR The ID specified in XADD is equal or smaller than the target stream top item"
                .into(),
        ),
    }
}

fn xrange(store: &Store, key: &str, start: &str, end: &str) -> Value {
    // A bound without a sequence number covers the whole millisecond, so the
    // start falls back to the lowest sequence and the end to the highest.
    let (Some(start), Some(end)) = (parse_bound(start, 0), parse_bound(end, u64::MAX)) else {
        return invalid_id();
    };

    match store.xrange(key, start, end) {
        Ok(entries) => Value::Array(entries.into_iter().map(encode_entry).collect()),
        Err(WrongType) => wrong_type(),
    }
}

/// Reads the entries recorded after the given ids, which `XREAD` treats as
/// exclusive. The keys come first and their ids follow, one for each.
fn xread(store: &Store, arguments: &[String]) -> Value {
    if arguments.is_empty() || !arguments.len().is_multiple_of(2) {
        return Value::Error(
            "ERR Unbalanced XREAD list of streams: for each stream key an ID or '$' must be specified."
                .into(),
        );
    }

    let (keys, ids) = arguments.split_at(arguments.len() / 2);
    let mut streams = Vec::new();

    for (key, id) in keys.iter().zip(ids) {
        let Some(after) = parse_id(id, 0) else {
            return invalid_id();
        };

        match store.xread(key, after) {
            // Streams with nothing new are left out of the reply entirely.
            Ok(entries) if entries.is_empty() => {}
            Ok(entries) => streams.push(Value::Array(vec![
                Value::BulkString(key.clone()),
                Value::Array(entries.into_iter().map(encode_entry).collect()),
            ])),
            Err(WrongType) => return wrong_type(),
        }
    }

    if streams.is_empty() {
        return Value::NullArray;
    }

    Value::Array(streams)
}

/// `XRANGE` also accepts the two ends of the stream in place of an id, so that
/// a range can be asked for without knowing the first and last ids.
fn parse_bound(bound: &str, missing_sequence: u64) -> Option<EntryId> {
    match bound {
        "-" => Some(EntryId::ZERO),
        "+" => Some(EntryId::MAX),
        _ => parse_id(bound, missing_sequence),
    }
}

fn parse_id(id: &str, missing_sequence: u64) -> Option<EntryId> {
    let (milliseconds, sequence) = match id.split_once('-') {
        Some((milliseconds, sequence)) => (milliseconds, sequence.parse().ok()?),
        None => (id, missing_sequence),
    };

    Some(EntryId {
        milliseconds: milliseconds.parse().ok()?,
        sequence,
    })
}

/// An entry is replied to as its id followed by a flat array of its fields and
/// values, in the order they were recorded.
fn encode_entry(entry: StreamEntry) -> Value {
    let mut fields = Vec::with_capacity(entry.fields.len() * 2);
    for (field, value) in entry.fields {
        fields.push(Value::BulkString(field));
        fields.push(Value::BulkString(value));
    }

    Value::Array(vec![
        Value::BulkString(entry.id.to_string()),
        Value::Array(fields),
    ])
}

fn invalid_id() -> Value {
    Value::Error("ERR Invalid stream ID specified as stream command argument".into())
}
