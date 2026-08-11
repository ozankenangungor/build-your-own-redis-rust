use super::wrong_arity;
use crate::resp::Value;
use crate::users::Users;
use bytes::Bytes;

/// The user a connection is taken to be until it says otherwise.
///
/// Redis gives every new connection this name. Whether it has to prove itself
/// before it is believed is another matter, and one still to come.
const DEFAULT_USER: &str = "default";

/// How Redis flags a user that will let any password through, having none of
/// its own to check against.
const NO_PASSWORD: &[u8] = b"nopass";

/// Handles the commands that ask who a client is and what it may do. `None`
/// means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], users: &Users) -> Option<Value> {
    let reply = match command {
        // `ACL` covers several commands in one, told apart by the word after it.
        "ACL" => match args.split_first() {
            Some((action, rest)) if action.eq_ignore_ascii_case(b"WHOAMI") => match rest {
                [] => Value::BulkString(Bytes::from_static(DEFAULT_USER.as_bytes())),
                _ => unknown_action(action),
            },
            Some((action, rest)) if action.eq_ignore_ascii_case(b"GETUSER") => match rest {
                [user] => user_named(user, users),
                _ => unknown_action(action),
            },
            Some((action, rest)) if action.eq_ignore_ascii_case(b"SETUSER") => match rest {
                [user, rules @ ..] if !rules.is_empty() => set_user(user, rules, users),
                _ => unknown_action(action),
            },
            Some((action, _)) => unknown_action(action),
            None => wrong_arity("acl"),
        },
        // Whether a client knows a password the user may be let in with. The
        // connection is not yet held to the answer; that comes later.
        "AUTH" => match args {
            [password] => alone(password, users),
            [user, password] => authenticate(user, password, users),
            _ => wrong_arity("auth"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Checks a password against the user a client says it is.
///
/// A user this server has never heard of is turned away in the same words as a
/// password that does not match, so that a client learns nothing from the
/// asking beyond whether it got in.
fn authenticate(user: &Bytes, password: &Bytes, users: &Users) -> Value {
    if user != DEFAULT_USER || !users.accepts(password) {
        return wrong_password();
    }

    Value::SimpleString("OK".into())
}

/// The same, for a client that gave a password and no user.
///
/// Redis takes it for the default user, but only when that user has a password
/// to check: told a password where none was ever set, it says so rather than
/// letting the client believe it had done something.
fn alone(password: &Bytes, users: &Users) -> Value {
    if users.wants_no_password() {
        return Value::Error(
            "ERR Client sent AUTH, but no password is set. Did you mean AUTH <username> <password>?"
                .into(),
        );
    }

    authenticate(
        &Bytes::from_static(DEFAULT_USER.as_bytes()),
        password,
        users,
    )
}

fn wrong_password() -> Value {
    Value::Error("WRONGPASS invalid username-password pair or user is disabled.".into())
}

/// Everything this server has to say about a user, as the pairs of a property
/// and its value that `ACL GETUSER` answers with.
///
/// A name this server has never heard of is answered with nothing at all: there
/// is only the one user, and it was here before the server started.
fn user_named(user: &Bytes, users: &Users) -> Value {
    if user != DEFAULT_USER {
        return Value::NullArray;
    }

    // A user with no password of its own is flagged as wanting none, which is
    // what makes a client believed without being asked. Give it one and the
    // flag goes: there is now something to check against.
    let flags = match users.wants_no_password() {
        true => vec![Value::BulkString(Bytes::from_static(NO_PASSWORD))],
        false => Vec::new(),
    };

    // The passwords go back hashed, as they are kept, so that a client asking
    // after a user learns nothing it could log in with.
    let passwords = users
        .hashed_passwords()
        .into_iter()
        .map(|hashed| Value::BulkString(Bytes::from(hashed)))
        .collect();

    Value::Array(vec![
        Value::BulkString(Bytes::from_static(b"flags")),
        Value::Array(flags),
        Value::BulkString(Bytes::from_static(b"passwords")),
        Value::Array(passwords),
    ])
}

/// Changes a user by the rules it is given.
///
/// Every rule is looked over before any is applied, so a command with one it
/// cannot follow changes nothing at all.
fn set_user(user: &Bytes, rules: &[Bytes], users: &Users) -> Value {
    // Redis makes a user it has never heard of; this server has the one it
    // started with, and says so rather than pretending to make another.
    if user != DEFAULT_USER {
        return Value::Error(format!(
            "ERR this server has only the '{DEFAULT_USER}' user"
        ));
    }

    let mut passwords = Vec::with_capacity(rules.len());

    for rule in rules {
        match rule.split_first() {
            // `>password` is the one rule this server follows: it gives the
            // user another password it may be let in with.
            Some((b'>', password)) if !password.is_empty() => passwords.push(password),
            _ => return bad_rule(rule),
        }
    }

    for password in passwords {
        users.add_password(password);
    }

    Value::SimpleString("OK".into())
}

/// What a client is told of a rule this server cannot follow.
fn bad_rule(rule: &[u8]) -> Value {
    Value::Error(format!(
        "ERR Error in ACL SETUSER modifier '{}': Syntax error",
        String::from_utf8_lossy(rule)
    ))
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
        acl_of(words, &Users::default())
    }

    fn acl_of(words: &[&str], users: &Users) -> Value {
        let args: Vec<Bytes> = words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect();

        run("ACL", &args, users).expect("acl belongs to this module")
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
    fn says_what_it_has_to_say_of_the_user_every_client_is() {
        // Two properties: the flag that says the user wants no password, and
        // the passwords it has, of which there are none.
        assert_eq!(
            acl(&["GETUSER", "default"]).encode(),
            b"*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n"
        );
    }

    #[test]
    fn says_nothing_of_a_user_it_has_never_heard_of() {
        // There is the one user, and it was here before the server started.
        assert_eq!(acl(&["GETUSER", "alice"]), Value::NullArray);
        assert_eq!(acl(&["GETUSER", ""]), Value::NullArray);
    }

    #[test]
    fn takes_the_word_before_the_user_however_it_is_spelled() {
        for spelling in ["GETUSER", "getuser", "GetUser"] {
            assert_eq!(
                acl(&[spelling, "default"]).encode(),
                b"*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n",
                "{spelling}"
            );
        }
    }

    /// The hash of the password the stage sets, as Redis keeps it.
    const MY_PASSWORD: &str = "89e01536ac207279409d4de1e5253e01f4a1769e696db0d6062ca9b8f56767c8";

    #[test]
    fn takes_a_password_for_the_user_and_says_it_has() {
        let users = Users::default();

        assert_eq!(
            acl_of(&["SETUSER", "default", ">mypassword"], &users),
            Value::SimpleString("OK".into())
        );
    }

    #[test]
    fn lets_the_flag_go_once_the_user_has_a_password_to_check() {
        let users = Users::default();

        acl_of(&["SETUSER", "default", ">mypassword"], &users);

        assert_eq!(
            acl_of(&["GETUSER", "default"], &users).encode(),
            format!("*4\r\n$5\r\nflags\r\n*0\r\n$9\r\npasswords\r\n*1\r\n$64\r\n{MY_PASSWORD}\r\n")
                .as_bytes()
        );
    }

    #[test]
    fn takes_the_word_before_the_rules_however_it_is_spelled() {
        for spelling in ["SETUSER", "setuser", "SetUser"] {
            let users = Users::default();

            acl_of(&[spelling, "default", ">mypassword"], &users);

            assert_eq!(users.hashed_passwords(), [MY_PASSWORD], "{spelling}");
        }
    }

    #[test]
    fn takes_every_password_a_command_gives_the_user() {
        let users = Users::default();

        acl_of(&["SETUSER", "default", ">first", ">second"], &users);

        assert_eq!(users.hashed_passwords().len(), 2);
    }

    #[test]
    fn takes_nothing_at_all_from_a_command_with_a_rule_it_cannot_follow() {
        let users = Users::default();

        // The rule it could follow is passed over too: a command half carried
        // out is worse than one refused.
        assert_eq!(
            acl_of(&["SETUSER", "default", ">mypassword", "allkeys"], &users),
            bad_rule(b"allkeys")
        );
        assert!(users.wants_no_password());
    }

    #[test]
    fn refuses_a_rule_it_cannot_follow() {
        for rule in ["on", "off", "~*", "+@all", "<mypassword", "nopass", ">", ""] {
            let users = Users::default();

            assert_eq!(
                acl_of(&["SETUSER", "default", rule], &users),
                bad_rule(rule.as_bytes()),
                "{rule:?}"
            );
        }
    }

    #[test]
    fn refuses_to_make_a_user_it_was_never_started_with() {
        let users = Users::default();
        let Value::Error(said) = acl_of(&["SETUSER", "alice", ">mypassword"], &users) else {
            panic!("a user this server does not have is refused");
        };

        assert!(said.starts_with("ERR "), "{said:?}");
        assert!(users.wants_no_password());
    }

    fn auth(words: &[&str], users: &Users) -> Value {
        let args: Vec<Bytes> = words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect();

        run("AUTH", &args, users).expect("auth belongs to this module")
    }

    #[test]
    fn lets_in_a_client_that_knows_the_password() {
        let users = Users::default();
        acl_of(&["SETUSER", "default", ">mypassword"], &users);

        assert_eq!(
            auth(&["default", "mypassword"], &users),
            Value::SimpleString("OK".into())
        );
    }

    #[test]
    fn turns_away_a_client_that_does_not() {
        let users = Users::default();
        acl_of(&["SETUSER", "default", ">mypassword"], &users);

        assert_eq!(
            auth(&["default", "wrongpassword"], &users),
            wrong_password()
        );
        assert_eq!(auth(&["default", ""], &users), wrong_password());
        assert_eq!(auth(&["default", "MYPASSWORD"], &users), wrong_password());
    }

    #[test]
    fn lets_in_a_client_that_knows_any_of_the_passwords() {
        let users = Users::default();
        acl_of(&["SETUSER", "default", ">first", ">second"], &users);

        for password in ["first", "second"] {
            assert_eq!(
                auth(&["default", password], &users),
                Value::SimpleString("OK".into()),
                "{password}"
            );
        }
    }

    #[test]
    fn turns_away_a_client_naming_a_user_this_server_does_not_have() {
        let users = Users::default();
        acl_of(&["SETUSER", "default", ">mypassword"], &users);

        // Said in the same words as a wrong password, so that a client learns
        // nothing from the asking beyond whether it got in.
        assert_eq!(auth(&["alice", "mypassword"], &users), wrong_password());
    }

    #[test]
    fn lets_anyone_in_while_the_user_wants_no_password() {
        let users = Users::default();

        assert_eq!(
            auth(&["default", "anything at all"], &users),
            Value::SimpleString("OK".into())
        );
    }

    #[test]
    fn takes_a_password_given_without_a_user() {
        let users = Users::default();
        acl_of(&["SETUSER", "default", ">mypassword"], &users);

        assert_eq!(
            auth(&["mypassword"], &users),
            Value::SimpleString("OK".into())
        );
        assert_eq!(auth(&["wrongpassword"], &users), wrong_password());
    }

    #[test]
    fn says_so_when_a_password_is_given_where_none_was_ever_set() {
        let users = Users::default();
        let Value::Error(said) = auth(&["mypassword"], &users) else {
            panic!("a password where none was set is refused");
        };

        assert!(said.starts_with("ERR Client sent AUTH"), "{said:?}");
    }

    #[test]
    fn refuses_an_auth_that_says_nothing() {
        let users = Users::default();

        assert_eq!(auth(&[], &users), wrong_arity("auth"));
        assert_eq!(auth(&["default", "a", "b"], &users), wrong_arity("auth"));
    }

    #[test]
    fn refuses_a_setuser_that_names_no_user_or_no_rule() {
        assert_eq!(acl(&["SETUSER"]), unknown_action(b"SETUSER"));
        assert_eq!(acl(&["SETUSER", "default"]), unknown_action(b"SETUSER"));
    }

    #[test]
    fn refuses_a_getuser_that_names_no_user_or_more_than_one() {
        assert_eq!(acl(&["GETUSER"]), unknown_action(b"GETUSER"));
        assert_eq!(
            acl(&["GETUSER", "default", "extra"]),
            unknown_action(b"GETUSER")
        );
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
        assert!(run("GET", &[], &Users::default()).is_none());
    }
}
