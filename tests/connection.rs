//! Behaviour of the connection loop itself, independent of any data type.

mod common;

use common::Server;

#[test]
fn responds_to_ping() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn responds_to_multiple_pings_on_one_connection() {
    let server = Server::start();
    let mut client = server.connect();

    for _ in 0..3 {
        client.send(&["PING"]);
        client.expect_reply("+PONG\r\n");
    }
}

#[test]
fn answers_every_command_arriving_in_one_read() {
    let server = Server::start();
    let mut client = server.connect();

    client.send_raw(b"*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n");
    client.expect_reply("+PONG\r\n+PONG\r\n");
}

#[test]
fn serves_concurrent_clients() {
    let server = Server::start();
    let mut clients: Vec<_> = (0..3).map(|_| server.connect()).collect();

    // Every client sends before any of them reads, so the server cannot serve
    // them one connection at a time.
    for client in &mut clients {
        client.send(&["PING"]);
    }
    for client in &mut clients {
        client.expect_reply("+PONG\r\n");
    }
}

#[test]
fn rejects_an_unknown_command_without_dropping_the_connection() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["NOPE"]);
    client.expect_reply("-ERR unknown command 'NOPE'\r\n");

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn survives_a_client_that_nests_values_without_end() {
    let server = Server::start();
    let mut onlooker = server.connect();
    let mut attacker = server.connect();

    // Nothing but array headers. Followed all the way down, each one is a step
    // further down the stack, and the whole server goes with it.
    attacker.try_send_raw(&b"*1\r\n".repeat(200_000));

    // The one connection is turned away, and every other one carries on.
    onlooker.send(&["PING"]);
    onlooker.expect_reply("+PONG\r\n");

    let mut newcomer = server.connect();
    newcomer.send(&["SET", "key", "value"]);
    newcomer.expect_reply("+OK\r\n");
    newcomer.send(&["GET", "key"]);
    newcomer.expect_reply("$5\r\nvalue\r\n");
}
