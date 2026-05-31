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
fn stores_a_value_that_is_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    // A PNG header: a lone `\xff`, an embedded NUL and bytes that are not valid
    // UTF-8 are all ordinary content as far as Redis is concerned.
    let value: &[u8] = b"\x89PNG\r\n\x1a\n\x00\xff\xfe";

    client.send_bytes(&[b"SET", b"picture", value]);
    client.expect_reply("+OK\r\n");

    client.send_bytes(&[b"GET", b"picture"]);
    client.expect_bytes(b"$11\r\n\x89PNG\r\n\x1a\n\x00\xff\xfe\r\n");
}

#[test]
fn stores_a_key_that_is_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    client.send_bytes(&[b"SET", b"\xff\x00key", b"value"]);
    client.expect_reply("+OK\r\n");

    client.send_bytes(&[b"GET", b"\xff\x00key"]);
    client.expect_reply("$5\r\nvalue\r\n");

    // The bytes have to match exactly, not merely look alike once mangled.
    client.send_bytes(&[b"GET", b"\xfd\x00key"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn echoes_bytes_that_are_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    client.send_bytes(&[b"ECHO", b"\x00\xff"]);
    client.expect_bytes(b"$2\r\n\x00\xff\r\n");
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
