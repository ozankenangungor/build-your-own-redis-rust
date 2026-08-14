mod common;

use common::Server;

#[test]
fn says_who_the_client_is() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.expect_reply("$7\r\ndefault\r\n");
}

#[test]
fn says_the_same_to_every_client() {
    let server = Server::start();

    // Every connection starts out as the same user, so the answer does not
    // depend on who is asking.
    for _ in 0..3 {
        let mut client = server.connect();

        client.send(&["ACL", "WHOAMI"]);
        client.expect_reply("$7\r\ndefault\r\n");
    }
}

#[test]
fn accepts_any_casing_of_the_command_and_the_word_after_it() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [
        ["ACL", "WHOAMI"],
        ["acl", "whoami"],
        ["Acl", "WhoAmI"],
        ["ACL", "whoami"],
    ] {
        client.send(&command);
        client.expect_reply("$7\r\ndefault\r\n");
    }
}

#[test]
fn says_what_it_has_to_say_of_the_user_every_client_is() {
    let server = Server::start();
    let mut client = server.connect();

    // Two properties, each a name and its value: the flag that says the user
    // wants no password, and the passwords it has, of which there are none.
    client.send(&["ACL", "GETUSER", "default"]);
    client.expect_reply("*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n");
}

/// The hash of the password the tester sets, as Redis keeps it.
const MY_PASSWORD: &str = "89e01536ac207279409d4de1e5253e01f4a1769e696db0d6062ca9b8f56767c8";

#[test]
fn lets_the_flag_go_once_the_user_has_a_password_to_check() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "GETUSER", "default"]);
    client.expect_reply("*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n");

    client.send(&["ACL", "SETUSER", "default", ">mypassword"]);
    client.expect_reply("+OK\r\n");

    // The flag is gone, and the password is there in its place, hashed.
    client.send(&["ACL", "GETUSER", "default"]);
    client.expect_reply(&format!(
        "*4\r\n$5\r\nflags\r\n*0\r\n$9\r\npasswords\r\n*1\r\n$64\r\n{MY_PASSWORD}\r\n"
    ));
}

#[test]
fn keeps_the_password_for_every_client_and_not_the_one_that_set_it() {
    let server = Server::start();
    let mut setting = server.connect();

    setting.send(&["ACL", "SETUSER", "default", ">mypassword"]);
    setting.expect_reply("+OK\r\n");

    // There is one user, and it is the same user on every connection: the new
    // one is shut out until it gives the password, and then sees the same.
    let mut asking = server.connect();

    asking.send(&["AUTH", "default", "mypassword"]);
    asking.expect_reply("+OK\r\n");

    asking.send(&["ACL", "GETUSER", "default"]);
    asking.expect_reply(&format!(
        "*4\r\n$5\r\nflags\r\n*0\r\n$9\r\npasswords\r\n*1\r\n$64\r\n{MY_PASSWORD}\r\n"
    ));
}

#[test]
fn lets_in_a_client_that_knows_the_password_and_turns_away_one_that_does_not() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "SETUSER", "default", ">mypassword"]);
    client.expect_reply("+OK\r\n");

    client.send(&["AUTH", "default", "wrongpassword"]);
    let said = client.read_line();
    assert!(said.starts_with("-WRONGPASS"), "{said:?}");

    client.send(&["AUTH", "default", "mypassword"]);
    client.expect_reply("+OK\r\n");
}

#[test]
fn shuts_out_a_new_connection_and_leaves_the_one_that_set_the_password_in() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    // The connection that set the password was let in before it, and stays in.
    inside.send(&["ACL", "WHOAMI"]);
    inside.expect_reply("$7\r\ndefault\r\n");

    // One made afterwards has to ask.
    let mut outside = server.connect();

    outside.send(&["ACL", "WHOAMI"]);
    let said = outside.read_line();

    assert!(said.starts_with("-NOAUTH"), "{said:?}");
}

#[test]
fn shuts_a_new_connection_out_of_everything_it_might_ask_for() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    let mut outside = server.connect();

    for command in [
        ["PING"].as_slice(),
        ["GET", "foo"].as_slice(),
        ["SET", "foo", "bar"].as_slice(),
        ["ACL", "GETUSER", "default"].as_slice(),
        ["SUBSCRIBE", "channel"].as_slice(),
        ["MULTI"].as_slice(),
        ["NONSENSE"].as_slice(),
    ] {
        outside.send(command);
        let said = outside.read_line();

        assert!(said.starts_with("-NOAUTH"), "{command:?}: {said:?}");
    }
}

