use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The key-value store, shared by every connection.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<HashMap<String, Entry>>>);

impl Store {
    pub fn set(&self, key: String, value: String, expires_in: Option<Duration>) {
        let entry = Entry {
            value,
            expires_at: expires_in.map(|delay| Instant::now() + delay),
        };
        self.entries().insert(key, entry);
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let mut entries = self.entries();

        let entry = entries.get(key)?;
        if !entry.has_expired() {
            return Some(entry.value.clone());
        }

        // Redis expires keys lazily, so drop this one now that we noticed.
        entries.remove(key);
        None
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // A panic elsewhere poisons the lock but leaves the map intact, so
        // recover rather than taking down every other connection with it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct Entry {
    value: String,
    expires_at: Option<Instant>,
}

impl Entry {
    fn has_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}
