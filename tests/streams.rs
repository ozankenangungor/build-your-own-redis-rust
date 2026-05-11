mod common;

use common::Server;

const TOP_ITEM: &str =
    "-ERR The ID specified in XADD is equal or smaller than the target stream top item\r\n";

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
fn rejects_an_id_that_does_not_beat_the_last_one() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "1-1", "foo", "bar"]);
    client.expect_reply("$3\r\n1-1\r\n");

    // The exact time and sequence number as the last entry.
    client.send(&["XADD", "stream_key", "1-1", "bar", "baz"]);
    client.expect_reply(TOP_ITEM);

    // A smaller time with a larger sequence number.
    client.send(&["XADD", "stream_key", "0-2", "bar", "baz"]);
    client.expect_reply(TOP_ITEM);
}

#[test]
fn accepts_an_id_that_grows_in_either_half() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "1-1", "foo", "bar"]);
    client.expect_reply("$3\r\n1-1\r\n");

    // A larger sequence number within the same millisecond.
    client.send(&["XADD", "stream_key", "1-2", "foo", "bar"]);
    client.expect_reply("$3\r\n1-2\r\n");

    // A later millisecond with a smaller sequence number.
    client.send(&["XADD", "stream_key", "2-0", "foo", "bar"]);
    client.expect_reply("$3\r\n2-0\r\n");
}

#[test]
fn rejects_the_zero_id_and_accepts_the_one_above_it() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-0", "baz", "foo"]);
    client.expect_reply("-ERR The ID specified in XADD must be greater than 0-0\r\n");

    client.send(&["XADD", "stream_key", "0-1", "baz", "foo"]);
    client.expect_reply("$3\r\n0-1\r\n");
}

#[test]
fn refuses_the_zero_id_even_on_a_string_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    // Redis checks the id before it ever looks at the key.
    client.send(&["XADD", "foo", "0-0", "field", "value"]);
    client.expect_reply("-ERR The ID specified in XADD must be greater than 0-0\r\n");
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
