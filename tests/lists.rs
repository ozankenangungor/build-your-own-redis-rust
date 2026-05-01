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
fn keeps_a_list_per_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "one", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["RPUSH", "other", "element"]);
    client.expect_reply(":1\r\n");
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
