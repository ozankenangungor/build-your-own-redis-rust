mod common;

use common::Server;

#[test]
fn counts_the_location_it_is_given() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "11.5030378", "48.1642721", "Munich"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn counts_each_of_the_locations_named_in_one_go() {
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
}

#[test]
fn keeps_the_location_as_a_member_of_a_sorted_set() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "2.2944692", "48.8584625", "Paris"]);
    client.expect_reply(":1\r\n");

    client.send(&["ZRANGE", "places", "0", "-1"]);
    client.expect_reply("*1\r\n$5\r\nParis\r\n");
}

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
fn counts_nothing_for_a_place_the_key_already_held() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "2.2944692", "48.8584625", "Paris"]);
    client.expect_reply(":1\r\n");
    client.send(&["GEOADD", "places", "11.5030378", "48.1642721", "Paris"]);
    client.expect_reply(":0\r\n");

    client.send(&["ZCARD", "places"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn keeps_nothing_when_one_of_the_locations_will_not_read() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&[
        "GEOADD", "places", "11.5", "48.1", "Munich", "181", "0.3", "nowhere",
    ]);
    client.read_line();

    // The good location beside the bad one went nowhere either.
    client.send(&["ZCARD", "places"]);
    client.expect_reply(":0\r\n");
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

/// A client that has just put the tester's two places on the map.
fn with_two_places(server: &Server) -> common::Client {
    let mut client = server.connect();

    for (longitude, latitude, place) in [
        ("-0.0884948", "51.5006479", "London"),
        ("11.5030378", "48.1642721", "Munich"),
    ] {
        client.send(&["GEOADD", "location_key", longitude, latitude, place]);
        client.expect_reply(":1\r\n");
    }

    client
}

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

        assert_near(&found, longitude, latitude, AS_REDIS_DOES, place);
    }
}

/// How close a decoded place has to be to the one Redis decodes from the same
/// score: six places after the point, which is what the tester allows.
const AS_REDIS_DOES: f64 = 0.000001;

/// How close a place read back has to be to where it went in. Working a place
/// into a score and back loses a little of it: what comes back is the middle of
/// the square the place fell in, so it is the same place only to within the
/// width of one square.
const WITHIN_A_SQUARE: f64 = 360.0 / 67108864.0;

/// Asserts that a `GEOPOS` answer names a place within `close_enough` of this
/// one, either way.
fn assert_near(found: &str, longitude: f64, latitude: f64, close_enough: f64, place: &str) {
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
            (read - expected).abs() < close_enough,
            "{place}: {which} {read} is not {expected}"
        );
    }
}

#[test]
fn says_a_place_is_where_it_was_put() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "location_key", "2.2944692", "48.8584625", "Paris"]);
    client.expect_reply(":1\r\n");

    client.send(&["GEOPOS", "location_key", "Paris"]);
    let found = client.read_reply();

    // Near enough to where it went in that no map would tell them apart.
    assert_near(&found, 2.2944692, 48.8584625, WITHIN_A_SQUARE, "Paris");
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

    // The same distance whichever way round it is asked, and none at all from
    // a place to itself.
    client.send(&["GEODIST", "places", "Paris", "Munich"]);
    client.expect_reply("$11\r\n682477.7582\r\n");
    client.send(&["GEODIST", "places", "Munich", "Munich"]);
    client.expect_reply("$6\r\n0.0000\r\n");
}

#[test]
fn measures_in_the_unit_it_was_asked_for() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude, place) in [
        ("11.5030378", "48.164271", "Munich"),
        ("2.2944692", "48.8584625", "Paris"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, place]);
        client.expect_reply(":1\r\n");
    }

    for (unit, expected) in [("m", "682477.7582"), ("km", "682.4778")] {
        client.send(&["GEODIST", "places", "Munich", "Paris", unit]);
        client.expect_reply(&format!("${}\r\n{expected}\r\n", expected.len()));
    }
}

