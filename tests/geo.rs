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
