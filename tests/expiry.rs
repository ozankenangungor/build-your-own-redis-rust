mod common;

use common::Server;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn keeps_a_key_until_its_expiry_passes() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar", "PX", "100"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbar\r\n");

    sleep(Duration::from_millis(200));

    client.send(&["GET", "foo"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn accepts_seconds_and_any_casing_of_the_option() {
    let server = Server::start();
    let mut client = server.connect();

    for option in ["EX", "ex", "Ex"] {
        client.send(&["SET", "foo", "bar", option, "10"]);
        client.expect_reply("+OK\r\n");

        client.send(&["GET", "foo"]);
        client.expect_reply("$3\r\nbar\r\n");
    }
}

#[test]
fn keeps_a_key_forever_without_an_expiry_option() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    sleep(Duration::from_millis(200));

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbar\r\n");
}

#[test]
fn rejects_a_malformed_expiry() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar", "PX", "soon"]);
    client.expect_reply("-ERR value is not an integer or out of range\r\n");

    client.send(&["SET", "foo", "bar", "ZZ", "100"]);
    client.expect_reply("-ERR syntax error\r\n");
}
