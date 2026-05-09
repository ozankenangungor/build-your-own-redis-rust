mod common;

use common::{Client, Server};

/// Fills `list_key` with "a".."e" and returns a connection to query it.
fn five_elements(server: &Server) -> Client {
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "a", "b", "c", "d", "e"]);
    client.expect_reply(":5\r\n");

    client
}

#[test]
fn lists_a_range_of_elements() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LRANGE", "list_key", "0", "1"]);
    client.expect_reply("*2\r\n$1\r\na\r\n$1\r\nb\r\n");

    client.send(&["LRANGE", "list_key", "2", "4"]);
    client.expect_reply("*3\r\n$1\r\nc\r\n$1\r\nd\r\n$1\r\ne\r\n");
}

#[test]
fn counts_negative_indexes_from_the_end() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LRANGE", "list_key", "-2", "-1"]);
    client.expect_reply("*2\r\n$1\r\nd\r\n$1\r\ne\r\n");

    client.send(&["LRANGE", "list_key", "0", "-3"]);
    client.expect_reply("*3\r\n$1\r\na\r\n$1\r\nb\r\n$1\r\nc\r\n");

    client.send(&["LRANGE", "list_key", "2", "-1"]);
    client.expect_reply("*3\r\n$1\r\nc\r\n$1\r\nd\r\n$1\r\ne\r\n");
}

#[test]
fn clamps_a_negative_index_reaching_past_the_start() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LRANGE", "list_key", "-6", "-4"]);
    client.expect_reply("*2\r\n$1\r\na\r\n$1\r\nb\r\n");
}

#[test]
fn treats_a_stop_past_the_end_as_the_last_element() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LRANGE", "list_key", "3", "100"]);
    client.expect_reply("*2\r\n$1\r\nd\r\n$1\r\ne\r\n");
}

#[test]
fn returns_an_empty_array_when_the_range_yields_nothing() {
    let server = Server::start();
    let mut client = five_elements(&server);

    // Start past the end of the list.
    client.send(&["LRANGE", "list_key", "5", "9"]);
    client.expect_reply("*0\r\n");

    // Start after stop.
    client.send(&["LRANGE", "list_key", "3", "1"]);
    client.expect_reply("*0\r\n");

    // A list that does not exist at all.
    client.send(&["LRANGE", "missing", "0", "9"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn rejects_a_range_with_non_numeric_bounds() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LRANGE", "list_key", "0", "last"]);
    client.expect_reply("-ERR value is not an integer or out of range\r\n");
}

#[test]
fn rejects_a_range_over_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["LRANGE", "foo", "0", "1"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn reports_the_length_of_a_list() {
    let server = Server::start();
    let mut client = five_elements(&server);

    client.send(&["LLEN", "list_key"]);
    client.expect_reply(":5\r\n");
}

#[test]
fn reports_zero_for_a_missing_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["LLEN", "missing_list_key"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn rejects_a_length_query_on_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["LLEN", "foo"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}
