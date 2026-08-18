//! What a client sees of the sorted set commands over a connection.
//!
//! How a set orders its members, what it counts as new, and what a window on it
//! comes to are measured in `src/store/sorted_sets.rs`, and how each command
//! reads its arguments in `src/commands/sorted_sets.rs`. What is left here is
//! what only a running server can show: that the commands are reachable, that a
//! member put in through one comes back out through another, and that the
//! answers are laid out on the wire as Redis lays them.

mod common;

use common::Server;

#[test]
fn takes_a_member_named_in_bytes_that_are_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    // A member is named in bytes, as a key is, and may hold anything at all.
    client.send_raw(b"*4\r\n$4\r\nZADD\r\n$6\r\nracers\r\n$3\r\n8.0\r\n$4\r\n\xff\x00\r\n\r\n");
    client.expect_reply(":1\r\n");
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

/// The set the tester builds, and a client that has just built it. It comes out
/// in the order baz, caz, paz, bar, foo.
fn with_a_zset(server: &Server) -> common::Client {
    let mut client = server.connect();

    for (score, member) in [
        ("100.0", "foo"),
        ("100.0", "bar"),
        ("20.0", "baz"),
        ("30.1", "caz"),
        ("40.2", "paz"),
    ] {
        client.send(&["ZADD", "zset_key", score, member]);
        client.expect_reply(":1\r\n");
    }

    client
}

#[test]
fn says_where_each_member_falls_in_the_order() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    // By score, and by name where the scores are equal: `bar` comes before
    // `foo` at a hundred apiece.
    for (member, rank) in [("baz", 0), ("caz", 1), ("paz", 2), ("bar", 3), ("foo", 4)] {
        client.send(&["ZRANK", "zset_key", member]);
        client.expect_reply(&format!(":{rank}\r\n"));
    }
}

#[test]
fn says_nothing_of_a_member_or_a_set_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "zset_key", "1.0", "member"]);
    client.expect_reply(":1\r\n");

    // Nothing to say is a bulk string that is not there, rather than a place of
    // its own among the numbers.
    client.send(&["ZRANK", "zset_key", "missing_member"]);
    client.expect_reply("$-1\r\n");
    client.send(&["ZRANK", "missing_key", "member"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn lists_the_members_between_two_places_in_the_order() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    client.send(&["ZRANGE", "zset_key", "2", "4"]);
    client.expect_reply("*3\r\n$3\r\npaz\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");

    client.send(&["ZRANGE", "zset_key", "0", "1"]);
    client.expect_reply("*2\r\n$3\r\nbaz\r\n$3\r\ncaz\r\n");

    client.send(&["ZRANGE", "zset_key", "-2", "-1"]);
    client.expect_reply("*2\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");
}

#[test]
fn lists_nothing_of_a_set_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    // An empty answer rather than no answer at all.
    client.send(&["ZRANGE", "missing_key", "0", "9"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn refuses_a_window_that_is_not_counted_in_whole_numbers() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    for (start, stop) in [("one", "2"), ("0", "two"), ("0.5", "2")] {
        client.send(&["ZRANGE", "zset_key", start, stop]);
        client.expect_reply("-ERR value is not an integer or out of range\r\n");
    }
}

#[test]
fn refuses_a_zrange_that_names_no_window() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZRANGE", "zset_key", "0"]);
    client.expect_reply("-ERR wrong number of arguments for 'zrange' command\r\n");
}

#[test]
fn takes_out_the_member_it_is_given() {
    let server = Server::start();
    let mut client = server.connect();

    for (score, member) in [("80.5", "foo"), ("50.3", "baz"), ("80.5", "bar")] {
        client.send(&["ZADD", "zset_key", score, member]);
        client.expect_reply(":1\r\n");
    }

    client.send(&["ZREM", "zset_key", "baz"]);
    client.expect_reply(":1\r\n");

    // Ordered bar, foo: baz went, and the rest keep the places their scores
    // gave them.
    client.send(&["ZRANGE", "zset_key", "0", "-1"]);
    client.expect_reply("*2\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");

    client.send(&["ZREM", "zset_key", "missing_member"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn lets_go_of_a_key_whose_set_it_has_emptied() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "1.0", "only"]);
    client.expect_reply(":1\r\n");
    client.send(&["ZREM", "racers", "only"]);
    client.expect_reply(":1\r\n");

    // Redis keeps no set with nothing in it, so the key goes with the last
    // member out of it.
    client.send(&["TYPE", "racers"]);
    client.expect_reply("+none\r\n");
    client.send(&["KEYS", "*"]);
    client.expect_reply("*0\r\n");
    client.send(&["ZCARD", "racers"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn refuses_a_zrem_that_names_no_member() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZREM"]);
    client.expect_reply("-ERR wrong number of arguments for 'zrem' command\r\n");
    client.send(&["ZREM", "racers"]);
    client.expect_reply("-ERR wrong number of arguments for 'zrem' command\r\n");
}

#[test]
fn says_the_score_a_member_was_given() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "zset_key", "30.1", "zset_member"]);
    client.expect_reply(":1\r\n");
    client.send(&["ZSCORE", "zset_key", "zset_member"]);
    client.expect_reply("$4\r\n30.1\r\n");

    // Updating the score is what the next ask should turn up.
    client.send(&["ZADD", "zset_key", "100.99", "zset_member"]);
    client.expect_reply(":0\r\n");
    client.send(&["ZSCORE", "zset_key", "zset_member"]);
    client.expect_reply("$6\r\n100.99\r\n");
}

#[test]
fn writes_a_score_back_out_the_way_redis_writes_one() {
    let server = Server::start();
    let mut client = server.connect();

    // The whole way round: the spelling a client gives, the number it is read
    // as, and the spelling it comes back out in.
    for (given, written) in [
        ("24.34", "24.34"),
        ("0.0043", "0.0043"),
        ("-1.5", "-1.5"),
        // A score that happens to be whole comes back without the point Redis
        // never prints, however it was spelled going in.
        ("20.0", "20"),
        ("8", "8"),
        ("1e3", "1000"),
        ("inf", "inf"),
        ("-inf", "-inf"),
    ] {
        client.send(&["ZADD", "racers", given, "member"]);
        client.read_reply();

        client.send(&["ZSCORE", "racers", "member"]);
        client.expect_reply(&format!("${}\r\n{written}\r\n", written.len()));
    }
}

#[test]
fn counts_the_members_a_set_holds() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    client.send(&["ZCARD", "zset_key"]);
    client.expect_reply(":5\r\n");

    client.send(&["ZCARD", "missing_key"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn refuses_a_zcard_that_names_no_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZCARD"]);
    client.expect_reply("-ERR wrong number of arguments for 'zcard' command\r\n");
    client.send(&["ZCARD", "racers", "extra"]);
    client.expect_reply("-ERR wrong number of arguments for 'zcard' command\r\n");
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
