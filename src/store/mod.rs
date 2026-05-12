mod lists;
mod streams;
mod strings;

pub use lists::{Blocked, Side};
pub use streams::{RequestedId, XaddError};

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

/// The kind of value a key holds, or `None` when there is no such key.
pub enum Kind {
    None,
    String,
    List,
    Stream,
}

#[derive(Default)]
struct State {
    entries: Entries,
    /// Clients blocked on a key, in the order they started waiting. Only the
    /// list commands use these, but they have to share the lock with `entries`.
    waiters: HashMap<String, VecDeque<oneshot::Sender<String>>>,
}

type Entries = HashMap<String, Entry>;

impl Store {
    /// Reports what `key` holds. This is the one lookup that never fails on a
    /// type mismatch, since telling them apart is the whole point.
    pub fn kind(&self, key: &str) -> Kind {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        match state.entries.get(key) {
            None => Kind::None,
            Some(Entry {
                data: Data::String(_),
                ..
            }) => Kind::String,
            Some(Entry {
                data: Data::List(_),
                ..
            }) => Kind::List,
            Some(Entry {
                data: Data::Stream(_),
                ..
            }) => Kind::Stream,
        }
    }

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
    Stream(Vec<streams::StreamEntry>),
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
