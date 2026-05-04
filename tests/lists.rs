mod common;

use common::Server;

#[test]
fn creates_a_list_on_the_first_push() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn appends_to_an_existing_list() {
    let server = Server::start();
    let mut client = server.connect();

    for length in 1..=3 {
        client.send(&["RPUSH", "list_key", &format!("element{length}")]);
        client.expect_reply(&format!(":{length}\r\n"));
    }
}

#[test]
fn creates_a_list_with_several_elements_at_once() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "another_list", "bar", "baz"]);
    client.expect_reply(":2\r\n");

    client.send(&["RPUSH", "another_list", "foo", "bar", "baz"]);
    client.expect_reply(":5\r\n");
}

#[test]
fn rejects_a_push_without_elements() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key"]);
    client.expect_reply("-ERR wrong number of arguments for 'rpush' command\r\n");
}

#[test]
fn keeps_a_list_per_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "one", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["RPUSH", "other", "element"]);
    client.expect_reply(":1\r\n");
}

/// Fills `list_key` with "a".."e" and returns a connection to query it.
fn server_with_five_elements(server: &Server) -> common::Client {
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "a", "b", "c", "d", "e"]);
    client.expect_reply(":5\r\n");

    client
}

#[test]
fn lists_a_range_of_elements() {
    let server = Server::start();
    let mut client = server_with_five_elements(&server);

    client.send(&["LRANGE", "list_key", "0", "1"]);
    client.expect_reply("*2\r\n$1\r\na\r\n$1\r\nb\r\n");

    client.send(&["LRANGE", "list_key", "2", "4"]);
    client.expect_reply("*3\r\n$1\r\nc\r\n$1\r\nd\r\n$1\r\ne\r\n");
}

#[test]
fn counts_negative_indexes_from_the_end() {
    let server = Server::start();
    let mut client = server_with_five_elements(&server);

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
    let mut client = server_with_five_elements(&server);

    client.send(&["LRANGE", "list_key", "-6", "-4"]);
    client.expect_reply("*2\r\n$1\r\na\r\n$1\r\nb\r\n");
}

#[test]
fn treats_a_stop_past_the_end_as_the_last_element() {
    let server = Server::start();
    let mut client = server_with_five_elements(&server);

    client.send(&["LRANGE", "list_key", "3", "100"]);
    client.expect_reply("*2\r\n$1\r\nd\r\n$1\r\ne\r\n");
}

#[test]
fn returns_an_empty_array_when_the_range_yields_nothing() {
    let server = Server::start();
    let mut client = server_with_five_elements(&server);

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
    let mut client = server_with_five_elements(&server);

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
fn rejects_a_push_onto_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["RPUSH", "foo", "element"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn rejects_a_get_of_a_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["GET", "list_key"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}
