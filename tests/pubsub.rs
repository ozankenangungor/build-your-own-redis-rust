mod common;

use common::Server;

#[test]
fn confirms_the_channel_a_client_asks_to_listen_on() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
}

#[test]
fn counts_the_channels_one_client_has_asked_for() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
    client.send(&["SUBSCRIBE", "bar"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n");
}

#[test]
fn counts_the_channels_of_one_client_and_not_another() {
    let server = Server::start();
    let mut mine = server.connect();
    let mut yours = server.connect();

    mine.send(&["SUBSCRIBE", "foo"]);
    mine.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
    mine.send(&["SUBSCRIBE", "bar"]);
    mine.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n");

    // What one client listens on is no business of the next.
    yours.send(&["SUBSCRIBE", "baz"]);
    yours.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbaz\r\n:1\r\n");
}

#[test]
fn leaves_the_count_where_it_was_on_a_channel_it_is_already_on() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
}

#[test]
fn counts_the_channels_of_each_of_several_clients_apart() {
    let server = Server::start();

    // Each client is asked the same and answered the same, whatever the ones
    // before it were told: the count belongs to the client, not the server.
    for _ in 0..3 {
        let mut client = server.connect();

        client.send(&["SUBSCRIBE", "foo"]);
        client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
        client.send(&["SUBSCRIBE", "bar"]);
        client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n");
        client.send(&["SUBSCRIBE", "bar"]);
        client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n");
        client.send(&["SUBSCRIBE", "baz"]);
        client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbaz\r\n:3\r\n");
    }
}

#[test]
fn counts_a_channel_it_is_already_on_without_losing_its_place() {
    let server = Server::start();
    let mut client = server.connect();

    // Asking again for a channel it already has changes nothing, and the next
    // new one carries on from where the count stood.
    for (channel, count) in [
        ("foo", 1),
        ("bar", 2),
        ("foo", 2),
        ("bar", 2),
        ("baz", 3),
        ("baz", 3),
    ] {
        client.send(&["SUBSCRIBE", channel]);
        client.expect_reply(&format!(
            "*3\r\n$9\r\nsubscribe\r\n${}\r\n{channel}\r\n:{count}\r\n",
            channel.len()
        ));
    }
}

#[test]
fn accepts_any_casing_of_the_command() {
    let server = Server::start();

    for spelling in ["SUBSCRIBE", "subscribe", "Subscribe"] {
        let mut client = server.connect();

        client.send(&[spelling, "foo"]);
        client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");
    }
}

#[test]
fn takes_a_channel_named_in_bytes_that_are_not_text() {
    let server = Server::start();
    let mut client = server.connect();

    // A channel is named in bytes, as a key is, and may hold anything at all.
    client.send_raw(b"*2\r\n$9\r\nSUBSCRIBE\r\n$4\r\n\xff\x00\r\n\r\n");
    client.expect_bytes(b"*3\r\n$9\r\nsubscribe\r\n$4\r\n\xff\x00\r\n\r\n:1\r\n");
}

#[test]
fn confirms_each_of_the_channels_named_in_one_go() {
    let server = Server::start();
    let mut client = server.connect();

    // Redis takes as many channels as it is given and confirms them one by one.
    client.send(&["SUBSCRIBE", "foo", "bar"]);
    client.expect_reply(
        "*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n\
         *3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n",
    );
}

#[test]
fn refuses_a_subscribe_that_names_no_channel() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE"]);
    client.expect_reply("-ERR wrong number of arguments for 'subscribe' command\r\n");
}

#[test]
fn keeps_serving_the_connection_after_a_subscribe() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}
