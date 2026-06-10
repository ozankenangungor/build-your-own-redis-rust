use super::{Data, Entries, Entry, State, Store, WrongType, drop_if_expired};
use bytes::Bytes;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

/// The identifier of a stream entry: a millisecond timestamp and a sequence
/// number that orders entries recorded within the same millisecond.
///
/// The derived ordering compares the timestamps first and the sequence numbers
/// only to break a tie, which is exactly how Redis orders entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryId {
    pub milliseconds: u64,
    pub sequence: u64,
}

impl EntryId {
    /// The lower bound every id has to beat; `0-1` is the smallest valid id.
    pub const ZERO: Self = Self {
        milliseconds: 0,
        sequence: 0,
    };

    /// The largest id there is, and so an upper bound no entry can pass.
    pub const MAX: Self = Self {
        milliseconds: u64::MAX,
        sequence: u64::MAX,
    };
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.milliseconds, self.sequence)
    }
}

/// The id an `XADD` asked for, which may leave part of it to the server.
pub enum RequestedId {
    Explicit(EntryId),
    /// The timestamp is given and the sequence number is ours to pick.
    AutoSequence(u64),
    /// Both halves are ours to pick.
    Auto,
}

/// Ids arrive as `<milliseconds>-<sequence>`, where the sequence number may be
/// `*`, or as a bare `*`. Anything else is malformed.
impl FromStr for RequestedId {
    type Err = ();

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        if id == "*" {
            return Ok(Self::Auto);
        }

        let (milliseconds, sequence) = id.split_once('-').ok_or(())?;
        let milliseconds = milliseconds.parse().map_err(|_| ())?;

        if sequence == "*" {
            return Ok(Self::AutoSequence(milliseconds));
        }

        Ok(Self::Explicit(EntryId {
            milliseconds,
            sequence: sequence.parse().map_err(|_| ())?,
        }))
    }
}

/// One entry of a stream: an id and the field-value pairs recorded under it.
#[derive(Debug, Clone)]
pub struct StreamEntry {
    pub id: EntryId,
    pub fields: Vec<(Bytes, Bytes)>,
}

/// A stream's key together with the entries read from it.
pub type ReadStream = (Bytes, Vec<StreamEntry>);

/// The outcome of a blocking read: either something was there, or the caller
/// has a waker that fires once one of the streams grows.
pub enum StreamRead {
    Ready(Vec<ReadStream>),
    Waiting(Arc<Notify>),
}

/// Why an `XADD` was refused.
pub enum XaddError {
    WrongType,
    /// Ids have to be strictly greater than `0-0`.
    NotAboveZero,
    /// Ids have to be strictly greater than the stream's last entry.
    NotAboveTop,
}

impl Store {
    /// Appends an entry to the stream at `key`, creating the stream if it does
    /// not exist yet, and returns the id it was stored under.
    ///
    /// Ids have to arrive in strictly increasing order, which is what makes a
    /// stream a log rather than a bag of entries.
    pub fn xadd(
        &self,
        key: &Bytes,
        requested: RequestedId,
        fields: Vec<(Bytes, Bytes)>,
    ) -> Result<EntryId, XaddError> {
        // Redis rejects `0-0` before it ever looks at the key.
        if let RequestedId::Explicit(id) = requested
            && id <= EntryId::ZERO
        {
            return Err(XaddError::NotAboveZero);
        }

        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let version = state.next_version();
        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| Entry::new(Data::Stream(Vec::new()), version));

        let Data::Stream(stream) = &mut entry.data else {
            return Err(XaddError::WrongType);
        };

        let top = stream.last().map(|entry| entry.id);
        let id = resolve(requested, top)?;

        if top.is_some_and(|top| id <= top) {
            return Err(XaddError::NotAboveTop);
        }

        stream.push(StreamEntry { id, fields });
        entry.version = version;
        state.wake_watchers(key);

