//! What a master says when asked how many replicas have caught up.

mod common;

use common::{FakeReplica, Server};

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
fn waits_the_time_out_for_replicas_that_are_not_there() {
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
    let _replicas: Vec<common::Client> = (0..3).map(|_| common::follow(&server).0).collect();
    let mut client = server.connect();

    client.send(&["WAIT", "3", "500"]);
    client.expect_reply(":3\r\n");
}

#[test]
fn counts_them_all_however_few_are_asked_for() {
    let server = Server::start();
    let _replicas: Vec<common::Client> = (0..3).map(|_| common::follow(&server).0).collect();
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
    let _replicas: Vec<common::Client> = (0..2).map(|_| common::follow(&server).0).collect();
    let mut client = server.connect();

    let started = std::time::Instant::now();

    client.send(&["WAIT", "9", "5000"]);
    client.expect_reply(":2\r\n");

    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn counts_the_replica_that_has_caught_up_with_a_write() {
    let server = Server::start();
    let _replica = FakeReplica::follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    client.send(&["WAIT", "1", "500"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn counts_every_replica_that_has_caught_up() {
    let server = Server::start();
    let _replicas: Vec<FakeReplica> = (0..3).map(|_| FakeReplica::follow(&server)).collect();
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    client.send(&["WAIT", "3", "500"]);
    client.expect_reply(":3\r\n");
}

#[test]
fn answers_as_soon_as_enough_replicas_have_caught_up() {
    let server = Server::start();
    let _replicas: Vec<FakeReplica> = (0..2).map(|_| FakeReplica::follow(&server)).collect();
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    let started = std::time::Instant::now();

    client.send(&["WAIT", "2", "5000"]);
    client.expect_reply(":2\r\n");

    // Waiting the timeout out when the answer is already known would be
    // waiting for nothing.
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn counts_only_the_replicas_that_answered_before_the_time_was_up() {
    let server = Server::start();
    let _keeping_up = FakeReplica::follow(&server);
    // This one takes what it is sent and never says a word about it.
    let _silent = common::follow(&server).0;
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    let started = std::time::Instant::now();

    client.send(&["WAIT", "2", "300"]);
    client.expect_reply(":1\r\n");

    // The one that never answers is waited for, and only for as long as asked.
    assert!(started.elapsed() >= std::time::Duration::from_millis(300));
}

#[test]
fn counts_them_afresh_for_each_write_it_is_asked_about() {
    let server = Server::start();
    let mut client = server.connect();

    let _first = FakeReplica::follow(&server);
    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");
    client.send(&["WAIT", "1", "500"]);
    client.expect_reply(":1\r\n");

    let _second = FakeReplica::follow(&server);
    client.send(&["SET", "bar", "456"]);
    client.expect_reply("+OK\r\n");
    client.send(&["WAIT", "2", "500"]);
    client.expect_reply(":2\r\n");
}

#[test]
fn stops_counting_a_replica_that_has_gone() {
    let server = Server::start();
    let _staying = FakeReplica::follow(&server);
    let leaving = FakeReplica::follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");
    client.send(&["WAIT", "2", "500"]);
    client.expect_reply(":2\r\n");

    drop(leaving);

    // Hanging up is heard on the connection, so the one left is the one
    // counted from here on.
    client.send(&["SET", "key", "again"]);
    client.expect_reply("+OK\r\n");
    client.send(&["WAIT", "2", "300"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn waits_however_long_it_takes_when_given_no_time_at_all() {
    let server = Server::start();
    let _replica = FakeReplica::follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    // No time at all is Redis's way of saying however long it takes, and what
    // it takes here is one round trip.
    client.send(&["WAIT", "1", "0"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn goes_on_waiting_when_given_no_time_and_no_answer() {
    let server = Server::start();
    let _keeping_up = FakeReplica::follow(&server);
    let _silent = common::follow(&server).0;
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    // One of the two will never answer, so this waits for good rather than
    // giving up and saying one.
    client.send(&["WAIT", "2", "0"]);
    client.expect_silence();
}

#[test]
fn takes_in_what_a_replica_said_alongside_its_request() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");

    let mut replica = server.connect();
    replica.send(&["PING"]);
    replica.expect_reply("+PONG\r\n");

    // Both in one packet, so the word about how far it has got arrives in the
    // same read as the request that makes it a replica.
    replica.send_raw(
        b"*3\r\n$5\r\nPSYNC\r\n$1\r\n?\r\n$2\r\n-1\r\n\
          *3\r\n$8\r\nREPLCONF\r\n$3\r\nACK\r\n$5\r\n10000\r\n",
    );
    replica.read_line();
    replica.read_file();

    client.send(&["SET", "bar", "456"]);
    client.expect_reply("+OK\r\n");

    // Were that word dropped along with the rest of the read, this would wait
    // the timeout out and answer nothing.
    let started = std::time::Instant::now();
    client.send(&["WAIT", "1", "1000"]);
    client.expect_reply(":1\r\n");

    assert!(started.elapsed() < std::time::Duration::from_millis(500));
}
