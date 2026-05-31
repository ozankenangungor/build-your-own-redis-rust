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
}
