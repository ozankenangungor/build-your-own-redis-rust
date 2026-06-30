//! What a master makes of the replicas introducing themselves to it.

mod common;

use common::Server;

#[test]
fn takes_the_port_a_replica_says_it_listens_on() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn takes_what_a_replica_says_it_can_do() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn takes_settings_it_has_never_heard_of() {
    let server = Server::start();
    let mut replica = server.connect();

    // Redis takes these in pairs and passes over the ones it has no use for,
    // so that a newer replica can still introduce itself to an older master.
    replica.send(&["REPLCONF", "ip-address", "127.0.0.1", "capa", "eof"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn accepts_any_casing_of_replconf() {
    let server = Server::start();
    let mut replica = server.connect();

    for name in ["REPLCONF", "replconf", "ReplConf"] {
        replica.send(&[name, "capa", "psync2"]);
        replica.expect_reply("+OK\r\n");
    }
}

#[test]
fn rejects_a_setting_with_nothing_set_to_it() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["REPLCONF", "listening-port"]);
    replica.expect_reply("-ERR wrong number of arguments for 'replconf' command\r\n");
}

#[test]
fn takes_a_replconf_that_says_nothing() {
    let server = Server::start();
    let mut replica = server.connect();

    // No pairs is a fine number of pairs.
    replica.send(&["REPLCONF"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn goes_on_serving_the_connection_a_replica_introduced_itself_on() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PING"]);
    replica.expect_reply("+PONG\r\n");
    replica.send(&["REPLCONF", "listening-port", "6380"]);
    replica.expect_reply("+OK\r\n");
    replica.send(&["REPLCONF", "capa", "psync2"]);
    replica.expect_reply("+OK\r\n");

    // Nothing about the handshake so far closes the connection off.
    replica.send(&["SET", "key", "value"]);
    replica.expect_reply("+OK\r\n");
}

#[test]
fn starts_a_replica_on_the_history_it_is_keeping() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PSYNC", "?", "-1"]);
    let agreement = replica.read_line();

    let rest = agreement
        .strip_prefix("+FULLRESYNC ")
        .unwrap_or_else(|| panic!("not a full resync: {agreement:?}"));
    let (id, offset) = rest.split_once(' ').expect("an id and an offset");

    assert_eq!(id.len(), 40, "{id:?}");
    assert_eq!(offset, "0");
}

#[test]
fn starts_a_replica_on_the_same_history_it_reports_through_info() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INFO", "replication"]);
    let report = client.read_bulk_string();
    let reported = report
        .lines()
        .find_map(|line| line.strip_prefix("master_replid:"))
        .expect("a replication id");

    let mut replica = server.connect();
    replica.send(&["PSYNC", "?", "-1"]);
    let agreement = replica.read_line();

    assert!(
        agreement.contains(reported),
        "{agreement:?} vs {reported:?}"
    );
}

#[test]
fn starts_a_replica_afresh_however_much_history_it_claims() {
    let server = Server::start();
    let mut replica = server.connect();

    // Keeping no record of what it has handed out, this master can only ever
    // start a replica over, even one that says it was following already.
    replica.send(&["PSYNC", "8371b4fb1155b71f4a04d3e1bc3e18c4d990aeeb", "42"]);
    assert!(replica.read_line().starts_with("+FULLRESYNC "));
}

#[test]
fn rejects_a_psync_that_says_too_little() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PSYNC", "?"]);
    replica.expect_reply("-ERR wrong number of arguments for 'psync' command\r\n");
}

#[test]
fn hands_the_replica_the_dataset_to_start_from() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PSYNC", "?", "-1"]);
    assert!(replica.read_line().starts_with("+FULLRESYNC "));

    let dataset = replica.read_file();

    // A file Redis saved, rather than anything of our own making.
    assert!(dataset.starts_with(b"REDIS"), "{dataset:?}");
    // The marker for the end, and nothing but a checksum after it.
    assert_eq!(dataset[dataset.len() - 9], 0xff, "{dataset:?}");
}

#[test]
fn sends_the_dataset_without_the_crlf_a_bulk_string_ends_in() {
    let server = Server::start();
    let mut replica = server.connect();

    replica.send(&["PSYNC", "?", "-1"]);
    replica.read_line();
    replica.read_file();

    let mut client = server.connect();
    client.send(&["SET", "key", "value"]);
    client.expect_reply("+OK\r\n");

    // Were a CRLF written after the file, it would arrive here, ahead of the
    // command that really follows it.
    replica.expect_reply("*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n");
}
