mod lists;
mod strings;

pub use lists::{Blocked, Side};

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::oneshot;

/// The key-value store, shared by every connection.
///
/// This module owns the pieces every data type needs — the lock, the entry
/// table and expiry — while the command families live in the modules beside it.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<State>>);

/// Returned when a command is used on a key holding another type of value.
pub struct WrongType;

#[derive(Default)]
struct State {
    entries: Entries,
    /// Clients blocked on a key, in the order they started waiting. Only the
    /// list commands use these, but they have to share the lock with `entries`.
    waiters: HashMap<String, VecDeque<oneshot::Sender<String>>>,
}

type Entries = HashMap<String, Entry>;

impl Store {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panic elsewhere poisons the lock but leaves the state intact, so
        // recover rather than taking down every other connection with it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct Entry {
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

/// Redis expires keys lazily, so drop this one now that we are looking at it.
fn drop_if_expired(entries: &mut Entries, key: &str) {
    if entries.get(key).is_some_and(Entry::has_expired) {
        entries.remove(key);
    }
}
