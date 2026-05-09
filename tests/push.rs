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
fn keeps_a_list_per_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "one", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["RPUSH", "other", "element"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn prepends_elements_in_reverse_order() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["LPUSH", "list_key", "c"]);
    client.expect_reply(":1\r\n");

    client.send(&["LPUSH", "list_key", "b", "a"]);
    client.expect_reply(":3\r\n");

    client.send(&["LRANGE", "list_key", "0", "-1"]);
    client.expect_reply("*3\r\n$1\r\na\r\n$1\r\nb\r\n$1\r\nc\r\n");
}

#[test]
fn prepends_onto_a_list_built_from_the_right() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "b", "c"]);
    client.expect_reply(":2\r\n");

    client.send(&["LPUSH", "list_key", "a"]);
    client.expect_reply(":3\r\n");

    client.send(&["LRANGE", "list_key", "0", "-1"]);
    client.expect_reply("*3\r\n$1\r\na\r\n$1\r\nb\r\n$1\r\nc\r\n");
}

#[test]
fn rejects_a_push_without_elements() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key"]);
    client.expect_reply("-ERR wrong number of arguments for 'rpush' command\r\n");

    client.send(&["LPUSH", "list_key"]);
    client.expect_reply("-ERR wrong number of arguments for 'lpush' command\r\n");
}

#[test]
fn rejects_a_push_onto_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["RPUSH", "foo", "element"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");

    client.send(&["LPUSH", "foo", "element"]);
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
