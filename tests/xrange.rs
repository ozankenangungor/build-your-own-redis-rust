mod common;

use common::{Client, Server};

/// Fills `stream_key` with the three entries from the stage description.
fn three_entries(server: &Server) -> Client {
    let mut client = server.connect();

    for (id, field, value) in [
        ("0-1", "foo", "bar"),
        ("0-2", "bar", "baz"),
        ("0-3", "baz", "foo"),
    ] {
        client.send(&["XADD", "stream_key", id, field, value]);
        client.expect_reply(&format!("$3\r\n{id}\r\n"));
    }

    client
}

#[test]
fn queries_a_range_of_entries() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-2", "0-3"]);
    client.expect_reply(concat!(
        "*2\r\n",
        "*2\r\n$3\r\n0-2\r\n*2\r\n$3\r\nbar\r\n$3\r\nbaz\r\n",
        "*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn includes_the_entries_sitting_on_both_bounds() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-1", "0-1"]);
    client.expect_reply("*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n");
}

#[test]
fn fills_in_a_missing_sequence_number_on_each_bound() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "1-1", "a", "1"]);
    client.expect_reply("$3\r\n1-1\r\n");
    client.send(&["XADD", "stream_key", "2-1", "b", "2"]);
    client.expect_reply("$3\r\n2-1\r\n");
    client.send(&["XADD", "stream_key", "3-1", "c", "3"]);
    client.expect_reply("$3\r\n3-1\r\n");

    // `2` as a start means `2-0`, and as an end it means the whole of
    // millisecond two, so only the middle entry is in range.
    client.send(&["XRANGE", "stream_key", "2", "2"]);
    client.expect_reply("*1\r\n*2\r\n$3\r\n2-1\r\n*2\r\n$1\r\nb\r\n$1\r\n2\r\n");
}

#[test]
fn replies_with_several_field_value_pairs_in_order() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&[
        "XADD",
        "stream_key",
        "1526985054069-0",
        "temperature",
        "36",
        "humidity",
        "95",
    ]);
    client.expect_reply("$15\r\n1526985054069-0\r\n");

    client.send(&["XRANGE", "stream_key", "1526985054069", "1526985054069"]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$15\r\n1526985054069-0\r\n",
        "*4\r\n$11\r\ntemperature\r\n$2\r\n36\r\n$8\r\nhumidity\r\n$2\r\n95\r\n",
    ));
}

#[test]
fn queries_from_the_start_of_the_stream_with_a_dash() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "-", "0-2"]);
    client.expect_reply(concat!(
        "*2\r\n",
        "*2\r\n$3\r\n0-1\r\n*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n",
        "*2\r\n$3\r\n0-2\r\n*2\r\n$3\r\nbar\r\n$3\r\nbaz\r\n",
    ));
}

#[test]
fn queries_to_the_end_of_the_stream_with_a_plus() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-2", "+"]);
    client.expect_reply(concat!(
        "*2\r\n",
        "*2\r\n$3\r\n0-2\r\n*2\r\n$3\r\nbar\r\n$3\r\nbaz\r\n",
        "*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn queries_the_whole_stream_with_both_ends() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "-", "+"]);
    client.expect_reply(concat!(
        "*3\r\n",
        "*2\r\n$3\r\n0-1\r\n*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n",
        "*2\r\n$3\r\n0-2\r\n*2\r\n$3\r\nbar\r\n$3\r\nbaz\r\n",
        "*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn returns_an_empty_array_when_nothing_is_in_range() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-4", "0-9"]);
    client.expect_reply("*0\r\n");

    client.send(&["XRANGE", "missing_stream", "0-1", "0-9"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn holds_stream_fields_that_are_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    client.send_bytes(&[b"XADD", b"stream_key", b"0-1", b"\xff", b"\x00\x01"]);
    client.expect_reply("$3\r\n0-1\r\n");

    client.send_bytes(&[b"XRANGE", b"stream_key", b"-", b"+"]);
    client.expect_bytes(b"*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$1\r\n\xff\r\n$2\r\n\x00\x01\r\n");
}

#[test]
fn rejects_a_range_with_a_malformed_bound() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-1", "later"]);
    client.expect_reply("-ERR Invalid stream ID specified as stream command argument\r\n");
}

#[test]
fn rejects_a_stream_range_over_a_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["XRANGE", "list_key", "0-1", "0-9"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}
