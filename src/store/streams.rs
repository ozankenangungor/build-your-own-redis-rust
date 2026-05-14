use super::{Data, Entry, Store, WrongType, drop_if_expired};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// The identifier of a stream entry: a millisecond timestamp and a sequence
/// number that orders entries recorded within the same millisecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryId {
    pub milliseconds: u64,
    pub sequence: u64,
}

/// One entry of a stream: an id and the field-value pairs recorded under it.
#[derive(Debug, Clone)]
pub struct StreamEntry {
    pub id: EntryId,
    pub fields: Vec<(String, String)>,
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

/// Why an `XADD` was refused.
pub enum XaddError {
    WrongType,
    /// Ids have to be strictly greater than `0-0`.
    NotAboveZero,
    /// Ids have to be strictly greater than the stream's last entry.
    NotAboveTop,
}

impl EntryId {
    /// The lower bound every id has to beat; `0-1` is the smallest valid id.
    pub const ZERO: Self = Self {
        milliseconds: 0,
        sequence: 0,
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

impl Store {
    /// Appends an entry to the stream at `key`, creating the stream if it does
    /// not exist yet, and returns the id it was stored under.
    ///
    /// Ids have to arrive in strictly increasing order, which is what makes a
    /// stream a log rather than a bag of entries.
    pub fn xadd(
        &self,
        key: &str,
        requested: RequestedId,
        fields: Vec<(String, String)>,
    ) -> Result<EntryId, XaddError> {
        // Redis rejects `0-0` before it ever looks at the key.
        if let RequestedId::Explicit(id) = requested
            && id <= EntryId::ZERO
        {
            return Err(XaddError::NotAboveZero);
        }

        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let entry = state
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry::new(Data::Stream(Vec::new())));

        let Data::Stream(stream) = &mut entry.data else {
            return Err(XaddError::WrongType);
        };

        let top = stream.last().map(|entry| entry.id);
        let id = resolve(requested, top)?;

        if top.is_some_and(|top| id <= top) {
            return Err(XaddError::NotAboveTop);
        }

        stream.push(StreamEntry { id, fields });
        Ok(id)
    }

    /// Returns the entries of the stream at `key` whose ids fall between
    /// `start` and `end`, both included.
    pub fn xrange(
        &self,
        key: &str,
        start: EntryId,
        end: EntryId,
    ) -> Result<Vec<StreamEntry>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let stream = match state.entries.get(key) {
            None => return Ok(Vec::new()),
            Some(Entry {
                data: Data::Stream(stream),
                ..
            }) => stream,
            Some(_) => return Err(WrongType),
        };

        // Entries are appended in increasing id order, so the range asked for
        // is a contiguous slice that binary search can find.
        let first = stream.partition_point(|entry| entry.id < start);
        let last = stream.partition_point(|entry| entry.id <= end);

        Ok(stream[first..last].to_vec())
    }
}
