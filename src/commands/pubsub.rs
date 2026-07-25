use super::wrong_arity;
use crate::channels::Channels;
use crate::resp::Value;
use bytes::Bytes;

/// The commands a client may still use once it is listening on a channel.
///
/// The list is Redis's, and includes commands this server has yet to learn:
/// what a listening client may ask for does not depend on which of them have
/// been written, and one it does not know will say so for itself.
const ALLOWED_WHILE_LISTENING: &[&str] = &[
    "SUBSCRIBE",
    "UNSUBSCRIBE",
    "PSUBSCRIBE",
    "PUNSUBSCRIBE",
    "PING",
    "QUIT",
    "RESET",
];

/// The channels one client is listening on.
///
/// This belongs to the connection rather than to the server: subscribing is
/// something a client does, and it lasts exactly as long as the client does.
/// The server's own tally is kept up to date from here, arriving and leaving
/// alike, so that what a publisher is told is what is really there.
#[derive(Default)]
pub struct Subscriptions {
    on: Vec<Bytes>,
    channels: Channels,
}

impl Subscriptions {
    /// The channels of a client on this server.
    pub fn of(channels: Channels) -> Self {
        Self {
            on: Vec::new(),
            channels,
        }
    }

    /// Whether this client is listening on anything at all.
    ///
    /// A client that is has gone from asking a server questions to waiting on
    /// what others say, and most of what it could ask before is closed to it
    /// until it stops listening.
    pub fn listening(&self) -> bool {
        !self.on.is_empty()
    }

    /// Takes on a channel and says how many this client is then listening on.
    ///
    /// Subscribing twice to the one channel leaves it listening once, as it was
    /// already: Redis counts channels, not the asking.
    fn add(&mut self, channel: &Bytes) -> usize {
        if !self.on.contains(channel) {
            self.on.push(channel.clone());
            self.channels.joined(channel);
        }

        self.on.len()
    }
}

impl Drop for Subscriptions {
    fn drop(&mut self) {
        // A client that has gone is listening to nothing, whether it said so or
        // simply hung up.
        for channel in &self.on {
            self.channels.left(channel);
        }
    }
}

/// Handles the commands a client uses to listen for what others have to say,
/// and to say something itself. `None` means the command belongs to another
/// module.
pub fn run(
    command: &str,
    args: &[Bytes],
    subscriptions: &mut Subscriptions,
    channels: &Channels,
) -> Option<Value> {
    let reply = match command {
        // How far the message reached, counted as it goes out. Carrying it to
        // those listening is the connection's own work, and still to come.
        "PUBLISH" => match args {
            [channel, _message] => Value::Integer(channels.listening_to(channel) as i64),
            _ => wrong_arity("publish"),
        },
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
        // A listening client hears back in the shape everything else on the
        // connection arrives in, so that one reader can make sense of them all.
        // A client that is not listening is left to the module that has always
        // answered it.
        "PING" if subscriptions.listening() => Value::Array(vec![
            Value::BulkString(Bytes::from_static(b"pong")),
            Value::BulkString(Bytes::new()),
        ]),
        _ => return None,
    };

    Some(reply)
}

/// Whether a client listening on a channel may still use this command.
pub fn allowed_while_listening(command: &str) -> bool {
    ALLOWED_WHILE_LISTENING.contains(&command)
}

