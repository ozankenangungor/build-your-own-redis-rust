mod common;

use common::Server;

#[test]
fn pops_the_first_element() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "one", "two", "three", "four", "five"]);
    client.expect_reply(":5\r\n");

    client.send(&["LPOP", "list_key"]);
    client.expect_reply("$3\r\none\r\n");

    client.send(&["LRANGE", "list_key", "0", "-1"]);
    client.expect_reply("*4\r\n$3\r\ntwo\r\n$5\r\nthree\r\n$4\r\nfour\r\n$4\r\nfive\r\n");
}

#[test]
fn pops_several_elements_at_once() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "one", "two", "three", "four", "five"]);
    client.expect_reply(":5\r\n");

    client.send(&["LPOP", "list_key", "2"]);
    client.expect_reply("*2\r\n$3\r\none\r\n$3\r\ntwo\r\n");

    client.send(&["LRANGE", "list_key", "0", "-1"]);
    client.expect_reply("*3\r\n$5\r\nthree\r\n$4\r\nfour\r\n$4\r\nfive\r\n");
}

#[test]
fn pops_the_whole_list_when_the_count_is_too_large() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "a", "b"]);
    client.expect_reply(":2\r\n");

    client.send(&["LPOP", "list_key", "10"]);
    client.expect_reply("*2\r\n$1\r\na\r\n$1\r\nb\r\n");

    client.send(&["LLEN", "list_key"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn returns_an_empty_array_for_a_count_that_removes_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "a"]);
    client.expect_reply(":1\r\n");

    client.send(&["LPOP", "list_key", "0"]);
    client.expect_reply("*0\r\n");

    client.send(&["LPOP", "missing_list_key", "3"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn returns_null_when_popping_a_missing_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["LPOP", "missing_list_key"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn drops_the_key_once_the_list_runs_empty() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "only"]);
    client.expect_reply(":1\r\n");

    client.send(&["LPOP", "list_key"]);
    client.expect_reply("$4\r\nonly\r\n");

    client.send(&["LLEN", "list_key"]);
    client.expect_reply(":0\r\n");

    client.send(&["LPOP", "list_key"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn rejects_a_negative_count() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["LPOP", "list_key", "-1"]);
    client.expect_reply("-ERR value is out of range, must be positive\r\n");

    client.send(&["LPOP", "list_key", "many"]);
    client.expect_reply("-ERR value is not an integer or out of range\r\n");
}

#[test]
fn rejects_a_pop_on_a_string() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "bar"]);
    client.expect_reply("+OK\r\n");

    client.send(&["LPOP", "foo"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}