#[test]
fn says_nothing_of_the_distance_to_a_place_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "11.5030378", "48.164271", "Munich"]);
    client.expect_reply(":1\r\n");

    client.send(&["GEODIST", "places", "Munich", "nowhere"]);
    client.expect_reply("$-1\r\n");
    client.send(&["GEODIST", "missing_key", "Munich", "Paris"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn refuses_a_geodist_that_names_fewer_than_two_places() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEODIST", "places", "Munich"]);
    client.expect_reply("-ERR wrong number of arguments for 'geodist' command\r\n");
}

#[test]
fn says_nothing_of_a_place_the_key_does_not_hold() {
    let server = Server::start();
    let mut client = with_two_places(&server);

    client.send(&["GEOPOS", "location_key", "missing_location"]);
    client.expect_reply("*1\r\n*-1\r\n");

    // The answers still line up with the asking: a place that is not there is
    // answered with nothing rather than left out.
    client.send(&["GEOPOS", "location_key", "London", "missing_location"]);
    let found = client.read_reply();

    assert!(found.starts_with("*2\r\n*2\r\n"), "{found:?}");
    assert!(found.ends_with("*-1\r\n"), "{found:?}");
}

#[test]
fn answers_for_every_place_asked_after_of_a_key_that_is_not_there() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOPOS", "missing_key", "London", "Munich"]);
    client.expect_reply("*2\r\n*-1\r\n*-1\r\n");
}

#[test]
fn refuses_a_geopos_that_names_no_place() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOPOS"]);
    client.expect_reply("-ERR wrong number of arguments for 'geopos' command\r\n");
    client.send(&["GEOPOS", "location_key"]);
    client.expect_reply("-ERR wrong number of arguments for 'geopos' command\r\n");
}

#[test]
fn refuses_to_place_a_member_of_a_key_holding_something_else() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "places", "a string"]);
    client.expect_reply("+OK\r\n");

    client.send(&["GEOPOS", "places", "London"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn refuses_a_location_naming_no_place_on_the_earth() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude, wrong) in [
        ("200", "100", "longitude"),
        ("180", "90", "latitude"),
        ("181", "0.3", "longitude"),
        ("0", "-85.05112879", "latitude"),
        ("-180.1", "0", "longitude"),
    ] {
        client.send(&["GEOADD", "location_key", longitude, latitude, "foo"]);
        let said = client.read_line();

        assert!(said.starts_with("-ERR "), "{said:?}");
        assert!(said.contains(wrong), "{said:?} should name the {wrong}");
    }
}

#[test]
fn takes_a_location_on_the_very_edge_of_the_world() {
    let server = Server::start();
    let mut client = server.connect();

    // Both limits are the last place that counts, not the first that does not.
    for (longitude, latitude, corner) in [
        ("-180", "-85.05112878", "south-west"),
        ("180", "85.05112878", "north-east"),
        ("-180", "85.05112878", "north-west"),
        ("180", "-85.05112878", "south-east"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, corner]);
        client.expect_reply(":1\r\n");
    }
}

#[test]
fn refuses_coordinates_that_are_not_numbers() {
    let server = Server::start();
    let mut client = server.connect();

    for (longitude, latitude) in [("east", "0"), ("0", "north"), ("nan", "0")] {
        client.send(&["GEOADD", "places", longitude, latitude, "nowhere"]);
        client.expect_reply("-ERR value is not a valid float\r\n");
    }
}

#[test]
fn counts_nothing_when_one_of_the_locations_will_not_read() {
    let server = Server::start();
    let mut client = server.connect();

    // The command is refused whole: one location naming no place is no reason
    // to take in the good one beside it.
    client.send(&[
        "GEOADD", "places", "11.5", "48.1", "Munich", "181", "0.3", "nowhere",
    ]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR "), "{said:?}");
    assert!(said.contains("longitude"), "{said:?}");
}

#[test]
fn refuses_a_geoadd_short_of_one_whole_location() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [
        ["GEOADD"].as_slice(),
        ["GEOADD", "places"].as_slice(),
        ["GEOADD", "places", "11.5"].as_slice(),
        ["GEOADD", "places", "11.5", "48.1"].as_slice(),
    ] {
        client.send(command);
        client.expect_reply("-ERR wrong number of arguments for 'geoadd' command\r\n");
    }
}

#[test]
fn refuses_locations_that_do_not_pair_up() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["GEOADD", "places", "11.5", "48.1", "Munich", "0.0"]);
    client.expect_reply("-ERR syntax error\r\n");
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
