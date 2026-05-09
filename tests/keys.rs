mod common;

use common::Server;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn reports_the_type_of_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "some_key", "foo"]);
    client.expect_reply("+OK\r\n");

    client.send(&["TYPE", "some_key"]);
    client.expect_reply("+string\r\n");
}

#[test]
fn reports_none_for_a_missing_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["TYPE", "missing_key"]);
    client.expect_reply("+none\r\n");
}

#[test]
fn reports_the_type_of_a_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["TYPE", "list_key"]);
    client.expect_reply("+list\r\n");
}

#[test]
fn reports_none_once_a_key_has_expired() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "some_key", "foo", "PX", "100"]);
    client.expect_reply("+OK\r\n");

    client.send(&["TYPE", "some_key"]);
    client.expect_reply("+string\r\n");

    sleep(Duration::from_millis(200));

    client.send(&["TYPE", "some_key"]);
    client.expect_reply("+none\r\n");
}

#[test]
fn rejects_a_type_query_without_a_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["TYPE"]);
    client.expect_reply("-ERR wrong number of arguments for 'type' command\r\n");
}
