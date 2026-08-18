//! What a client sees of the geo commands over a connection.
//!
//! The arithmetic behind them — the score a place works out to, the place a
//! score reads back as, the distance between two of them — is measured in
//! `src/commands/geo.rs`, where each of those can be called on its own. What is
//! left here is what only a running server can show: that the commands are
//! reachable, that a place put in through one command comes back out through
//! another, and that the answers are laid out on the wire as Redis lays them.

mod common;

use common::Server;

#[test]
fn keeps_a_location_under_the_score_redis_keeps_it_under() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude, place, score) in [
        ("2.2944692", "48.8584625", "Paris", "3663832614298053"),
        ("-0.1277583", "51.5073509", "London", "2163557714754256"),
        ("100.5252", "13.7220", "Bangkok", "3962257306574459"),
        ("139.6917", "35.6895", "Tokyo", "4171231230197045"),
        ("-74.0060", "40.7128", "New York", "1791873974549446"),
        ("151.2093", "-33.8688", "Sydney", "3252046221964352"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, place]);
        client.expect_reply(":1\r\n");

        client.send(&["ZSCORE", "places", place]);
        client.expect_reply(&format!("${}\r\n{score}\r\n", score.len()));
    }
}

#[test]
fn keeps_the_places_of_the_world_in_the_order_of_their_scores() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude, place) in [
        ("-74.0060", "40.7128", "New York"),
        ("139.6917", "35.6895", "Tokyo"),
        ("2.2944692", "48.8584625", "Paris"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, place]);
        client.expect_reply(":1\r\n");
    }

    // Ordered by score now rather than by name, and the score follows the
    // whereabouts: New York, then Paris, then Tokyo, west to east.
    client.send(&["ZRANGE", "places", "0", "-1"]);
    client.expect_reply("*3\r\n$8\r\nNew York\r\n$5\r\nParis\r\n$5\r\nTokyo\r\n");
}

#[test]
fn keeps_each_of_the_locations_it_was_given() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&[
        "GEOADD",
        "places",
        "-0.0884948",
        "51.5006479",
        "London",
        "11.5030378",
        "48.1642721",
        "Munich",
    ]);
    client.expect_reply(":2\r\n");

    client.send(&["ZCARD", "places"]);
    client.expect_reply(":2\r\n");

    // London lies west of Munich, and the score follows the whereabouts.
    client.send(&["ZRANGE", "places", "0", "-1"]);
    client.expect_reply("*2\r\n$6\r\nLondon\r\n$6\r\nMunich\r\n");
}

#[test]
fn refuses_to_keep_a_location_under_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "places", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GEOADD", "places", "2.2944692", "48.8584625", "Paris"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn calls_what_it_made_a_sorted_set() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "2.2944692", "48.8584625", "Paris"]);
    client.read_reply();

    client.send(&["TYPE", "places"]);
    client.expect_reply("+zset\r\n");
}

/// How close a decoded place has to be to the one Redis decodes from the same
/// score: six places after the point, which is what the tester allows.
const AS_REDIS_DOES: f64 = 0.000001;

#[test]
fn says_where_each_of_the_places_asked_after_is() {
    let server = Server::start();
    let mut client = server.connect();

    // The scores the tester puts in, and where they lie.
    for (score, place) in [
        ("3663832614298053", "Foo"),
        ("3876464048901851", "Bar"),
        ("3468915414364476", "Baz"),
        ("3781709020344510", "Caz"),
    ] {
        client.send(&["ZADD", "location_key", score, place]);
        client.expect_reply(":1\r\n");
    }

    for (place, longitude, latitude) in [
        ("Foo", 2.294471561908722, 48.85846255040141),
        ("Bar", 49.12499874830245, 72.99100027813946),
        ("Baz", 10.08720070123672, 34.50260034107078),
        ("Caz", 41.12499922513961, 73.99100100464303),
    ] {
        client.send(&["GEOPOS", "location_key", place]);
        let found = client.read_reply();

        assert_near(&found, longitude, latitude, place);
    }
}

/// Asserts that a `GEOPOS` answer names a place near enough to this one.
fn assert_near(found: &str, longitude: f64, latitude: f64, place: &str) {
    let numbers: Vec<f64> = found
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();

    let [read_longitude, read_latitude] = numbers.as_slice() else {
        panic!("{place}: expected two numbers in {found:?}");
    };

    for (read, expected, which) in [
        (read_longitude, longitude, "longitude"),
        (read_latitude, latitude, "latitude"),
    ] {
        assert!(
            (read - expected).abs() < AS_REDIS_DOES,
            "{place}: {which} {read} is not {expected}"
        );
    }
}

#[test]
fn says_how_far_apart_two_places_are() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude, place) in [
        ("11.5030378", "48.164271", "Munich"),
        ("2.2944692", "48.8584625", "Paris"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, place]);
        client.expect_reply(":1\r\n");
    }

    client.send(&["GEODIST", "places", "Munich", "Paris"]);
    client.expect_reply("$11\r\n682477.7582\r\n");

    client.send(&["GEODIST", "places", "Munich", "Paris", "km"]);
    client.expect_reply("$8\r\n682.4778\r\n");
}

/// A client that has just put the tester's three places on the map.
fn with_three_places(server: &Server) -> common::Client {
    let mut client = server.connect();

    for (longitude, latitude, place) in [
        ("11.5030378", "48.164271", "Munich"),
        ("2.2944692", "48.8584625", "Paris"),
        ("-0.0884948", "51.5006479", "London"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, place]);
        client.expect_reply(":1\r\n");
    }

    client
}

#[test]
fn finds_the_places_within_the_way_it_was_given() {
    let server = Server::start();
    let mut client = with_three_places(&server);

    client.send(&[
        "GEOSEARCH",
        "places",
        "FROMLONLAT",
        "2",
        "48",
        "BYRADIUS",
        "100000",
        "m",
    ]);
    client.expect_reply("*1\r\n$5\r\nParis\r\n");
}

#[test]
fn finds_nothing_where_there_is_nothing_to_find() {
    let server = Server::start();
    let mut client = with_three_places(&server);

    // Nothing found is an empty answer rather than no answer at all, whether
    // the search came up short or the key was never there.
    client.send(&[
        "GEOSEARCH",
        "places",
        "FROMLONLAT",
        "-160",
        "0",
        "BYRADIUS",
        "1000",
        "m",
    ]);
    client.expect_reply("*0\r\n");

    client.send(&[
        "GEOSEARCH",
        "missing_key",
        "FROMLONLAT",
        "2",
        "48",
        "BYRADIUS",
        "500000",
        "m",
    ]);
    client.expect_reply("*0\r\n");
}

#[test]
fn accepts_any_casing_of_the_command() {
    let server = Server::start();
    let mut client = server.connect();

    for (spelling, member) in [("GEOADD", "one"), ("geoadd", "two"), ("GeoAdd", "three")] {
        client.send(&[spelling, "places", "11.5030378", "48.1642721", member]);
        client.expect_reply(":1\r\n");
    }
}

#[test]
fn keeps_serving_the_connection_after_a_geoadd() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "11.5030378", "48.1642721", "Munich"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
