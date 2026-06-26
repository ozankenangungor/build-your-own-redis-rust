//! What a replica says to the master it was told to follow.

mod common;

use common::{FakeMaster, Server};

#[test]
fn greets_the_master_it_was_told_to_follow() {
    let master = FakeMaster::start();
    let _replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    // A master is spoken to in commands, so the greeting is a PING like any
    // client would send.
    master.accept().expect_reply("*1\r\n$4\r\nPING\r\n");
}

#[test]
fn leaves_the_master_alone_unless_told_to_follow_one() {
    let master = FakeMaster::start();
    let _server = Server::start_with(&["--port", "0"]);

    master.expect_no_one();
}

#[test]
fn serves_its_own_clients_while_greeting_the_master() {
    let master = FakeMaster::start();
    let replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    let mut client = replica.connect();
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn keeps_serving_when_the_master_cannot_be_reached() {
    // Nothing is listening on this port, so following it must fail.
    let replica = Server::start_with(&["--replicaof", "127.0.0.1 1"]);

    let mut client = replica.connect();
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
