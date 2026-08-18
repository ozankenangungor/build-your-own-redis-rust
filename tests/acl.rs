//! What a client sees of the ACL commands over a connection.
//!
//! What each command answers on its own, and what a password does to one
//! identity, are measured in `src/commands/acl.rs`. What is left here is what
//! only a running server can show: that a password set on one connection is the
//! same password on the next, that the door it closes is closed to every
//! command, and that opening it for one client opens it for that client alone.

mod common;

use common::Server;

/// The hash of the password the tester sets, as Redis keeps it.
const MY_PASSWORD: &str = "89e01536ac207279409d4de1e5253e01f4a1769e696db0d6062ca9b8f56767c8";

#[test]
fn says_who_the_client_is() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.expect_reply("$7\r\ndefault\r\n");
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
fn lets_a_client_in_for_good_once_it_has_given_the_password() {
    let server = Server::start();
    let mut inside = server.connect();

    inside.send(&["ACL", "SETUSER", "default", ">newpassword"]);
    inside.expect_reply("+OK\r\n");

    let mut outside = server.connect();

    outside.send(&["PING"]);
    let said = outside.read_line();
    assert!(said.starts_with("-NOAUTH"), "{said:?}");

    // A wrong password leaves it where it was.
    outside.send(&["AUTH", "default", "wrongpassword"]);
    let said = outside.read_line();
    assert!(said.starts_with("-WRONGPASS"), "{said:?}");

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
fn keeps_serving_the_connection_after_an_acl() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ACL", "WHOAMI"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
