mod common;

use common::Server;

#[test]
fn echoes_its_argument() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ECHO", "hey"]);
    client.expect_reply("$3\r\nhey\r\n");
}

#[test]
fn accepts_any_casing_of_the_command_name() {
    let server = Server::start();
    let mut client = server.connect();

    for name in ["ECHO", "echo", "EcHo"] {
        client.send(&[name, "hey"]);
        client.expect_reply("$3\r\nhey\r\n");
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
