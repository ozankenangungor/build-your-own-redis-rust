use bytes::Bytes;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Who is listening on what, across the whole server.
///
/// Only the tally is kept, one number to a channel. What a client is to be told
/// is still the connection's own business; this is what a publisher asks to
/// learn how far its message reached.
#[derive(Clone, Default)]
pub struct Channels(Arc<Mutex<HashMap<Bytes, usize>>>);

impl Channels {
    /// Counts one more client as listening on this channel.
    pub fn joined(&self, channel: &Bytes) {
        *self.listeners().entry(channel.clone()).or_insert(0) += 1;
    }

    /// Counts one fewer. A channel nobody is left on is forgotten rather than
    /// kept at nothing, so that a server that has seen many channels come and
    /// go is no heavier for it.
    pub fn left(&self, channel: &Bytes) {
        let mut listeners = self.listeners();

        if let Some(count) = listeners.get_mut(channel) {
            *count -= 1;

            if *count == 0 {
                listeners.remove(channel);
            }
        }
    }

    /// How many clients are listening on this channel.
    pub fn listening_to(&self, channel: &Bytes) -> usize {
        self.listeners().get(channel).copied().unwrap_or(0)
    }

    fn listeners(&self) -> MutexGuard<'_, HashMap<Bytes, usize>> {
        // A panic elsewhere poisons the lock but leaves the tally intact, so
        // recover rather than taking down every other connection with it.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str) -> Bytes {
        Bytes::copy_from_slice(name.as_bytes())
    }

    #[test]
    fn counts_nobody_on_a_channel_nobody_has_asked_for() {
        assert_eq!(Channels::default().listening_to(&channel("foo")), 0);
    }

    #[test]
    fn counts_the_clients_as_they_arrive() {
        let channels = Channels::default();

        channels.joined(&channel("foo"));
        assert_eq!(channels.listening_to(&channel("foo")), 1);

        channels.joined(&channel("foo"));
        assert_eq!(channels.listening_to(&channel("foo")), 2);
    }

    #[test]
    fn counts_each_channel_apart_from_the_others() {
        let channels = Channels::default();

        channels.joined(&channel("foo"));
        channels.joined(&channel("bar"));
        channels.joined(&channel("bar"));

        assert_eq!(channels.listening_to(&channel("foo")), 1);
        assert_eq!(channels.listening_to(&channel("bar")), 2);
        assert_eq!(channels.listening_to(&channel("baz")), 0);
    }

    #[test]
    fn counts_the_clients_as_they_go() {
        let channels = Channels::default();

        channels.joined(&channel("foo"));
        channels.joined(&channel("foo"));
        channels.left(&channel("foo"));

        assert_eq!(channels.listening_to(&channel("foo")), 1);

        channels.left(&channel("foo"));
        assert_eq!(channels.listening_to(&channel("foo")), 0);
    }

    #[test]
    fn thinks_nothing_of_a_client_leaving_a_channel_it_was_never_on() {
        let channels = Channels::default();

        channels.left(&channel("foo"));

        assert_eq!(channels.listening_to(&channel("foo")), 0);
    }

    #[test]
    fn is_one_tally_however_many_hands_hold_it() {
        let channels = Channels::default();
        let shared = channels.clone();

        shared.joined(&channel("foo"));

        assert_eq!(channels.listening_to(&channel("foo")), 1);
    }
}
