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
fn counts_the_members_as_they_are_added_one_after_another() {
    let server = Server::start();
    let mut client = server.connect();

    // A member added is one the set had never held; a member updated is not,
    // however far its score moves.
    for (score, member, added) in [
        ("20.0", "zset_member1", ":1\r\n"),
        ("30.1", "zset_member2", ":1\r\n"),
        ("40.2", "zset_member3", ":1\r\n"),
        ("50.3", "zset_member4", ":1\r\n"),
        ("100.0", "zset_member1", ":0\r\n"),
        ("0.0043", "zset_member5", ":1\r\n"),
    ] {
        client.send(&["ZADD", "zset_key", score, member]);
        client.expect_reply(added);
    }
}

#[test]
fn keeps_the_sets_at_two_keys_apart() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "8.0", "Sam"]);
    client.expect_reply(":1\r\n");

    // The same name under another key is a member that key had never held.
    client.send(&["ZADD", "riders", "8.0", "Sam"]);
    client.expect_reply(":1\r\n");
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
fn says_where_each_member_falls_in_the_order() {
    let server = Server::start();
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

    client.send(&["ZRANK", "zset_key", "missing_member"]);
    client.expect_reply("$-1\r\n");
    client.send(&["ZRANK", "missing_key", "member"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn says_where_a_member_falls_once_its_score_has_moved() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZADD", "racers", "1.0", "first", "2.0", "second"]);
    client.expect_reply(":2\r\n");
    client.send(&["ZRANK", "racers", "first"]);
    client.expect_reply(":0\r\n");

    client.send(&["ZADD", "racers", "9.0", "first"]);
    client.expect_reply(":0\r\n");

    // Updating a score moves the member, and with it where it falls.
    client.send(&["ZRANK", "racers", "first"]);
    client.expect_reply(":1\r\n");
    client.send(&["ZRANK", "racers", "second"]);
    client.expect_reply(":0\r\n");
}

/// The set the tester builds, and a client that has just built it.
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
fn lists_the_members_between_two_places_in_the_order() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    // Ordered baz, caz, paz, bar, foo.
    client.send(&["ZRANGE", "zset_key", "2", "4"]);
    client.expect_reply("*3\r\n$3\r\npaz\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");

    client.send(&["ZRANGE", "zset_key", "0", "1"]);
    client.expect_reply("*2\r\n$3\r\nbaz\r\n$3\r\ncaz\r\n");

    client.send(&["ZRANGE", "zset_key", "1", "1"]);
    client.expect_reply("*1\r\n$3\r\ncaz\r\n");
}

#[test]
fn lists_as_far_as_the_set_goes_when_the_window_reaches_past_it() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    client.send(&["ZRANGE", "zset_key", "3", "99"]);
    client.expect_reply("*2\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");

    client.send(&["ZRANGE", "zset_key", "0", "99"]);
    client.expect_reply("*5\r\n$3\r\nbaz\r\n$3\r\ncaz\r\n$3\r\npaz\r\n$3\r\nbar\r\n$3\r\nfoo\r\n");
}

/// The set the tester builds for the places counted back from the end. It comes
/// out in the order foo, caz, paz, bar, baz.
fn with_a_zset_to_count_back_through(server: &Server) -> common::Client {
    let mut client = server.connect();

    for (score, member) in [
        ("20.0", "foo"),
        ("30.1", "bar"),
        ("40.2", "baz"),
        ("25.0", "paz"),
        ("25.0", "caz"),
    ] {
        client.send(&["ZADD", "zset_key", score, member]);
        client.expect_reply(":1\r\n");
    }

    client
}

#[test]
fn counts_a_place_back_from_the_end_when_it_is_given_one() {
    let server = Server::start();
    let mut client = with_a_zset_to_count_back_through(&server);

    client.send(&["ZRANGE", "zset_key", "2", "-1"]);
    client.expect_reply("*3\r\n$3\r\npaz\r\n$3\r\nbar\r\n$3\r\nbaz\r\n");

    client.send(&["ZRANGE", "zset_key", "-2", "-1"]);
    client.expect_reply("*2\r\n$3\r\nbar\r\n$3\r\nbaz\r\n");

    client.send(&["ZRANGE", "zset_key", "0", "-3"]);
    client.expect_reply("*3\r\n$3\r\nfoo\r\n$3\r\ncaz\r\n$3\r\npaz\r\n");

    client.send(&["ZRANGE", "zset_key", "-1", "-1"]);
    client.expect_reply("*1\r\n$3\r\nbaz\r\n");
}

