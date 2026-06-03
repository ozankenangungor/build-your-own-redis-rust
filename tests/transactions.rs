mod common;

use common::Server;

#[test]
fn starts_a_transaction() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");
}

#[test]
fn accepts_any_casing_of_multi() {
    let server = Server::start();
    let mut client = server.connect();

    for name in ["MULTI", "multi", "MuLtI"] {
        client.send(&[name]);
        client.expect_reply("+OK\r\n");

        client.send(&["EXEC"]);
        client.expect_reply("*0\r\n");
    }
}

#[test]
fn executes_an_empty_transaction() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("*0\r\n");

    // The transaction is over, so the next `EXEC` has none to execute.
    client.send(&["EXEC"]);
    client.expect_reply("-ERR EXEC without MULTI\r\n");
}

#[test]
fn keeps_a_transaction_to_the_connection_that_started_it() {
    let server = Server::start();
    let mut inside = server.connect();
    let mut outside = server.connect();

    inside.send(&["MULTI"]);
    inside.expect_reply("+OK\r\n");

    outside.send(&["EXEC"]);
    outside.expect_reply("-ERR EXEC without MULTI\r\n");

    inside.send(&["EXEC"]);
    inside.expect_reply("*0\r\n");
}

#[test]
fn refuses_to_start_a_transaction_inside_one() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["MULTI"]);
    client.expect_reply("-ERR MULTI calls can not be nested\r\n");

    // The transaction that was already open is still the one running.
    client.send(&["EXEC"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn refuses_to_execute_a_transaction_that_was_never_started() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["EXEC"]);
    client.expect_reply("-ERR EXEC without MULTI\r\n");
}

#[test]
fn rejects_a_multi_with_arguments() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI", "foo"]);
    client.expect_reply("-ERR wrong number of arguments for 'multi' command\r\n");

    client.send(&["EXEC", "foo"]);
    client.expect_reply("-ERR wrong number of arguments for 'exec' command\r\n");
}