#[test]
fn lets_a_shut_out_connection_say_who_it_is() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    let mut outside = server.connect();

    // A wrong password leaves it outside, and a right one lets it in.
    outside.send(&["AUTH", "default", "wrongpassword"]);
    let said = outside.read_line();
    assert!(said.starts_with("-WRONGPASS"), "{said:?}");

    outside.send(&["ACL", "WHOAMI"]);
    let said = outside.read_line();
    assert!(said.starts_with("-NOAUTH"), "{said:?}");

    outside.send(&["AUTH", "default", "newpassword"]);
    outside.expect_reply("+OK\r\n");

    outside.send(&["ACL", "WHOAMI"]);
    outside.expect_reply("$7\r\ndefault\r\n");
}

#[test]
fn lets_a_client_in_for_good_once_it_has_given_the_password() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    let mut outside = server.connect();

    outside.send(&["PING"]);
    let said = outside.read_line();
    assert!(said.starts_with("-NOAUTH"), "{said:?}");

    outside.send(&["AUTH", "default", "newpassword"]);
    outside.expect_reply("+OK\r\n");

    // Being let in lasts as long as the connection: everything it could not ask
    // for before, it may ask for now, and without asking again.
    outside.send(&["PING"]);
    outside.expect_reply("+PONG\r\n");
    outside.send(&["SET", "foo", "bar"]);
    outside.expect_reply("+OK\r\n");
    outside.send(&["GET", "foo"]);
    outside.expect_reply("$3\r\nbar\r\n");
    outside.send(&["ACL", "WHOAMI"]);
    outside.expect_reply("$7\r\ndefault\r\n");
}

#[test]
fn leaves_the_other_clients_where_they_were_when_one_gets_in() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    let mut letting_itself_in = server.connect();
    let mut staying_out = server.connect();

    letting_itself_in.send(&["AUTH", "default", "newpassword"]);
    letting_itself_in.expect_reply("+OK\r\n");

    // One client getting in says nothing about another.
    staying_out.send(&["PING"]);
    let said = staying_out.read_line();

    assert!(said.starts_with("-NOAUTH"), "{said:?}");
}

#[test]
fn takes_a_password_set_on_another_connection() {
    let server = Server::start();
    let mut setting = server.connect();

    setting.send(&["ACL", "SETUSER", "default", ">mypassword"]);
    setting.expect_reply("+OK\r\n");

    // One user, one set of passwords, whichever connection asks.
    let mut authenticating = server.connect();

    authenticating.send(&["AUTH", "default", "mypassword"]);
    authenticating.expect_reply("+OK\r\n");
}

#[test]
fn turns_away_a_client_naming_a_user_this_server_does_not_have() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "SETUSER", "default", ">mypassword"]);
    client.expect_reply("+OK\r\n");

    client.send(&["AUTH", "alice", "mypassword"]);
    let said = client.read_line();

    assert!(said.starts_with("-WRONGPASS"), "{said:?}");
}

#[test]
fn lets_anyone_in_while_the_user_wants_no_password() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["AUTH", "default", "anything at all"]);
    client.expect_reply("+OK\r\n");
}

#[test]
fn refuses_an_auth_that_says_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["AUTH"]);
    client.expect_reply("-ERR wrong number of arguments for 'auth' command\r\n");
}

#[test]
fn takes_a_password_that_is_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    // A password is a run of bytes, as a value is, and may hold anything.
    client
        .send_raw(b"*4\r\n$3\r\nACL\r\n$7\r\nSETUSER\r\n$7\r\ndefault\r\n$5\r\n>\xff\x00\r\n\r\n");
    client.expect_reply("+OK\r\n");

    client.send(&["ACL", "GETUSER", "default"]);
    let said = client.read_reply();

    assert!(said.contains("passwords"), "{said:?}");
    assert!(!said.contains("nopass"), "{said:?}");
}

#[test]
fn refuses_a_rule_it_cannot_follow() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "SETUSER", "default", "on"]);
    let said = client.read_line();

    assert!(
        said.starts_with("-ERR Error in ACL SETUSER modifier 'on'"),
        "{said:?}"
    );

    // Nothing was taken on, so the user still wants no password.
    client.send(&["ACL", "GETUSER", "default"]);
    client.expect_reply("*4\r\n$5\r\nflags\r\n*1\r\n$6\r\nnopass\r\n$9\r\npasswords\r\n*0\r\n");
}

#[test]
fn refuses_a_setuser_that_names_no_rule() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "SETUSER", "default"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Unknown ACL subcommand"), "{said:?}");
}

#[test]
fn says_nothing_of_a_user_it_has_never_heard_of() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "GETUSER", "alice"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn refuses_a_getuser_that_names_no_user() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "GETUSER"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Unknown ACL subcommand"), "{said:?}");
}

#[test]
fn refuses_an_acl_that_says_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL"]);
    client.expect_reply("-ERR wrong number of arguments for 'acl' command\r\n");
}

#[test]
fn refuses_a_word_it_has_no_answer_for() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "NONSENSE"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Unknown ACL subcommand"), "{said:?}");
}

#[test]
fn keeps_serving_the_connection_after_an_acl() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