#[test]
fn starts_at_the_first_member_when_it_is_told_to_count_back_past_it() {
    let server = Server::start();
    let mut client = with_a_zset_to_count_back_through(&server);

    // Reaching back further than the set is long lands at the start rather
    // than wrapping round to the end.
    client.send(&["ZRANGE", "zset_key", "-99", "-1"]);
    client.expect_reply("*5\r\n$3\r\nfoo\r\n$3\r\ncaz\r\n$3\r\npaz\r\n$3\r\nbar\r\n$3\r\nbaz\r\n");

    client.send(&["ZRANGE", "zset_key", "-99", "2"]);
    client.expect_reply("*3\r\n$3\r\nfoo\r\n$3\r\ncaz\r\n$3\r\npaz\r\n");
}

#[test]
fn lists_nothing_when_a_place_counted_back_ends_before_it_starts() {
    let server = Server::start();
    let mut client = with_a_zset_to_count_back_through(&server);

    client.send(&["ZRANGE", "zset_key", "-1", "-2"]);
    client.expect_reply("*0\r\n");
    client.send(&["ZRANGE", "zset_key", "3", "-3"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn lists_nothing_counted_back_through_a_set_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZRANGE", "missing_key", "-2", "-1"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn lists_nothing_when_the_window_falls_outside_the_set() {
    let server = Server::start();
    let mut client = with_a_zset(&server);

    // Starting past the last member, and ending before it starts.
    client.send(&["ZRANGE", "zset_key", "5", "9"]);
    client.expect_reply("*0\r\n");
    client.send(&["ZRANGE", "zset_key", "3", "2"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn lists_nothing_of_a_set_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

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
fn refuses_to_list_the_members_of_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "racers", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["ZRANGE", "racers", "0", "9"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn refuses_a_zrange_that_names_no_window() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZRANGE", "zset_key", "0"]);
    client.expect_reply("-ERR wrong number of arguments for 'zrange' command\r\n");
}

#[test]
fn counts_the_members_a_set_holds() {
    let server = Server::start();
    let mut client = server.connect();

    for (score, member) in [
        ("20.0", "zset_member1"),
        ("30.1", "zset_member2"),
        ("40.2", "zset_member3"),
        ("50.3", "zset_member4"),
    ] {
        client.send(&["ZADD", "zset_key", score, member]);
        client.expect_reply(":1\r\n");
    }

    client.send(&["ZCARD", "zset_key"]);
    client.expect_reply(":4\r\n");

    // Updating a member's score moves it; it does not make another of it.
    client.send(&["ZADD", "zset_key", "100.0", "zset_member1"]);
    client.expect_reply(":0\r\n");
    client.send(&["ZCARD", "zset_key"]);
    client.expect_reply(":4\r\n");
}

#[test]
fn counts_nothing_in_a_set_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZCARD", "missing_key"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn counts_the_members_as_the_set_grows() {
    let server = Server::start();
    let mut client = server.connect();

    for (member, count) in [("one", 1), ("two", 2), ("three", 3)] {
        client.send(&["ZADD", "racers", "1.0", member]);
        client.read_reply();
        client.send(&["ZCARD", "racers"]);
        client.expect_reply(&format!(":{count}\r\n"));
    }
}

#[test]
fn refuses_to_count_the_members_of_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "racers", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["ZCARD", "racers"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
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
fn refuses_to_place_a_member_of_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "racers", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["ZRANK", "racers", "Sam"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn refuses_a_zrank_that_names_no_member() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["ZRANK", "racers"]);
    client.expect_reply("-ERR wrong number of arguments for 'zrank' command\r\n");
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
