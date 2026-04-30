use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// The key-value store, shared by every connection.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<HashMap<String, String>>>);

impl Store {
    pub fn set(&self, key: String, value: String) {
        self.entries().insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.entries().get(key).cloned()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, String>> {
        // A panic elsewhere poisons the lock but leaves the map intact, so
        // recover rather than taking down every other connection with it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
