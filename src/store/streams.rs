use super::{Data, Entry, Store, WrongType, drop_if_expired};
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
// Entries are only written so far; the commands that read them back, and the
// validation that compares ids, arrive in the stages after this one.
#[allow(dead_code)]
#[derive(Debug)]
pub struct StreamEntry {
    pub id: EntryId,
    pub fields: Vec<(String, String)>,
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
    pub fn xadd(
        &self,
        key: &str,
        id: EntryId,
        fields: Vec<(String, String)>,
    ) -> Result<EntryId, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let entry = state
            .entries
            .entry(key.to_string())
            .or_insert_with(|| Entry::new(Data::Stream(Vec::new())));

        let Data::Stream(stream) = &mut entry.data else {
            return Err(WrongType);
        };

        stream.push(StreamEntry { id, fields });
        Ok(id)
    }
}