/// What a listening client is told when it asks for something it may not have
/// while it listens.
///
/// The command is named as Redis names it rather than as it was spelled, since
/// what is being refused is the command, not the asking.
pub fn out_of_context(command: &str) -> Value {
    Value::Error(format!(
        "ERR Can't execute '{}': only (P|S)SUBSCRIBE / (P|S)UNSUBSCRIBE / PING / QUIT / RESET are allowed in this context",
        command.to_lowercase()
    ))
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

    fn named(channel: &str) -> Bytes {
        Bytes::copy_from_slice(channel.as_bytes())
    }

    fn subscribe(channels: &[&str], subscriptions: &mut Subscriptions) -> Value {
        let named: Vec<Bytes> = channels.iter().copied().map(named).collect();
        let listening = subscriptions.channels.clone();

        run("SUBSCRIBE", &named, subscriptions, &listening)
            .expect("subscribe belongs to this module")
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

    fn publish(channel: &str, channels: &Channels) -> Value {
        let mut nobody = Subscriptions::default();
        let args = [named(channel), named("hello")];

        run("PUBLISH", &args, &mut nobody, channels).expect("publish belongs to this module")
    }

    #[test]
    fn tells_a_publisher_how_many_are_listening() {
        let channels = Channels::default();
        let mut mine = Subscriptions::of(channels.clone());
        let mut yours = Subscriptions::of(channels.clone());

        assert_eq!(publish("foo", &channels), Value::Integer(0));

        subscribe(&["foo"], &mut mine);
        assert_eq!(publish("foo", &channels), Value::Integer(1));

        subscribe(&["foo"], &mut yours);
        assert_eq!(publish("foo", &channels), Value::Integer(2));
    }

    #[test]
    fn counts_only_those_listening_on_the_channel_published_to() {
        let channels = Channels::default();
        let mut client = Subscriptions::of(channels.clone());

        subscribe(&["foo", "bar"], &mut client);

        assert_eq!(publish("foo", &channels), Value::Integer(1));
        assert_eq!(publish("bar", &channels), Value::Integer(1));
        assert_eq!(publish("baz", &channels), Value::Integer(0));
    }

    #[test]
    fn counts_a_client_on_a_channel_once_however_often_it_asked() {
        let channels = Channels::default();
        let mut client = Subscriptions::of(channels.clone());

        subscribe(&["foo"], &mut client);
        subscribe(&["foo"], &mut client);

        assert_eq!(publish("foo", &channels), Value::Integer(1));
    }

    #[test]
    fn stops_counting_a_client_that_has_gone() {
        let channels = Channels::default();
        let mut staying = Subscriptions::of(channels.clone());

        subscribe(&["foo"], &mut staying);
        {
            let mut leaving = Subscriptions::of(channels.clone());
            subscribe(&["foo"], &mut leaving);

            assert_eq!(publish("foo", &channels), Value::Integer(2));
        }

        // A client that has hung up is listening to nothing, and a publisher
        // told otherwise would be told a message reached further than it did.
        assert_eq!(publish("foo", &channels), Value::Integer(1));
    }

    #[test]
    fn refuses_a_publish_that_is_missing_a_channel_or_a_message() {
        let channels = Channels::default();
        let mut nobody = Subscriptions::default();

        for args in [
            vec![],
            vec![named("foo")],
            vec![named("a"), named("b"), named("c")],
        ] {
            assert_eq!(
                run("PUBLISH", &args, &mut nobody, &channels),
                Some(Value::Error(
                    "ERR wrong number of arguments for 'publish' command".into()
                )),
                "{args:?}"
            );
        }
    }

    #[test]
    fn leaves_a_ping_from_a_client_that_is_not_listening_to_another_module() {
        let mut subscriptions = Subscriptions::default();

        assert!(run("PING", &[], &mut subscriptions, &Channels::default()).is_none());
    }

    #[test]
    fn answers_a_ping_from_a_listening_client_the_way_it_hears_everything_else() {
        let mut subscriptions = Subscriptions::default();
        subscribe(&["foo"], &mut subscriptions);

        let reply = run("PING", &[], &mut subscriptions, &Channels::default())
            .expect("a listening client is answered");

        assert_eq!(reply.encode(), b"*2\r\n$4\r\npong\r\n$0\r\n\r\n");
    }

    #[test]
    fn is_listening_to_nothing_until_it_is_asked_to_listen() {
        let mut subscriptions = Subscriptions::default();

        assert!(!subscriptions.listening());

        subscribe(&["foo"], &mut subscriptions);
        assert!(subscriptions.listening());
    }

    #[test]
    fn leaves_open_the_commands_that_steer_the_listening() {
        for command in [
            "SUBSCRIBE",
            "UNSUBSCRIBE",
            "PSUBSCRIBE",
            "PUNSUBSCRIBE",
            "PING",
            "QUIT",
            "RESET",
        ] {
            assert!(allowed_while_listening(command), "{command}");
        }
    }

    #[test]
    fn closes_off_the_commands_that_ask_the_server_something() {
        for command in ["GET", "SET", "ECHO", "MULTI", "EXEC", "KEYS", "PUBLISH"] {
            assert!(!allowed_while_listening(command), "{command}");
        }
    }

    #[test]
    fn names_the_command_it_will_not_run_the_way_redis_names_it() {
        let Value::Error(said) = out_of_context("ECHO") else {
            panic!("a refusal is an error");
        };

        assert!(said.starts_with("ERR Can't execute 'echo': "), "{said:?}");
        assert!(said.ends_with("are allowed in this context"), "{said:?}");
    }

    #[test]
    fn leaves_alone_the_commands_that_are_not_its_own() {
        let mut subscriptions = Subscriptions::default();

        assert!(run("GET", &[], &mut subscriptions, &Channels::default()).is_none());
    }
}
