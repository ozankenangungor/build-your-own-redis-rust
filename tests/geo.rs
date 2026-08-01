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
    for (longitude, latitude) in [
        ("-180", "-85.05112878"),
        ("180", "85.05112878"),
        ("-180", "85.05112878"),
        ("180", "-85.05112878"),
    ] {
        client.send(&["GEOADD", "places", longitude, latitude, "edge"]);
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

    for spelling in ["GEOADD", "geoadd", "GeoAdd"] {
        client.send(&[spelling, "places", "11.5030378", "48.1642721", "Munich"]);
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
