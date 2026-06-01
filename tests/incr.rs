mod common;

use common::Server;

#[test]
fn adds_one_to_a_number() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "41"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply(":42\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply(":43\r\n");
}

#[test]
fn leaves_the_new_number_behind_for_a_later_get() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "counter", "9"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "counter"]);
    client.expect_reply(":10\r\n");

    client.send(&["GET", "counter"]);
    client.expect_reply("$2\r\n10\r\n");
}

#[test]
fn starts_a_missing_key_at_one() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INCR", "foo"]);
    client.expect_reply(":1\r\n");

    client.send(&["INCR", "bar"]);
    client.expect_reply(":1\r\n");

    // The key is left behind as a string, the way `SET` would have left it.
    client.send(&["GET", "foo"]);
    client.expect_reply("$1\r\n1\r\n");

    client.send(&["TYPE", "foo"]);
    client.expect_reply("+string\r\n");
}

#[test]
fn starts_over_from_one_after_the_key_expires() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "41", "PX", "100"]);
    client.expect_reply("+OK\r\n");

    std::thread::sleep(std::time::Duration::from_millis(200));

    client.send(&["INCR", "foo"]);
    client.expect_reply(":1\r\n");
}

#[test]
fn counts_up_from_a_negative_number() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "-1"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn keeps_the_expiry_the_key_already_had() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "1", "PX", "100"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply(":2\r\n");

    std::thread::sleep(std::time::Duration::from_millis(200));

    client.send(&["GET", "foo"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn refuses_to_count_past_the_largest_number() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SET", "foo", "9223372036854775807"]);
    client.expect_reply("+OK\r\n");

    client.send(&["INCR", "foo"]);
    client.expect_reply("-ERR increment or decrement would overflow\r\n");
}

#[test]
fn rejects_an_incr_of_a_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["INCR", "list_key"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn rejects_an_incr_without_exactly_one_key() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["INCR"]);
    client.expect_reply("-ERR wrong number of arguments for 'incr' command\r\n");

    client.send(&["INCR", "foo", "bar"]);
    client.expect_reply("-ERR wrong number of arguments for 'incr' command\r\n");
}
