mod common;

use common::{Client, Server};
use std::time::{SystemTime, UNIX_EPOCH};

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
fn generates_a_sequence_number_starting_at_zero() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "5-*", "foo", "bar"]);
    client.expect_reply("$3\r\n5-0\r\n");

    client.send(&["XADD", "stream_key", "5-*", "bar", "baz"]);
    client.expect_reply("$3\r\n5-1\r\n");
}

#[test]
fn generates_a_sequence_number_starting_at_one_for_time_zero() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-*", "foo", "bar"]);
    client.expect_reply("$3\r\n0-1\r\n");

    client.send(&["XADD", "stream_key", "0-*", "bar", "baz"]);
    client.expect_reply("$3\r\n0-2\r\n");
}

#[test]
fn continues_the_sequence_of_an_explicit_entry() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "5-3", "foo", "bar"]);
    client.expect_reply("$3\r\n5-3\r\n");

    client.send(&["XADD", "stream_key", "5-*", "bar", "baz"]);
    client.expect_reply("$3\r\n5-4\r\n");
}

#[test]
fn restarts_the_sequence_for_a_later_millisecond() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "5-*", "foo", "bar"]);
    client.expect_reply("$3\r\n5-0\r\n");

    client.send(&["XADD", "stream_key", "6-*", "bar", "baz"]);
    client.expect_reply("$3\r\n6-0\r\n");
}

#[test]
fn rejects_a_generated_id_that_lands_below_the_top() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "5-0", "foo", "bar"]);
    client.expect_reply("$3\r\n5-0\r\n");

    client.send(&["XADD", "stream_key", "1-*", "bar", "baz"]);
    client.expect_reply(TOP_ITEM);
}

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
fn returns_an_empty_array_when_nothing_is_in_range() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XRANGE", "stream_key", "0-4", "0-9"]);
    client.expect_reply("*0\r\n");

    client.send(&["XRANGE", "missing_stream", "0-1", "0-9"]);
    client.expect_reply("*0\r\n");
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

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_millis() as u64
}

/// Appends an entry with a fully generated id and returns its two halves.
fn add_generated(client: &mut Client) -> (u64, u64) {
    client.send(&["XADD", "stream_key", "*", "foo", "bar"]);
    let id = client.read_bulk_string();

    let (milliseconds, sequence) = id
        .split_once('-')
        .unwrap_or_else(|| panic!("bad id {id:?}"));

    (
        milliseconds.parse().expect("a timestamp"),
        sequence.parse().expect("a sequence number"),
    )
}

#[test]
fn generates_the_whole_id_from_the_clock() {
    let server = Server::start();
    let mut client = server.connect();

    let before = unix_milliseconds();
    let (milliseconds, sequence) = add_generated(&mut client);
    let after = unix_milliseconds();

    assert_eq!(sequence, 0);
    assert!(
        (before..=after).contains(&milliseconds),
        "{milliseconds} is not within {before}..={after}"
    );
}

#[test]
fn keeps_generated_ids_increasing() {
    let server = Server::start();
    let mut client = server.connect();

    // Several of these usually land in the same millisecond, which is the case
    // that has to fall back to bumping the sequence number.
    let mut previous = add_generated(&mut client);
    for _ in 0..20 {
        let current = add_generated(&mut client);
        assert!(
            current > previous,
            "{current:?} does not follow {previous:?}"
        );
        previous = current;
    }
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