        Ok(id)
    }

    /// Returns the entries of the stream at `key` whose ids fall between
    /// `start` and `end`, both included.
    pub fn xrange(
        &self,
        key: &Bytes,
        start: EntryId,
        end: EntryId,
    ) -> Result<Vec<StreamEntry>, WrongType> {
        let mut state = self.state();
        let Some(stream) = stream_at(&mut state.entries, key)? else {
            return Ok(Vec::new());
        };

        // Entries are appended in increasing id order, so the range asked for
        // is a contiguous slice that binary search can find.
        let first = stream.partition_point(|entry| entry.id < start);
        let last = stream.partition_point(|entry| entry.id <= end);

        Ok(stream[first..last].to_vec())
    }

    /// Replaces the ids left open with the one each stream ends at right now,
    /// so that a read starting from `$` only sees what arrives from here on.
    /// Resolving them together keeps an entry from slipping between two keys.
    pub fn resolve_reads(
        &self,
        reads: &[(Bytes, Option<EntryId>)],
    ) -> Result<Vec<(Bytes, EntryId)>, WrongType> {
        let mut state = self.state();
        let mut resolved = Vec::with_capacity(reads.len());

        for (key, after) in reads {
            let after = match after {
                Some(after) => *after,
                None => stream_at(&mut state.entries, key)?
                    .and_then(|stream| stream.last())
                    .map_or(EntryId::ZERO, |entry| entry.id),
            };
            resolved.push((key.clone(), after));
        }

        Ok(resolved)
    }

    /// Returns the entries recorded after the given id in each stream. Unlike
    /// `XRANGE`, the entry carrying that id is not itself included, and streams
    /// with nothing new are left out.
    pub fn xread(&self, reads: &[(Bytes, EntryId)]) -> Result<Vec<ReadStream>, WrongType> {
        let mut state = self.state();
        read_streams(&mut state.entries, reads)
    }

    /// The same read, except that coming back empty leaves a watcher on every
    /// key, which fires as soon as any of them grows.
    pub fn xread_or_watch(&self, reads: &[(Bytes, EntryId)]) -> Result<StreamRead, WrongType> {
        let mut guard = self.state();
        let state = &mut *guard;

        let streams = read_streams(&mut state.entries, reads)?;
        if !streams.is_empty() {
            return Ok(StreamRead::Ready(streams));
        }

        // Registering before the lock goes away is what keeps an entry appended
        // right now from slipping past the client.
        let waker = Arc::new(Notify::new());
        for (key, _) in reads {
            let watchers = state.watchers.entry(key.clone()).or_default();
            // A client waiting on several keys is only ever woken through one of
            // them, so its wakers linger under the others. Once the client is
            // gone, the map holds the only reference left.
            watchers.retain(|waker| Arc::strong_count(waker) > 1);
            watchers.push(waker.clone());
        }

        Ok(StreamRead::Waiting(waker))
    }
}

impl State {
    /// Tells every client waiting on `key` that the stream has grown.
    fn wake_watchers(&mut self, key: &Bytes) {
        for waker in self.watchers.remove(key).unwrap_or_default() {
            // Each waker belongs to a single client, so a permit is kept for
            // one that has not started waiting yet.
            waker.notify_one();
        }
    }
}

fn read_streams(
    entries: &mut Entries,
    reads: &[(Bytes, EntryId)],
) -> Result<Vec<ReadStream>, WrongType> {
    let mut streams = Vec::new();

    for (key, after) in reads {
        let Some(stream) = stream_at(entries, key)? else {
            continue;
        };

        let first = stream.partition_point(|entry| entry.id <= *after);
        if first < stream.len() {
            streams.push((key.clone(), stream[first..].to_vec()));
        }
    }

    Ok(streams)
}

/// Looks up the stream stored at `key`. `Ok(None)` means the key is absent,
/// which the query commands treat as an empty stream rather than an error.
fn stream_at<'a>(
    entries: &'a mut Entries,
    key: &Bytes,
) -> Result<Option<&'a Vec<StreamEntry>>, WrongType> {
    drop_if_expired(entries, key);

    match entries.get(key) {
        None => Ok(None),
        Some(Entry {
            data: Data::Stream(stream),
            ..
        }) => Ok(Some(stream)),
        Some(_) => Err(WrongType),
    }
}

/// Fills in the parts of `requested` the client left to us, given the id of the
/// stream's last entry.
fn resolve(requested: RequestedId, top: Option<EntryId>) -> Result<EntryId, XaddError> {
    let milliseconds = match requested {
        RequestedId::Explicit(id) => return Ok(id),
        RequestedId::AutoSequence(milliseconds) => milliseconds,
        // Ids may never move backwards, even when the system clock does.
        RequestedId::Auto => {
            let now = now_milliseconds();
            top.map_or(now, |top| now.max(top.milliseconds))
        }
    };

    let sequence = match top {
        // Carry on the run of entries recorded in this same millisecond.
        Some(top) if top.milliseconds == milliseconds => {
            top.sequence.checked_add(1).ok_or(XaddError::NotAboveTop)?
        }
        // `0-0` is not a valid id, so the first sequence at time zero is one.
        _ if milliseconds == 0 => 1,
        _ => 0,
    };

    Ok(EntryId {
        milliseconds,
        sequence,
    })
}

/// The timestamp half of a generated id is the current Unix time, in the same
/// milliseconds the ids themselves are counted in.
fn now_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
