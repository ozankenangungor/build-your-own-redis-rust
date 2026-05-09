use super::{Data, Entry, Store, WrongType, drop_if_expired};
use std::time::{Duration, Instant};

impl Store {
    pub fn set(&self, key: String, value: String, expires_in: Option<Duration>) {
        let entry = Entry {
            data: Data::String(value),
            expires_at: expires_in.map(|delay| Instant::now() + delay),
        };
        self.state().entries.insert(key, entry);
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, WrongType> {
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
