//! What a master makes of the replicas introducing themselves to it.

mod common;

use common::Server;

#[test]
fn takes_the_port_a_replica_says_it_listens_on() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn takes_what_a_replica_says_it_can_do() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn takes_settings_it_has_never_heard_of() {
    let server = Server::start();
    let mut replica = server.connect();

    // Redis takes these in pairs and passes over the ones it has no use for,
    // so that a newer replica can still introduce itself to an older master.
    replica.send(&["REPLCONF", "ip-address", "127.0.0.1", "capa", "eof"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn accepts_any_casing_of_replconf() {
    let server = Server::start();
    let mut replica = server.connect();

    for name in ["REPLCONF", "replconf", "ReplConf"] {
        replica.send(&[name, "capa", "psync2"]);
        replica.expect_reply("+OK\r\n");
    }
}

#[test]
fn rejects_a_setting_with_nothing_set_to_it() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "listening-port"]);
    replica.expect_reply("-ERR wrong number of arguments for 'replconf' command\r\n");
}

#[test]
fn takes_a_replconf_that_says_nothing() {
    let server = Server::start();
    let mut replica = server.connect();

    // No pairs is a fine number of pairs.
    replica.send(&["REPLCONF"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn goes_on_serving_the_connection_a_replica_introduced_itself_on() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PING"]);
    replica.expect_reply("+PONG\r\n");
    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");

    // Nothing about the handshake so far closes the connection off.
    replica.send(&["SET", "key", "value"]);
    replica.expect_reply("+OK\r\n");
}
