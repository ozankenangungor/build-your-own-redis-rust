mod common;

use common::Server;

#[test]
fn reports_that_it_is_a_master() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "replication"]);
    let report = client.read_bulk_string();

    assert!(
        report.lines().any(|line| line == "role:master"),
        "no role in {report:?}",
    );
}

#[test]
fn heads_the_section_it_reports() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "replication"]);
    let report = client.read_bulk_string();

    assert!(report.starts_with("# Replication\r\n"), "{report:?}");
}

#[test]
fn separates_the_lines_it_reports_the_way_redis_does() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "replication"]);
    let report = client.read_bulk_string();

    // Every line ends in CRLF, including the last.
    assert!(report.ends_with("\r\n"), "{report:?}");
    assert!(!report.contains("\n\r"), "{report:?}");
}

#[test]
fn reports_every_section_when_asked_for_none() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO"]);
    let report = client.read_bulk_string();

    assert!(report.contains("role:master"), "{report:?}");
}

#[test]
fn accepts_any_casing_of_the_command_and_the_section() {
    let server = Server::start();
    let mut client = server.connect();

    for (name, section) in [
        ("INFO", "replication"),
        ("info", "REPLICATION"),
        ("InFo", "Replication"),
    ] {
        client.send(&[name, section]);
        assert!(client.read_bulk_string().contains("role:master"));
    }
}

#[test]
fn reports_nothing_for_a_section_it_does_not_have() {
    let server = Server::start();
    let mut client = server.connect();

    // Redis answers rather than refuses, since a section it lacks is not a
    // mistake on the client's part.
    client.send(&["INFO", "keyspace"]);
    client.expect_reply("$0\r\n\r\n");
}

#[test]
fn reports_each_of_the_sections_it_is_asked_for() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "keyspace", "replication"]);
    let report = client.read_bulk_string();

    assert!(report.contains("role:master"), "{report:?}");
}

#[test]
fn keeps_serving_the_connection_after_an_info() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "replication"]);
    client.read_bulk_string();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
