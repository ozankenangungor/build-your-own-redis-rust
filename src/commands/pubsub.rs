use super::wrong_arity;
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
#[derive(Default)]
pub struct Subscriptions(Vec<Bytes>);

impl Subscriptions {
    /// Whether this client is listening on anything at all.
    ///
    /// A client that is has gone from asking a server questions to waiting on
    /// what others say, and most of what it could ask before is closed to it
    /// until it stops listening.
    pub fn listening(&self) -> bool {
        !self.0.is_empty()
    }

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

        assert!(run("GET", &[], &mut subscriptions).is_none());
    }
}
