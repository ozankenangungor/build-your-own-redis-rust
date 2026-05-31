use super::{text, wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{
    EntryId, ReadStream, RequestedId, Store, StreamEntry, StreamRead, WrongType, XaddError,
};
use bytes::Bytes;
use std::time::{Duration, Instant};
use tokio::time;

/// Handles the commands that work on streams. `None` means the command belongs
/// to another module.
pub async fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
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
        "XREAD" => xread(store, args).await,
        _ => return None,
    };

    Some(reply)
}

fn xadd(store: &Store, key: &Bytes, id: &Bytes, fields: &[Bytes]) -> Value {
    let Some(id) = text(id).and_then(|id| id.parse::<RequestedId>().ok()) else {
        return invalid_id();
    };

    let fields = fields
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();

    match store.xadd(key, id, fields) {
        Ok(id) => Value::BulkString(id.to_string().into()),
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

fn xrange(store: &Store, key: &Bytes, start: &Bytes, end: &Bytes) -> Value {
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

/// How long an `XREAD` is willing to wait for entries to turn up.
enum Wait {
    /// Without `BLOCK`, the reply holds whatever is there right now.
    NotAtAll,
    Until(Instant),
    Forever,
}

/// A key to read from, with the id to read after. `None` stands for `$`, which
/// only the store can resolve.
type PendingRead = (Bytes, Option<EntryId>);

/// Reads the entries recorded after the given ids, which `XREAD` treats as
/// exclusive, optionally waiting for new ones to arrive.
async fn xread(store: &Store, args: &[Bytes]) -> Value {
    let (wait, pending) = match parse_xread(args) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };

    let reads = match store.resolve_reads(&pending) {
        Ok(reads) => reads,
        Err(WrongType) => return wrong_type(),
    };

    match wait {
        Wait::NotAtAll => match store.xread(&reads) {
            Ok(streams) => encode_streams(streams),
            Err(WrongType) => wrong_type(),
        },
        Wait::Forever => wait_for_entries(store, &reads, None).await,
        Wait::Until(deadline) => wait_for_entries(store, &reads, Some(deadline)).await,
    }
}

/// Reads the arguments of `[BLOCK <milliseconds>] STREAMS <key>... <id>...`.
fn parse_xread(args: &[Bytes]) -> Result<(Wait, Vec<PendingRead>), Value> {
    let (wait, rest) = match args {
        [block, milliseconds, rest @ ..] if block.eq_ignore_ascii_case(b"BLOCK") => {
            let Some(milliseconds) =
                text(milliseconds).and_then(|milliseconds| milliseconds.parse::<u64>().ok())
            else {
                return Err(Value::Error(
                    "ERR timeout is not an integer or out of range".into(),
                ));
            };
            let wait = match milliseconds {
                // A timeout of zero asks to wait for as long as it takes.
                0 => Wait::Forever,
                milliseconds => Wait::Until(Instant::now() + Duration::from_millis(milliseconds)),
            };
            (wait, rest)
        }
        _ => (Wait::NotAtAll, args),
    };

    let arguments = match rest {
        [streams, arguments @ ..] if streams.eq_ignore_ascii_case(b"STREAMS") => arguments,
        [] => return Err(wrong_arity("xread")),
        _ => return Err(Value::Error("ERR syntax error".into())),
    };

    // The keys come first and their ids follow, one for each.
    if arguments.is_empty() || !arguments.len().is_multiple_of(2) {
        return Err(Value::Error(
            "ERR Unbalanced XREAD list of streams: for each stream key an ID or '$' must be specified."
                .into(),
        ));
    }

    let (keys, ids) = arguments.split_at(arguments.len() / 2);
    let mut pending = Vec::with_capacity(keys.len());

    for (key, id) in keys.iter().zip(ids) {
        // `$` names whichever entry the stream ends at, which only the store
        // can tell us, so it is left open here.
        let after = match text(id) {
            Some("$") => None,
            Some(id) => Some(parse_id(id, 0).ok_or_else(invalid_id)?),
            None => return Err(invalid_id()),
        };

        pending.push((key.clone(), after));
    }

    Ok((wait, pending))
}

/// Reads again every time one of the streams grows, until something turns up or
/// the deadline passes.
async fn wait_for_entries(
    store: &Store,
    reads: &[(Bytes, EntryId)],
    deadline: Option<Instant>,
) -> Value {
    loop {
        let waker = match store.xread_or_watch(reads) {
            Ok(StreamRead::Ready(streams)) => return encode_streams(streams),
            Ok(StreamRead::Waiting(waker)) => waker,
            Err(WrongType) => return wrong_type(),
        };

        let Some(deadline) = deadline else {
            waker.notified().await;
            continue;
        };

        // Measured afresh each time round, so the waiting adds up to the
        // timeout the client asked for rather than restarting on every wake-up.
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Value::NullArray;
        };
        if time::timeout(remaining, waker.notified()).await.is_err() {
            return Value::NullArray;
        }
    }
}

/// Streams that came back empty are left out, and a reply with none left has
/// nothing to report.
fn encode_streams(streams: Vec<ReadStream>) -> Value {
    if streams.is_empty() {
        return Value::NullArray;
    }

    Value::Array(
        streams
            .into_iter()
            .map(|(key, entries)| {
                Value::Array(vec![
                    Value::BulkString(key),
                    Value::Array(entries.into_iter().map(encode_entry).collect()),
                ])
            })
            .collect(),
    )
}

/// `XRANGE` also accepts the two ends of the stream in place of an id, so that
/// a range can be asked for without knowing the first and last ids.
fn parse_bound(bound: &Bytes, missing_sequence: u64) -> Option<EntryId> {
    match text(bound)? {
        "-" => Some(EntryId::ZERO),
        "+" => Some(EntryId::MAX),
        id => parse_id(id, missing_sequence),
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
        Value::BulkString(entry.id.to_string().into()),
        Value::Array(fields),
    ])
}

fn invalid_id() -> Value {
    Value::Error("ERR Invalid stream ID specified as stream command argument".into())
}
