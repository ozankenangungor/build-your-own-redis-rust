mod common;

use common::Server;

#[test]
fn counts_the_member_it_adds_to_a_set_that_was_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "zset_key", "10.0", "zset_member"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn counts_the_member_it_adds_to_a_set_that_was() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "6.1", "Ford"]);
    client.expect_reply(":1\r\n");
    client.send(&["ZADD", "racers", "8.2", "Royce"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn counts_nothing_for_a_member_the_set_already_held() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "8.0", "Sam"]);
    client.expect_reply(":1\r\n");

    // The score moves, but the set is holding the names it was already holding.
    client.send(&["ZADD", "racers", "9.5", "Sam"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn counts_each_of_the_members_named_in_one_go() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "6.1", "Ford", "8.2", "Royce"]);
    client.expect_reply(":2\r\n");
}

#[test]
fn takes_the_scores_redis_takes() {
    let server = Server::start();
    let mut client = server.connect();

    for (score, member) in [
        ("8", "whole"),
        ("8.0", "written-out"),
        ("-1.5", "below"),
        ("1e3", "in-tens"),
        ("inf", "most"),
        ("-inf", "least"),
    ] {
        client.send(&["ZADD", "racers", score, member]);
        client.expect_reply(":1\r\n");
    }
}

#[test]
fn refuses_a_score_that_is_not_a_number() {
    let server = Server::start();
    let mut client = server.connect();

    for score in ["eight", "", "nan"] {
        client.send(&["ZADD", "racers", score, "Sam"]);
        client.expect_reply("-ERR value is not a valid float\r\n");
    }
}

#[test]
fn takes_a_member_named_in_bytes_that_are_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    // A member is named in bytes, as a key is, and may hold anything at all.
    client.send_raw(b"*4\r\n$4\r\nZADD\r\n$6\r\nracers\r\n$3\r\n8.0\r\n$4\r\n\xff\x00\r\n\r\n");
    client.expect_reply(":1\r\n");
}

#[test]
fn refuses_a_zadd_that_is_missing_a_score_or_a_member() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD"]);
    client.expect_reply("-ERR wrong number of arguments for 'zadd' command\r\n");
    client.send(&["ZADD", "racers"]);
    client.expect_reply("-ERR wrong number of arguments for 'zadd' command\r\n");
    client.send(&["ZADD", "racers", "8.0"]);
    client.expect_reply("-ERR syntax error\r\n");
}

#[test]
fn refuses_to_add_to_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "racers", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["ZADD", "racers", "8.0", "Sam"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn calls_what_it_made_a_sorted_set() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "8.0", "Sam"]);
    client.read_reply();

    client.send(&["TYPE", "racers"]);
    client.expect_reply("+zset\r\n");
}

#[test]
fn lists_the_key_it_made_alongside_the_others() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "8.0", "Sam"]);
    client.read_reply();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*1\r\n$6\r\nracers\r\n");
}

#[test]
fn accepts_any_casing_of_the_command() {
    let server = Server::start();
    let mut client = server.connect();

    for (spelling, member) in [("ZADD", "one"), ("zadd", "two"), ("Zadd", "three")] {
        client.send(&[spelling, "racers", "8.0", member]);
        client.expect_reply(":1\r\n");
    }
}
