use super::{Data, Entries, Entry, State, Store, WrongType, drop_if_expired};
use bytes::Bytes;
use tokio::sync::oneshot;

/// The end of a list a command works from.
pub enum Side {
    Left,
    Right,
}

/// The outcome of a blocking pop: either an element was there, or the caller
/// has been queued and will be handed one as soon as it arrives.
pub enum Blocked {
    Ready(Bytes),
    Waiting(oneshot::Receiver<Bytes>),
}

impl Store {
    /// Adds to the list at `key`, creating it first if it does not exist, and
    /// returns the list's new length.
    pub fn push(&self, key: &Bytes, elements: &[Bytes], side: Side) -> Result<usize, WrongType> {
        let mut guard = self.state();
        // Borrow the state once so `entries` and `waiters` count as separate
        // fields rather than two borrows of the guard.
        let state = &mut *guard;
        drop_if_expired(&mut state.entries, key);

        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| Entry::new(Data::List(Vec::new())));

        let Data::List(list) = &mut entry.data else {
            return Err(WrongType);
        };

        match side {
            Side::Right => list.extend_from_slice(elements),
            // Each element lands in front of the one before it, so a single
            // push reverses the order of its arguments.
            Side::Left => {
                list.splice(0..0, elements.iter().rev().cloned());
            }
        }

        // Redis answers the pushing client with the length before handing
        // anything to blocked clients, so measure it now.
        let length = list.len();
        state.wake_waiters(key);

        Ok(length)
    }

    /// Removes and returns up to `count` elements from the front of the list at
    /// `key`. Redis drops a key once its list runs empty, so we do the same.
    pub fn lpop(&self, key: &Bytes, count: usize) -> Result<Vec<Bytes>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let Some(entry) = state.entries.get_mut(key) else {
            return Ok(Vec::new());
        };
        let Data::List(list) = &mut entry.data else {
            return Err(WrongType);
        };

        let removed = list.drain(..count.min(list.len())).collect();
        if list.is_empty() {
            state.entries.remove(key);
        }

        Ok(removed)
    }

    /// Takes the first element of the list at `key`, or puts the caller at the
    /// back of the queue of clients waiting for one.
    pub fn blpop(&self, key: &Bytes) -> Result<Blocked, WrongType> {
        let mut guard = self.state();
        let state = &mut *guard;
        drop_if_expired(&mut state.entries, key);

        match state.entries.get(key) {
            Some(Entry {
                data: Data::List(_),
                ..
            }) => {}
            Some(_) => return Err(WrongType),
            None => {
                let (sender, receiver) = oneshot::channel();

                let queue = state.waiters.entry(key.clone()).or_default();
                // Clients that timed out or hung up leave their end behind, and
                // nothing else clears them until the key is pushed to.
                queue.retain(|waiter| !waiter.is_closed());
                queue.push_back(sender);

                return Ok(Blocked::Waiting(receiver));
            }
        }

        let element = pop_first(&mut state.entries, key).expect("a stored list is never empty");
        Ok(Blocked::Ready(element))
    }

    /// Returns the elements of the list at `key` between `start` and `stop`,
    /// both inclusive. A window that falls outside the list is not an error: it
    /// is clamped, and yields fewer elements or none at all.
    pub fn lrange(&self, key: &Bytes, start: i64, stop: i64) -> Result<Vec<Bytes>, WrongType> {
        let mut state = self.state();
        let Some(list) = list_at(&mut state.entries, key)? else {
            return Ok(Vec::new());
        };

        let start = resolve_index(start, list.len());
        let stop = resolve_index(stop, list.len());

        if start > stop || start >= list.len() {
            return Ok(Vec::new());
        }

        let stop = stop.min(list.len() - 1);
        Ok(list[start..=stop].to_vec())
    }

    /// Returns the length of the list at `key`, or zero if it does not exist.
    pub fn llen(&self, key: &Bytes) -> Result<usize, WrongType> {
        let mut state = self.state();
        Ok(list_at(&mut state.entries, key)?.map_or(0, Vec::len))
    }
}

impl State {
    /// Hands the elements at `key` to the clients blocked on it, serving the
    /// one that has been waiting the longest first.
    fn wake_waiters(&mut self, key: &Bytes) {
        let Some(queue) = self.waiters.get_mut(key) else {
            return;
        };

        while let Some(waiter) = queue.pop_front() {
            let Some(element) = pop_first(&mut self.entries, key) else {
                queue.push_front(waiter);
                break;
            };

            // A client that gave up in the meantime leaves its element behind
            // for whoever is next in line.
            if let Err(element) = waiter.send(element) {
                push_first(&mut self.entries, key, element);
            }
        }

        if queue.is_empty() {
            self.waiters.remove(key);
        }
    }
}

/// Looks up the list stored at `key`. `Ok(None)` means the key is absent, which
/// list commands treat as an empty list rather than an error.
fn list_at<'a>(entries: &'a mut Entries, key: &Bytes) -> Result<Option<&'a Vec<Bytes>>, WrongType> {
    drop_if_expired(entries, key);

    match entries.get(key) {
        None => Ok(None),
        Some(Entry {
            data: Data::List(list),
            ..
        }) => Ok(Some(list)),
        Some(_) => Err(WrongType),
    }
}

fn pop_first(entries: &mut Entries, key: &Bytes) -> Option<Bytes> {
    let Some(Entry {
        data: Data::List(list),
        ..
    }) = entries.get_mut(key)
    else {
        return None;
    };

    let element = list.remove(0);
    if list.is_empty() {
        entries.remove(key);
    }

    Some(element)
}

fn push_first(entries: &mut Entries, key: &Bytes, element: Bytes) {
    let entry = entries
        .entry(key.clone())
        .or_insert_with(|| Entry::new(Data::List(Vec::new())));

    if let Data::List(list) = &mut entry.data {
        list.insert(0, element);
    }
}

/// Turns a list index into an offset from the start. Negative indexes count
/// back from the end, and one reaching past the start clamps to the first
/// element rather than wrapping around.
fn resolve_index(index: i64, len: usize) -> usize {
    if index >= 0 {
        index as usize
    } else {
        len.saturating_sub(index.unsigned_abs() as usize)
    }
}
