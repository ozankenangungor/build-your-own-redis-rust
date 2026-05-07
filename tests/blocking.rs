mod common;

use common::Server;
use std::thread::sleep;
use std::time::Duration;

/// Long enough for the server to have registered a `BLPOP` before the next
/// command is sent, so the tests exercise the waiting path rather than racing.
const SETTLE: Duration = Duration::from_millis(100);

#[test]
fn pops_without_waiting_when_an_element_is_already_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "foo"]);
    client.expect_reply(":1\r\n");

    client.send(&["BLPOP", "list_key", "0"]);
    client.expect_reply("*2\r\n$8\r\nlist_key\r\n$3\r\nfoo\r\n");
}

#[test]
fn waits_until_another_client_pushes() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut pusher = server.connect();

    blocked.send(&["BLPOP", "list_key", "0"]);
    sleep(SETTLE);

    pusher.send(&["RPUSH", "list_key", "foo"]);
    pusher.expect_reply(":1\r\n");

    blocked.expect_reply("*2\r\n$8\r\nlist_key\r\n$3\r\nfoo\r\n");
}

#[test]
fn serves_the_client_that_has_waited_the_longest() {
    let server = Server::start();
    let mut first = server.connect();
    let mut second = server.connect();
    let mut pusher = server.connect();

    first.send(&["BLPOP", "another_list_key", "0"]);
    sleep(SETTLE);
    second.send(&["BLPOP", "another_list_key", "0"]);
    sleep(SETTLE);

    pusher.send(&["RPUSH", "another_list_key", "one"]);
    pusher.expect_reply(":1\r\n");
    first.expect_reply("*2\r\n$16\r\nanother_list_key\r\n$3\r\none\r\n");

    pusher.send(&["RPUSH", "another_list_key", "two"]);
    pusher.expect_reply(":1\r\n");
    second.expect_reply("*2\r\n$16\r\nanother_list_key\r\n$3\r\ntwo\r\n");
}

#[test]
fn hands_a_single_push_to_only_one_waiting_client() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut pusher = server.connect();

    blocked.send(&["BLPOP", "list_key", "0"]);
    sleep(SETTLE);

    pusher.send(&["RPUSH", "list_key", "foo"]);
    pusher.expect_reply(":1\r\n");
    blocked.expect_reply("*2\r\n$8\r\nlist_key\r\n$3\r\nfoo\r\n");

    // The element went to the blocked client, so nothing is left behind.
    pusher.send(&["LLEN", "list_key"]);
    pusher.expect_reply(":0\r\n");
}

#[test]
fn leaves_the_extra_elements_of_a_multi_element_push() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut pusher = server.connect();

    blocked.send(&["BLPOP", "list_key", "0"]);
    sleep(SETTLE);

    pusher.send(&["RPUSH", "list_key", "a", "b", "c"]);
    pusher.expect_reply(":3\r\n");
    blocked.expect_reply("*2\r\n$8\r\nlist_key\r\n$1\r\na\r\n");

    pusher.send(&["LRANGE", "list_key", "0", "-1"]);
    pusher.expect_reply("*2\r\n$1\r\nb\r\n$1\r\nc\r\n");
}

#[test]
fn gives_up_once_the_timeout_passes() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["BLPOP", "list_key", "0.1"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn unblocks_when_an_element_arrives_before_the_timeout() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut pusher = server.connect();

    blocked.send(&["BLPOP", "list_key", "5"]);
    sleep(SETTLE);

    pusher.send(&["RPUSH", "list_key", "foo"]);
    pusher.expect_reply(":1\r\n");

    blocked.expect_reply("*2\r\n$8\r\nlist_key\r\n$3\r\nfoo\r\n");
}

#[test]
fn keeps_the_element_when_the_waiting_client_has_already_given_up() {
    let server = Server::start();
    let mut timed_out = server.connect();
    let mut pusher = server.connect();

    timed_out.send(&["BLPOP", "list_key", "0.1"]);
    timed_out.expect_reply("*-1\r\n");
    sleep(SETTLE);

    // The abandoned waiter is still queued, but it must not swallow the push.
    pusher.send(&["RPUSH", "list_key", "foo"]);
    pusher.expect_reply(":1\r\n");

    pusher.send(&["LRANGE", "list_key", "0", "-1"]);
    pusher.expect_reply("*1\r\n$3\r\nfoo\r\n");
}

#[test]
fn rejects_a_blocking_pop_on_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["BLPOP", "foo", "0"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn rejects_a_malformed_timeout() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["BLPOP", "list_key", "soon"]);
    client.expect_reply("-ERR timeout is not a float or out of range\r\n");

    client.send(&["BLPOP", "list_key", "-1"]);
    client.expect_reply("-ERR timeout is negative\r\n");
}
