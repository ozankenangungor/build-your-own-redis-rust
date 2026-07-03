//! What a master says when asked how many replicas have caught up.

mod common;

use common::Server;

#[test]
fn waits_for_no_one_when_no_replica_is_following() {
    let server = Server::start();
    let mut client = server.connect();

    // Nobody to wait for, so there is nothing to wait out either.
    client.send(&["WAIT", "0", "60000"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn answers_at_once_however_many_replicas_are_asked_for() {
    let server = Server::start();
    let mut client = server.connect();

    let started = std::time::Instant::now();

    client.send(&["WAIT", "3", "5000"]);
    client.expect_reply(":0\r\n");

    // None will ever arrive, so waiting the timeout out would be waiting for
    // something that cannot happen.
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn counts_no_replicas_after_a_write() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");

    client.send(&["WAIT", "1", "500"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn rejects_a_wait_that_says_too_little() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [vec!["WAIT"], vec!["WAIT", "0"], vec!["WAIT", "0", "1", "2"]] {
        client.send(&command);
        client.expect_reply("-ERR wrong number of arguments for 'wait' command\r\n");
    }
}

#[test]
fn rejects_a_wait_measured_in_something_other_than_numbers() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [
        vec!["WAIT", "all", "60000"],
        vec!["WAIT", "0", "soon"],
        vec!["WAIT", "1.5", "60000"],
    ] {
        client.send(&command);
        client.expect_reply("-ERR value is not an integer or out of range\r\n");
    }
}

#[test]
fn rejects_a_wait_for_less_than_no_time() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["WAIT", "0", "-1"]);
    client.expect_reply("-ERR timeout is negative\r\n");
}

#[test]
fn accepts_any_casing_of_wait() {
    let server = Server::start();
    let mut client = server.connect();

    for name in ["WAIT", "wait", "WaIt"] {
        client.send(&[name, "0", "100"]);
        client.expect_reply(":0\r\n");
    }
}
