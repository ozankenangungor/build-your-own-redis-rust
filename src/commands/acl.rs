use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// The user a connection is taken to be until it says otherwise.
///
/// Redis gives every new connection this name. Whether it has to prove itself
/// before it is believed is another matter, and one still to come.
const DEFAULT_USER: &str = "default";

/// Handles the commands that ask who a client is and what it may do. `None`
/// means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes]) -> Option<Value> {
    let reply = match command {
        // `ACL` covers several commands in one, told apart by the word after it.
        "ACL" => match args.split_first() {
            Some((action, rest)) if action.eq_ignore_ascii_case(b"WHOAMI") => match rest {
                [] => Value::BulkString(Bytes::from_static(DEFAULT_USER.as_bytes())),
                _ => unknown_action(action),
            },
            Some((action, _)) => unknown_action(action),
            None => wrong_arity("acl"),
        },
        _ => return None,
    };

    Some(reply)
}

/// What a client is told when it asks `ACL` something this server has no answer
/// for. Redis says the same of a word it does not know and of one it knows but
/// was handed wrongly, so the two are not told apart here either.
fn unknown_action(action: &[u8]) -> Value {
    Value::Error(format!(
        "ERR Unknown ACL subcommand or wrong number of arguments for '{}'",
        String::from_utf8_lossy(action)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acl(words: &[&str]) -> Value {
        let args: Vec<Bytes> = words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect();

        run("ACL", &args).expect("acl belongs to this module")
    }

    #[test]
    fn says_who_the_client_is() {
        assert_eq!(
            acl(&["WHOAMI"]),
            Value::BulkString(Bytes::from_static(b"default"))
        );
    }

    #[test]
    fn takes_the_word_after_it_however_it_is_spelled() {
        for spelling in ["WHOAMI", "whoami", "WhoAmI"] {
            assert_eq!(
                acl(&[spelling]),
                Value::BulkString(Bytes::from_static(b"default")),
                "{spelling}"
            );
        }
    }

    #[test]
    fn refuses_an_acl_that_says_nothing() {
        assert_eq!(acl(&[]), wrong_arity("acl"));
    }

    #[test]
    fn refuses_a_word_it_has_no_answer_for() {
        assert_eq!(acl(&["NONSENSE"]), unknown_action(b"NONSENSE"));
        assert_eq!(acl(&["WHOAMI", "extra"]), unknown_action(b"WHOAMI"));
    }

    #[test]
    fn leaves_alone_the_commands_that_are_not_its_own() {
        assert!(run("GET", &[]).is_none());
    }
}
