//! What a master passes on to the replicas following it.

mod common;

use common::Server;

/// Takes a connection through the handshake and hands back the replica's end
/// of it, ready to be told what changes.
fn follow(server: &Server) -> common::Client {
    let mut replica = server.connect();

    replica.send(&["PING"]);
    replica.expect_reply("+PONG\r\n");
    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["PSYNC", "?", "-1"]);
    replica.read_line();
    replica.read_file();

    replica
}

#[test]
fn passes_a_write_on_to_the_replica() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "foo", "1"]);
    client.expect_reply("+OK\r\n");

    replica.expect_reply("*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n");
}

#[test]
fn passes_writes_on_in_the_order_they_were_made() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    for (key, value) in [("foo", "1"), ("bar", "2"), ("baz", "3")] {
        client.send(&["SET", key, value]);
        client.expect_reply("+OK\r\n");
    }

    for (key, value) in [("foo", "1"), ("bar", "2"), ("baz", "3")] {
        replica.expect_reply(&format!(
            "*3\r\n$3\r\nSET\r\n${}\r\n{key}\r\n${}\r\n{value}\r\n",
            key.len(),
            value.len(),
        ));
    }
}

#[test]
fn passes_on_every_command_that_changes_the_store() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    client.send(&["INCR", "counter"]);
    client.expect_reply(":1\r\n");
    replica.expect_reply("*2\r\n$4\r\nINCR\r\n$7\r\ncounter\r\n");

    client.send(&["RPUSH", "list", "a"]);
    client.expect_reply(":1\r\n");
    replica.expect_reply("*3\r\n$5\r\nRPUSH\r\n$4\r\nlist\r\n$1\r\na\r\n");

    client.send(&["LPOP", "list"]);
    client.expect_reply("$1\r\na\r\n");
    replica.expect_reply("*2\r\n$4\r\nLPOP\r\n$4\r\nlist\r\n");
}

#[test]
fn keeps_reads_to_itself() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    // None of these leaves the store any different, so a replica copying it
    // has no need to hear about them.
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
    client.send(&["ECHO", "hello"]);
    client.expect_reply("$5\r\nhello\r\n");
    client.send(&["GET", "missing"]);
    client.expect_reply("$-1\r\n");
    client.send(&["TYPE", "missing"]);
    client.expect_reply("+none\r\n");
    client.send(&["INFO", "replication"]);
    client.read_bulk_string();

    replica.expect_silence();
}

#[test]
fn keeps_a_write_that_was_turned_down_to_itself() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");
    replica.expect_reply("*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n");

    // The store is as it was, so there is nothing to pass on.
    client.send(&["INCR", "key"]);
    client.expect_reply("-ERR value is not an integer or out of range\r\n");

    replica.expect_silence();
}

#[test]
fn passes_on_what_a_transaction_changed() {
    let server = Server::start();
    let mut replica = follow(&server);
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");
    client.send(&["SET", "foo", "1"]);
    client.expect_reply("+QUEUED\r\n");
    client.send(&["GET", "foo"]);
    client.expect_reply("+QUEUED\r\n");
    client.send(&["INCR", "foo"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("*3\r\n+OK\r\n$1\r\n1\r\n:2\r\n");

    // The commands reach the replica as they run, and the read among them
    // does not.
    replica.expect_reply("*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n");
    replica.expect_reply("*2\r\n$4\r\nINCR\r\n$3\r\nfoo\r\n");
}

#[test]
fn passes_nothing_on_when_no_replica_is_following() {
    let server = Server::start();
    let mut client = server.connect();

    // Nothing to pass on to, and nothing that goes wrong for want of anyone.
    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");
    client.send(&["GET", "key"]);
    client.expect_reply("$5\r\nvalue\r\n");
}

#[test]
fn goes_on_serving_when_a_replica_hangs_up() {
    let server = Server::start();
    let replica = follow(&server);
    let mut client = server.connect();

    drop(replica);

    for _ in 0..3 {
        client.send(&["SET", "key", "value"]);
        client.expect_reply("+OK\r\n");
    }
}

#[test]
fn passes_a_write_on_to_every_replica() {
    let server = Server::start();
    let mut replicas: Vec<common::Client> = (0..3).map(|_| follow(&server)).collect();
    let mut client = server.connect();

    for (key, value) in [("foo", "1"), ("bar", "2"), ("baz", "3")] {
        client.send(&["SET", key, value]);
        client.expect_reply("+OK\r\n");
    }

    // Each of them is a copy of the same store, so each hears the same things
    // in the same order.
    for replica in &mut replicas {
        for (key, value) in [("foo", "1"), ("bar", "2"), ("baz", "3")] {
            replica.expect_reply(&format!(
                "*3\r\n$3\r\nSET\r\n${}\r\n{key}\r\n${}\r\n{value}\r\n",
                key.len(),
                value.len(),
            ));
        }
    }
}

#[test]
fn tells_a_replica_only_of_what_followed_its_arrival() {
    let server = Server::start();
    let mut early = follow(&server);
    let mut client = server.connect();

    client.send(&["SET", "before", "1"]);
    client.expect_reply("+OK\r\n");
    early.expect_reply("*3\r\n$3\r\nSET\r\n$6\r\nbefore\r\n$1\r\n1\r\n");

    let mut late = follow(&server);

    client.send(&["SET", "after", "2"]);
    client.expect_reply("+OK\r\n");

    // The one that arrived later hears this and nothing before it.
    late.expect_reply("*3\r\n$3\r\nSET\r\n$5\r\nafter\r\n$1\r\n2\r\n");
    early.expect_reply("*3\r\n$3\r\nSET\r\n$5\r\nafter\r\n$1\r\n2\r\n");
}

#[test]
fn goes_on_telling_the_others_when_one_replica_hangs_up() {
    let server = Server::start();
    let mut staying = follow(&server);
    let leaving = follow(&server);
    let mut client = server.connect();

    drop(leaving);

    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");

    staying.expect_reply("*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n");
}
