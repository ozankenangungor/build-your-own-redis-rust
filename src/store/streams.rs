use super::{Data, Entry, Store, drop_if_expired};
use std::fmt;
use std::str::FromStr;

/// The identifier of a stream entry: a millisecond timestamp and a sequence
/// number that orders entries recorded within the same millisecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryId {
    pub milliseconds: u64,
    pub sequence: u64,
}

/// One entry of a stream: an id and the field-value pairs recorded under it.
#[derive(Debug)]
pub struct StreamEntry {
    pub id: EntryId,
    /// Read back by the stream query commands, which come in later stages.
    #[allow(dead_code)]
    pub fields: Vec<(String, String)>,
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

/// Ids arrive as `<milliseconds>-<sequence>`; anything else is malformed.
impl FromStr for EntryId {
    type Err = ();

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        let (milliseconds, sequence) = id.split_once('-').ok_or(())?;

        Ok(Self {
            milliseconds: milliseconds.parse().map_err(|_| ())?,
            sequence: sequence.parse().map_err(|_| ())?,
        })
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
        id: EntryId,
        fields: Vec<(String, String)>,
    ) -> Result<EntryId, XaddError> {
        if id <= EntryId::ZERO {
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

        if let Some(top) = stream.last()
            && id <= top.id
        {
            return Err(XaddError::NotAboveTop);
        }

        stream.push(StreamEntry { id, fields });
        Ok(id)
    }
}
