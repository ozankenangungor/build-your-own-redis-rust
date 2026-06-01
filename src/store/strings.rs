use super::{Data, Entry, Store, WrongType, drop_if_expired};
use bytes::Bytes;
use std::time::{Duration, Instant};

impl Store {
    pub fn set(&self, key: Bytes, value: Bytes, expires_in: Option<Duration>) {
        let entry = Entry {
            data: Data::String(value),
            expires_at: expires_in.map(|delay| Instant::now() + delay),
        };
        self.state().entries.insert(key, entry);
    }

    pub fn get(&self, key: &Bytes) -> Result<Option<Bytes>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        match state.entries.get(key) {
            None => Ok(None),
            Some(Entry {
                data: Data::String(value),
                ..
            }) => Ok(Some(value.clone())),
            Some(_) => Err(WrongType),
        }
    }

    /// Adds one to the number held at `key`, storing and returning the result.
    /// A key that is not there counts as zero, so counting up from nothing
    /// leaves a one behind.
    pub fn increment(&self, key: &Bytes) -> Result<i64, IncrementError> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| Entry::new(Data::String(Bytes::from_static(b"0"))));

        let Data::String(value) = &mut entry.data else {
            return Err(IncrementError::WrongType);
        };

        let number = str::from_utf8(value)
            .ok()
            .and_then(|number| number.parse::<i64>().ok())
            .ok_or(IncrementError::NotAnInteger)?;
        let incremented = number.checked_add(1).ok_or(IncrementError::Overflow)?;

        // Writing through the entry leaves whatever expiry it carries in place,
        // since counting up is not the same as setting the key anew.
        *value = Bytes::from(incremented.to_string());
        Ok(incremented)
    }
}

/// Why an increment could not go through.
pub enum IncrementError {
    NotAnInteger,
    Overflow,
    WrongType,
}
