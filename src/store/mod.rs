mod lists;
mod sorted_sets;
mod streams;
mod strings;

pub use lists::{Blocked, Side};
pub use streams::{EntryId, ReadStream, RequestedId, StreamEntry, StreamRead, XaddError};
pub use strings::IncrementError;

use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use tokio::sync::{Notify, oneshot};

/// The key-value store, shared by every connection.
///
/// This module owns the pieces every data type needs — the lock, the entry
/// table and expiry — while the command families live in the modules beside it.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<State>>);

/// Returned when a command is used on a key holding another type of value.
#[derive(Debug, PartialEq)]
pub struct WrongType;

/// The kind of value a key holds, or `None` when there is no such key.
#[derive(Clone, Copy)]
pub enum Kind {
    None,
    String,
    List,
    Stream,
    SortedSet,
}

#[derive(Default)]
struct State {
    entries: Entries,
    /// Clients blocked on a key, in the order they started waiting. Only the
    /// list commands use these, but they have to share the lock with `entries`.
    waiters: HashMap<Bytes, VecDeque<oneshot::Sender<Bytes>>>,
    /// Clients waiting for a stream to grow. Every one of them is woken, since
    /// reading an entry does not take it away from the others.
    watchers: HashMap<Bytes, Vec<Arc<Notify>>>,
    /// Counts the changes made to any key. Versions are only ever compared for
    /// equality, so all that matters is that no number is handed out twice.
    changes: u64,
}

type Entries = HashMap<Bytes, Entry>;

impl Store {
    /// Reports what `key` holds. This is the one lookup that never fails on a
    /// type mismatch, since telling them apart is the whole point.
    pub fn kind(&self, key: &Bytes) -> Kind {
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
            Some(Entry {
                data: Data::SortedSet(_),
                ..
            }) => Kind::SortedSet,
        }
    }

    /// The keys this store holds that `wanted` asks for.
    ///
    /// Every key is looked at either way, which makes this the moment to be rid
    /// of the ones whose time has passed: a key that has expired is no longer
    /// there to be listed.
    pub fn keys(&self, wanted: impl Fn(&Bytes) -> bool) -> Vec<Bytes> {
        let mut state = self.state();
        state.entries.retain(|_, entry| !entry.has_expired());

        state
            .entries
            .keys()
            .filter(|key| wanted(key))
            .cloned()
            .collect()
    }

    /// The versions these keys hold now, for a client that wants to be told
    /// should any of them change. `None` means there is no such key, which is a
    /// state of its own: a key that appears later has changed just as surely as
    /// one rewritten.
    ///
    /// They are read together under one lock, so what comes back is one moment
    /// rather than several, just as [`Store::unchanged`] checks one moment.
    pub fn versions(&self, keys: &[Bytes]) -> Vec<Option<u64>> {
        let mut state = self.state();

        keys.iter()
            .map(|key| {
                drop_if_expired(&mut state.entries, key);
                state.entries.get(key).map(|entry| entry.version)
            })
            .collect()
    }

    /// Whether every one of these keys still holds the version it did when it
    /// was looked at. They are checked together under one lock, so no write can
    /// slip between two of them.
    ///
    /// A version is carried by the entry itself, which leaves one change
    /// invisible: a key that was missing when it was watched, then made and
    /// unmade again, is missing once more and so reads as untouched. Catching
    /// that would mean keeping a record of every key ever deleted, or having
    /// each write seek out the clients watching it, and neither is worth what
    /// it costs here.
    pub fn unchanged<'a>(
        &self,
        watched: impl IntoIterator<Item = (&'a Bytes, &'a Option<u64>)>,
    ) -> bool {
        let mut state = self.state();

        watched.into_iter().all(|(key, version)| {
            drop_if_expired(&mut state.entries, key);
            state.entries.get(key).map(|entry| entry.version) == *version
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A panic elsewhere poisons the lock but leaves the state intact, so
        // recover rather than taking down every other connection with it.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// The number to stamp the next change with.
    ///
    /// One counter serves every key, so a key that is deleted and made again
    /// never comes back wearing a version a client saw before.
    fn next_version(&mut self) -> u64 {
        self.changes += 1;
        self.changes
    }
}

struct Entry {
    data: Data,
    expires_at: Option<Instant>,
    /// When this entry was last changed, so that a client watching the key can
    /// tell whether anything happened to it while it was not looking.
    version: u64,
}

enum Data {
    String(Bytes),
    /// Lists are pushed to and popped from both ends, which is what a `VecDeque`
    /// is for: a `Vec` would shift every element on each `LPUSH` and `LPOP`.
    List(VecDeque<Bytes>),
    Stream(Vec<streams::StreamEntry>),
    /// Members held in the order of their scores, so that the ones asked for
    /// most — by where they fall — are found without a search.
    SortedSet(sorted_sets::SortedSet),
}

impl Entry {
    fn new(data: Data, version: u64) -> Self {
        Self {
            data,
            expires_at: None,
            version,
        }
    }

    fn has_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

/// Turns an index into an offset from the start. Negative indexes count back
/// from the end, and one reaching past the start clamps to the first element
/// rather than wrapping around.
///
/// Lists and sorted sets are both read off by where their elements fall, and
/// Redis counts them the same way.
fn resolve_index(index: i64, len: usize) -> usize {
    if index >= 0 {
        index as usize
    } else {
        len.saturating_sub(index.unsigned_abs() as usize)
    }
}

/// Redis expires keys lazily, so drop this one now that we are looking at it.
fn drop_if_expired(entries: &mut Entries, key: &Bytes) {
    if entries.get(key).is_some_and(Entry::has_expired) {
        entries.remove(key);
    }
}
