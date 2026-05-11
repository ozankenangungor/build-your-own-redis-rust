mod common;

use common::Server;

#[test]
fn creates_a_stream_and_returns_the_entry_id() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-1", "foo", "bar"]);
    client.expect_reply("$3\r\n0-1\r\n");

    client.send(&["TYPE", "stream_key"]);
    client.expect_reply("+stream\r\n");
}

#[test]
fn appends_to_an_existing_stream() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "1526919030474-0", "temperature", "36"]);
    client.expect_reply("$15\r\n1526919030474-0\r\n");

    client.send(&["XADD", "stream_key", "1526919030474-1", "temperature", "37"]);
    client.expect_reply("$15\r\n1526919030474-1\r\n");
}

#[test]
fn accepts_several_field_value_pairs() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&[
        "XADD",
        "stream_key",
        "0-1",
        "temperature",
        "36",
        "humidity",
        "95",
    ]);
    client.expect_reply("$3\r\n0-1\r\n");
}

#[test]
fn rejects_a_malformed_entry_id() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "one", "foo", "bar"]);
    client.expect_reply("-ERR Invalid stream ID specified as stream command argument\r\n");

    client.send(&["XADD", "stream_key", "0-x", "foo", "bar"]);
    client.expect_reply("-ERR Invalid stream ID specified as stream command argument\r\n");
}

#[test]
fn rejects_fields_that_do_not_pair_up() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-1"]);
    client.expect_reply("-ERR wrong number of arguments for 'xadd' command\r\n");

    client.send(&["XADD", "stream_key", "0-1", "foo"]);
    client.expect_reply("-ERR wrong number of arguments for 'xadd' command\r\n");
}

#[test]
fn rejects_an_append_onto_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["XADD", "foo", "0-1", "field", "value"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn rejects_a_get_of_a_stream() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-1", "foo", "bar"]);
    client.expect_reply("$3\r\n0-1\r\n");

    client.send(&["GET", "stream_key"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}
