use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The key-value store, shared by every connection.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<HashMap<String, Entry>>>);

/// Returned when a command is used on a key holding another type of value.
pub struct WrongType;

impl Store {
    pub fn set(&self, key: String, value: String, expires_in: Option<Duration>) {
        let entry = Entry {
            data: Data::String(value),
            expires_at: expires_in.map(|delay| Instant::now() + delay),
        };
        self.entries().insert(key, entry);
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, WrongType> {
        let mut entries = self.entries();
        drop_if_expired(&mut entries, key);

        match entries.get(key) {
            None => Ok(None),
            Some(Entry {
                data: Data::String(value),
                ..
            }) => Ok(Some(value.clone())),
            Some(_) => Err(WrongType),
        }
    }

    /// Appends to the list at `key`, creating it first if it does not exist,
    /// and returns the list's new length.
    pub fn rpush(&self, key: &str, elements: &[String]) -> Result<usize, WrongType> {
        let mut entries = self.entries();
        drop_if_expired(&mut entries, key);

        let entry = entries
            .entry(key.to_string())
            .or_insert_with(|| Entry::new(Data::List(Vec::new())));

        let Data::List(list) = &mut entry.data else {
            return Err(WrongType);
        };

        list.extend_from_slice(elements);
        Ok(list.len())
    }

    /// Returns the elements of the list at `key` between `start` and `stop`,
    /// both inclusive. A window that falls outside the list is not an error: it
    /// is clamped, and yields fewer elements or none at all.
    pub fn lrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>, WrongType> {
        let mut entries = self.entries();
        drop_if_expired(&mut entries, key);

        let list = match entries.get(key) {
            None => return Ok(Vec::new()),
            Some(Entry {
                data: Data::List(list),
                ..
            }) => list,
            Some(_) => return Err(WrongType),
        };

        // Negative indexes count from the end of the list; that comes later.
        let start = start.max(0) as usize;
        let stop = stop.max(0) as usize;

        if start > stop || start >= list.len() {
            return Ok(Vec::new());
        }

        let stop = stop.min(list.len() - 1);
        Ok(list[start..=stop].to_vec())
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // A panic elsewhere poisons the lock but leaves the map intact, so
        // recover rather than taking down every other connection with it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Redis expires keys lazily, so drop this one now that we are looking at it.
fn drop_if_expired(entries: &mut HashMap<String, Entry>, key: &str) {
    if entries.get(key).is_some_and(Entry::has_expired) {
        entries.remove(key);
    }
}

pub struct Entry {
    data: Data,
    expires_at: Option<Instant>,
}

enum Data {
    String(String),
    List(Vec<String>),
}

impl Entry {
    fn new(data: Data) -> Self {
        Self {
            data,
            expires_at: None,
        }
    }

    fn has_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}
