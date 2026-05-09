mod common;

use common::Server;

#[test]
fn echoes_its_argument() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ECHO", "hey"]);
    client.expect_reply("$3\r\nhey\r\n");
}

#[test]
fn accepts_any_casing_of_the_command_name() {
    let server = Server::start();
    let mut client = server.connect();

    for name in ["ECHO", "echo", "EcHo"] {
        client.send(&[name, "hey"]);
        client.expect_reply("$3\r\nhey\r\n");
    }
}

#[test]
fn sets_then_gets_a_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbar\r\n");
}

#[test]
fn overwrites_an_existing_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");
    client.send(&["SET", "foo", "baz"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbaz\r\n");
}

#[test]
fn returns_a_null_bulk_string_for_a_missing_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GET", "missing"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn shares_keys_across_connections() {
    let server = Server::start();
    let mut writer = server.connect();
    let mut reader = server.connect();

    writer.send(&["SET", "foo", "bar"]);
    writer.expect_reply("+OK\r\n");

    reader.send(&["GET", "foo"]);
    reader.expect_reply("$3\r\nbar\r\n");
}
