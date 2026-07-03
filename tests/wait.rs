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

#[test]
fn counts_the_replicas_that_are_following() {
    let server = Server::start();
    let _replicas: Vec<common::Client> = (0..3).map(|_| common::follow(&server)).collect();
    let mut client = server.connect();

    client.send(&["WAIT", "3", "500"]);
    client.expect_reply(":3\r\n");
}

#[test]
fn counts_them_all_however_few_are_asked_for() {
    let server = Server::start();
    let _replicas: Vec<common::Client> = (0..3).map(|_| common::follow(&server)).collect();
    let mut client = server.connect();

    // Nothing has been sent for them to catch up on, so all of them are as
    // far along as the master, whatever number the client had in mind.
    for asked in ["0", "1", "3", "9"] {
        client.send(&["WAIT", asked, "500"]);
        client.expect_reply(":3\r\n");
    }
}

#[test]
fn answers_at_once_when_there_is_nothing_to_catch_up_on() {
    let server = Server::start();
    let _replicas: Vec<common::Client> = (0..2).map(|_| common::follow(&server)).collect();
    let mut client = server.connect();

    let started = std::time::Instant::now();

    client.send(&["WAIT", "9", "5000"]);
    client.expect_reply(":2\r\n");

    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn stops_counting_a_replica_that_has_gone() {
    let server = Server::start();
    let staying = common::follow(&server);
    let leaving = common::follow(&server);
    let mut client = server.connect();

    client.send(&["WAIT", "2", "100"]);
    client.expect_reply(":2\r\n");

    drop(leaving);

    // A replica that has gone is only found to be gone when a write to it
    // fails, and the first write after it left may still be taken by the
    // operating system, so this takes as long as it takes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        client.send(&["SET", "key", "value"]);
        client.expect_reply("+OK\r\n");

        client.send(&["WAIT", "2", "100"]);
        let counted = client.read_reply();

        if counted == ":1\r\n" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "still counting {counted:?}",
        );
    }

    drop(staying);
}
