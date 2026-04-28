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
