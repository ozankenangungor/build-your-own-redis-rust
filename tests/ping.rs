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
