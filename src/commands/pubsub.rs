use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// The channels one client is listening on.
///
/// This belongs to the connection rather than to the server: subscribing is
/// something a client does, and it lasts exactly as long as the client does.
#[derive(Default)]
pub struct Subscriptions(Vec<Bytes>);

impl Subscriptions {
    /// Takes on a channel and says how many this client is then listening on.
    ///
    /// Subscribing twice to the one channel leaves it listening once, as it was
    /// already: Redis counts channels, not the asking.
    fn add(&mut self, channel: &Bytes) -> usize {
        if !self.0.contains(channel) {
            self.0.push(channel.clone());
        }

        self.0.len()
    }
}

/// Handles the commands a client uses to listen for what others have to say.
/// `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], subscriptions: &mut Subscriptions) -> Option<Value> {
    let reply = match command {
        // Each channel is confirmed on its own, in the order it was named, and
        // the count climbs as they are taken on one by one.
        "SUBSCRIBE" => match args {
            [] => wrong_arity("subscribe"),
            channels => Value::Sequence(
                channels
                    .iter()
                    .map(|channel| listening(channel, subscriptions.add(channel)))
                    .collect(),
            ),
        },
        _ => return None,
    };

    Some(reply)
}

/// What a client is told when it has been put on a channel: what happened, the
/// channel it happened to, and how many it is listening on all told.
fn listening(channel: &Bytes, count: usize) -> Value {
    Value::Array(vec![
        Value::BulkString(Bytes::from_static(b"subscribe")),
        Value::BulkString(channel.clone()),
        Value::Integer(count as i64),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscribe(channels: &[&str], subscriptions: &mut Subscriptions) -> Value {
        let channels: Vec<Bytes> = channels
            .iter()
            .map(|channel| Bytes::copy_from_slice(channel.as_bytes()))
            .collect();

        run("SUBSCRIBE", &channels, subscriptions).expect("subscribe belongs to this module")
    }

    #[test]
    fn confirms_the_channel_it_was_given() {
        let mut subscriptions = Subscriptions::default();

        assert_eq!(
            subscribe(&["foo"], &mut subscriptions).encode(),
            b"*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n"
        );
    }

    #[test]
    fn counts_the_channels_as_they_are_taken_on() {
        let mut subscriptions = Subscriptions::default();

        subscribe(&["foo"], &mut subscriptions);
        assert_eq!(
            subscribe(&["bar"], &mut subscriptions).encode(),
            b"*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n"
        );
    }

    #[test]
    fn confirms_each_of_the_channels_it_was_given_in_turn() {
        let mut subscriptions = Subscriptions::default();

        assert_eq!(
            subscribe(&["foo", "bar"], &mut subscriptions).encode(),
            b"*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n\
              *3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n"
        );
    }

    #[test]
    fn leaves_the_count_where_it_was_on_a_channel_it_is_already_on() {
        let mut subscriptions = Subscriptions::default();

        subscribe(&["foo"], &mut subscriptions);
        assert_eq!(
            subscribe(&["foo"], &mut subscriptions).encode(),
            b"*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n"
        );
    }

    #[test]
    fn counts_the_channels_of_one_client_and_not_another() {
        let mut mine = Subscriptions::default();
        let mut yours = Subscriptions::default();

        subscribe(&["foo"], &mut mine);
        subscribe(&["bar"], &mut mine);

        // What one client listens on is no business of the next.
        assert_eq!(
            subscribe(&["baz"], &mut yours).encode(),
            b"*3\r\n$9\r\nsubscribe\r\n$3\r\nbaz\r\n:1\r\n"
        );
    }

    #[test]
    fn refuses_a_subscribe_that_names_no_channel() {
        let mut subscriptions = Subscriptions::default();

        assert_eq!(
            subscribe(&[], &mut subscriptions),
            Value::Error("ERR wrong number of arguments for 'subscribe' command".into())
        );
    }

    #[test]
    fn leaves_alone_the_commands_that_are_not_its_own() {
        let mut subscriptions = Subscriptions::default();

        assert!(run("GET", &[], &mut subscriptions).is_none());
    }
}
