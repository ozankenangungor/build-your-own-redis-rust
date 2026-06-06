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
fn queues_commands_instead_of_running_them() {
    let server = Server::start();
    let mut client = server.connect();
    let mut onlooker = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "41"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply("+QUEUED\r\n");

    // Nothing has touched the store, as another connection can attest.
    onlooker.send(&["GET", "foo"]);
    onlooker.expect_reply("$-1\r\n");
}

#[test]
fn executes_the_queued_commands_in_order() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    for command in [
        vec!["SET", "foo", "6"],
        vec!["INCR", "foo"],
        vec!["INCR", "bar"],
        vec!["GET", "bar"],
    ] {
        client.send(&command);
        client.expect_reply("+QUEUED\r\n");
    }

    // One reply per queued command, each keeping the type it would have had on
    // its own.
    client.send(&["EXEC"]);
    client.expect_reply("*4\r\n+OK\r\n:7\r\n:1\r\n$1\r\n1\r\n");
}

#[test]
fn leaves_what_the_transaction_did_behind_in_the_store() {
    let server = Server::start();
    let mut client = server.connect();
    let mut onlooker = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "6"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("*2\r\n+OK\r\n:7\r\n");

    onlooker.send(&["GET", "foo"]);
    onlooker.expect_reply("$1\r\n7\r\n");
}

#[test]
fn carries_a_failure_back_inside_the_replies() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "abc"]);
    client.expect_reply("+OK\r\n");
    client.send(&["SET", "bar", "41"]);
    client.expect_reply("+OK\r\n");

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply("+QUEUED\r\n");
    client.send(&["INCR", "bar"]);
    client.expect_reply("+QUEUED\r\n");

    // The failed command takes its place in the array like any other reply.
    client.send(&["EXEC"]);
    client.expect_reply("*2\r\n-ERR value is not an integer or out of range\r\n:42\r\n");

    // Failing left the one key alone and did not hold up the other.
    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nabc\r\n");
    client.send(&["GET", "bar"]);
    client.expect_reply("$2\r\n42\r\n");
}

#[test]
fn runs_the_commands_that_follow_a_failure() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    for command in [
        vec!["SET", "foo", "xyz"],
        vec!["INCR", "foo"],
        vec!["SET", "bar", "7"],
    ] {
        client.send(&command);
        client.expect_reply("+QUEUED\r\n");
    }

    client.send(&["EXEC"]);
    client.expect_reply(concat!(
        "*3\r\n+OK\r\n",
        "-ERR value is not an integer or out of range\r\n",
        "+OK\r\n",
    ));

    client.send(&["GET", "bar"]);
    client.expect_reply("$1\r\n7\r\n");
}

#[test]
fn carries_back_a_failure_of_the_wrong_type() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GET", "list_key"]);
    client.expect_reply("+QUEUED\r\n");
    client.send(&["LLEN", "list_key"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["EXEC"]);
    client.expect_reply(concat!(
        "*2\r\n",
        "-WRONGTYPE Operation against a key holding the wrong kind of value\r\n",
        ":1\r\n",
    ));
}

#[test]
fn throws_away_a_transaction_on_discard() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "41"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["DISCARD"]);
    client.expect_reply("+OK\r\n");

    // None of the queued commands ever ran.
    client.send(&["GET", "foo"]);
    client.expect_reply("$-1\r\n");

    // And the transaction is gone, so there is nothing left to discard.
    client.send(&["DISCARD"]);
    client.expect_reply("-ERR DISCARD without MULTI\r\n");
}

#[test]
fn refuses_to_discard_a_transaction_that_was_never_started() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["DISCARD"]);
    client.expect_reply("-ERR DISCARD without MULTI\r\n");
}

#[test]
fn leaves_the_connection_ready_for_another_transaction_after_a_discard() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "41"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["DISCARD"]);
    client.expect_reply("+OK\r\n");

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+QUEUED\r\n");

    // The abandoned command is not carried over into the new transaction.
    client.send(&["EXEC"]);
    client.expect_reply("*1\r\n+OK\r\n");

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbar\r\n");
}

#[test]
fn closes_the_transaction_once_it_has_run() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("*1\r\n+OK\r\n");

    // The queue is gone, so this command runs on its own instead of waiting.
    client.send(&["SET", "foo", "baz"]);
    client.expect_reply("+OK\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("-ERR EXEC without MULTI\r\n");
}

#[test]
fn queues_reads_as_well_as_writes() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");

    // Even a command that would only look at the store waits its turn.
    client.send(&["GET", "foo"]);
    client.expect_reply("+QUEUED\r\n");
}

#[test]
fn queues_nothing_for_the_connection_outside_the_transaction() {
    let server = Server::start();
    let mut inside = server.connect();
    let mut outside = server.connect();

    inside.send(&["MULTI"]);
    inside.expect_reply("+OK\r\n");

    inside.send(&["SET", "foo", "41"]);
    inside.expect_reply("+QUEUED\r\n");

    // The other client is not in a transaction, so its commands run at once.
    outside.send(&["SET", "bar", "baz"]);
    outside.expect_reply("+OK\r\n");

    outside.send(&["GET", "bar"]);
    outside.expect_reply("$3\r\nbaz\r\n");
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
