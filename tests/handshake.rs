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
fn tells_the_master_where_to_reach_it_and_what_it_can_do() {
    let master = FakeMaster::start();
    let replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    let mut conversation = master.accept();

    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");
    conversation.send_raw(b"+PONG\r\n");

    // The port it reports is the one it is really listening on, which is not
    // always the one it was asked for.
    let port = replica.port().to_string();
    conversation.expect_reply(&format!(
        "*3\r\n$8\r\nREPLCONF\r\n$14\r\nlistening-port\r\n${}\r\n{port}\r\n",
        port.len(),
    ));
    conversation.send_raw(b"+OK\r\n");

    conversation.expect_reply("*3\r\n$8\r\nREPLCONF\r\n$4\r\ncapa\r\n$6\r\npsync2\r\n");
    conversation.send_raw(b"+OK\r\n");
}

#[test]
fn asks_the_master_for_its_whole_history() {
    let master = FakeMaster::start();
    let _replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    let mut conversation = master.accept();
    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");
    conversation.send_raw(b"+PONG\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");
    conversation.read_command();
    conversation.send_raw(b"+OK\r\n");

    // Following no one so far, it knows neither whose history to ask for nor
    // where in it to start.
    conversation.expect_reply("*3\r\n$5\r\nPSYNC\r\n$1\r\n?\r\n$2\r\n-1\r\n");
    conversation.send_raw(b"+FULLRESYNC 8371b4fb1155b71f4a04d3e1bc3e18c4d990aeeb 0\r\n");
}

#[test]
fn waits_for_each_answer_before_asking_the_next_thing() {
    let master = FakeMaster::start();
    let _replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    let mut conversation = master.accept();
    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");
    conversation.send_raw(b"+PONG\r\n");
    conversation.read_command();

    // The first REPLCONF has gone unanswered, so the second must not follow.
    conversation.expect_silence();
}

#[test]
fn waits_for_the_master_to_answer_before_going_on() {
    let master = FakeMaster::start();
    let _replica = Server::start_with(&["--replicaof", &format!("localhost {}", master.port())]);

    let mut conversation = master.accept();
    conversation.expect_reply("*1\r\n$4\r\nPING\r\n");

    // Nothing has answered the greeting, so nothing more should arrive.
    conversation.expect_silence();
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
