use bytes::Bytes;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;

/// One client's end of the channels it listens on: what to hand a message to
/// for it to reach that client.
///
/// A listener is written to rather than waited on, so that a client slow to
/// read cannot hold up the one publishing. It is the listening client's own
/// connection that takes the message off this and writes it out.
pub type Listener = mpsc::UnboundedSender<Bytes>;

/// Who is listening on what, across the whole server.
#[derive(Clone, Default)]
pub struct Channels(Arc<Mutex<HashMap<Bytes, Vec<Listener>>>>);

impl Channels {
    /// Takes on a client as listening on this channel.
    pub fn joined(&self, channel: &Bytes, listener: &Listener) {
        self.listeners()
            .entry(channel.clone())
            .or_default()
            .push(listener.clone());
    }

    /// Lets a client go from a channel. A channel nobody is left on is forgotten
    /// rather than kept empty, so that a server that has seen many channels come
    /// and go is no heavier for it.
    pub fn left(&self, channel: &Bytes, listener: &Listener) {
        let mut listeners = self.listeners();

        let Some(on_channel) = listeners.get_mut(channel) else {
            return;
        };

        // Told apart by which queue they lead to, since that is the one thing a
        // listener is.
        on_channel.retain(|other| !other.same_channel(listener));

        if on_channel.is_empty() {
            listeners.remove(channel);
        }
    }

    /// Hands `delivery` to everyone listening on `channel`, and says how many
    /// that was.
    pub fn send(&self, channel: &Bytes, delivery: &Bytes) -> usize {
        let mut listeners = self.listeners();

        let Some(on_channel) = listeners.get_mut(channel) else {
            return 0;
        };

        // A send that fails is one whose connection has just ended. Those are
        // dropped here rather than counted, since a message reached them no
        // more than it reached anybody who was never there.
        on_channel.retain(|listener| listener.send(delivery.clone()).is_ok());

        let reached = on_channel.len();
        if reached == 0 {
            listeners.remove(channel);
        }

        reached
    }

    fn listeners(&self) -> MutexGuard<'_, HashMap<Bytes, Vec<Listener>>> {
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

    fn message(text: &str) -> Bytes {
        Bytes::copy_from_slice(text.as_bytes())
    }

    /// A client listening, along with the end its messages arrive on.
    fn client() -> (Listener, mpsc::UnboundedReceiver<Bytes>) {
        mpsc::unbounded_channel()
    }

    #[test]
    fn reaches_nobody_on_a_channel_nobody_has_asked_for() {
        let channels = Channels::default();

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 0);
    }

    #[test]
    fn reaches_the_client_listening_on_the_channel() {
        let channels = Channels::default();
        let (listener, mut heard) = client();

        channels.joined(&channel("foo"), &listener);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
        assert_eq!(heard.try_recv().unwrap(), message("hello"));
    }

    #[test]
    fn reaches_every_client_listening_on_the_channel() {
        let channels = Channels::default();
        let (mine, mut i_hear) = client();
        let (yours, mut you_hear) = client();

        channels.joined(&channel("foo"), &mine);
        channels.joined(&channel("foo"), &yours);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 2);
        assert_eq!(i_hear.try_recv().unwrap(), message("hello"));
        assert_eq!(you_hear.try_recv().unwrap(), message("hello"));
    }

    #[test]
    fn reaches_only_the_channel_it_was_sent_to() {
        let channels = Channels::default();
        let (on_foo, mut foo_hears) = client();
        let (on_bar, mut bar_hears) = client();

        channels.joined(&channel("foo"), &on_foo);
        channels.joined(&channel("bar"), &on_bar);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
        assert_eq!(foo_hears.try_recv().unwrap(), message("hello"));
        assert!(bar_hears.try_recv().is_err());
    }

    #[test]
    fn reaches_one_client_on_each_of_the_channels_it_listens_on() {
        let channels = Channels::default();
        let (listener, mut heard) = client();

        channels.joined(&channel("foo"), &listener);
        channels.joined(&channel("bar"), &listener);

        assert_eq!(channels.send(&channel("foo"), &message("one")), 1);
        assert_eq!(channels.send(&channel("bar"), &message("two")), 1);

        assert_eq!(heard.try_recv().unwrap(), message("one"));
        assert_eq!(heard.try_recv().unwrap(), message("two"));
    }

    #[test]
    fn keeps_the_messages_in_the_order_they_were_sent() {
        let channels = Channels::default();
        let (listener, mut heard) = client();

        channels.joined(&channel("foo"), &listener);

        for text in ["one", "two", "three"] {
            channels.send(&channel("foo"), &message(text));
        }

        for text in ["one", "two", "three"] {
            assert_eq!(heard.try_recv().unwrap(), message(text));
        }
    }

    #[test]
    fn stops_reaching_a_client_that_has_left_the_channel() {
        let channels = Channels::default();
        let (staying, _stays) = client();
        let (leaving, mut left) = client();

        channels.joined(&channel("foo"), &staying);
        channels.joined(&channel("foo"), &leaving);
        channels.left(&channel("foo"), &leaving);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
        assert!(left.try_recv().is_err());
    }

    #[test]
    fn lets_one_client_go_and_leaves_the_others_where_they_were() {
        let channels = Channels::default();
        let (mine, _i_hear) = client();
        let (yours, _you_hear) = client();

        channels.joined(&channel("foo"), &mine);
        channels.joined(&channel("foo"), &yours);
        channels.left(&channel("foo"), &mine);
        channels.left(&channel("foo"), &mine);

        // Letting go of a client twice lets go of it once: the second time
        // there is nothing of it left to find.
        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
    }

    #[test]
    fn thinks_nothing_of_a_client_leaving_a_channel_it_was_never_on() {
        let channels = Channels::default();
        let (listener, _heard) = client();

        channels.left(&channel("foo"), &listener);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 0);
    }

    #[test]
    fn stops_reaching_a_client_that_has_gone() {
        let channels = Channels::default();
        let (staying, _stays) = client();
        let (leaving, gone) = client();

        channels.joined(&channel("foo"), &staying);
        channels.joined(&channel("foo"), &leaving);
        drop(gone);

        // A client whose connection has ended is reached no further than one
        // who was never listening.
        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
    }

    #[test]
    fn is_one_listing_however_many_hands_hold_it() {
        let channels = Channels::default();
        let shared = channels.clone();
        let (listener, _heard) = client();

        shared.joined(&channel("foo"), &listener);

        assert_eq!(channels.send(&channel("foo"), &message("hello")), 1);
    }
}
