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
    client.expect_reply("*2\r\n$4\r\npong\r\n$0\r\n\r\n");
}

#[test]
fn answers_a_ping_differently_depending_on_who_is_asking() {
    let server = Server::start();
    let mut listening = server.connect();
    let mut asking = server.connect();

    // A listening client hears back in the shape everything else on its
    // connection arrives in, so one reader can make sense of them all.
    listening.send(&["SUBSCRIBE", "foo"]);
    listening.read_reply();
    listening.send(&["PING"]);
    listening.expect_reply("*2\r\n$4\r\npong\r\n$0\r\n\r\n");

    // A client that is not listening is answered as it always was.
    asking.send(&["PING"]);
    asking.expect_reply("+PONG\r\n");
}

#[test]
fn answers_a_ping_the_old_way_until_the_client_starts_listening() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");

    client.send(&["SUBSCRIBE", "foo"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("*2\r\n$4\r\npong\r\n$0\r\n\r\n");
}

#[test]
fn tells_a_publisher_how_many_are_listening_on_the_channel() {
    let server = Server::start();
    let mut first = server.connect();
    let mut second = server.connect();
    let mut third = server.connect();
    let mut publisher = server.connect();

    first.send(&["SUBSCRIBE", "foo"]);
    first.read_reply();
    second.send(&["SUBSCRIBE", "bar"]);
    second.read_reply();
    third.send(&["SUBSCRIBE", "bar"]);
    third.read_reply();

    publisher.send(&["PUBLISH", "bar", "msg"]);
    publisher.expect_reply(":2\r\n");
    publisher.send(&["PUBLISH", "foo", "msg"]);
    publisher.expect_reply(":1\r\n");
}

#[test]
fn tells_a_publisher_of_nobody_on_a_channel_nobody_is_on() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["PUBLISH", "nobody", "msg"]);
    client.expect_reply(":0\r\n");
}

#[test]
fn counts_one_client_once_on_each_of_its_channels() {
    let server = Server::start();
    let mut client = server.connect();
    let mut publisher = server.connect();

    // One client on several channels is one listener on each of them.
    client.send(&["SUBSCRIBE", "foo", "bar"]);
    client.read_reply();
    client.read_reply();

    publisher.send(&["PUBLISH", "foo", "msg"]);
    publisher.expect_reply(":1\r\n");
    publisher.send(&["PUBLISH", "bar", "msg"]);
    publisher.expect_reply(":1\r\n");
}

#[test]
fn stops_counting_a_client_that_has_hung_up() {
    let server = Server::start();
    let mut publisher = server.connect();

    {
        let mut leaving = server.connect();
        leaving.send(&["SUBSCRIBE", "foo"]);
        leaving.read_reply();

        publisher.send(&["PUBLISH", "foo", "msg"]);
        publisher.expect_reply(":1\r\n");
    }

    // A client that has gone is listening to nothing, and a publisher told
    // otherwise would be told its message reached further than it did.
    publisher.expect_reply_eventually(&["PUBLISH", "foo", "msg"], ":0\r\n");
}

#[test]
fn will_not_publish_for_a_client_that_is_listening() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.read_reply();

    // Redis leaves publishing out of what a listening client may do.
    client.send(&["PUBLISH", "foo", "msg"]);
    let said = client.read_line();

    assert!(
        said.starts_with("-ERR Can't execute 'publish': "),
        "{said:?}"
    );
}

#[test]
fn refuses_a_publish_that_is_missing_a_channel_or_a_message() {
    let server = Server::start();
    let mut client = server.connect();

    for command in [["PUBLISH"].as_slice(), ["PUBLISH", "foo"].as_slice()] {
        client.send(command);
        client.expect_reply("-ERR wrong number of arguments for 'publish' command\r\n");
    }
}

#[test]
fn will_not_run_a_command_for_a_client_that_is_listening() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");

    for (command, refused) in [
        (["SET", "key", "value"].as_slice(), "set"),
        (["GET", "key"].as_slice(), "get"),
        (["ECHO", "hey"].as_slice(), "echo"),
    ] {
        client.send(command);
        let said = client.read_line();

        assert!(
            said.starts_with(&format!("-ERR Can't execute '{refused}': ")),
            "{said:?}"
        );
    }
}

#[test]
fn goes_on_taking_channels_from_a_client_that_is_listening() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nfoo\r\n:1\r\n");

    // Listening is not the end of the conversation, only a turn in it: the
    // commands that steer it answer as they always did.
    client.send(&["SUBSCRIBE", "bar"]);
    client.expect_reply("*3\r\n$9\r\nsubscribe\r\n$3\r\nbar\r\n:2\r\n");
    client.send(&["PING"]);
    client.expect_reply("*2\r\n$4\r\npong\r\n$0\r\n\r\n");
}

#[test]
fn goes_on_serving_the_clients_that_are_not_listening() {
    let server = Server::start();
    let mut listening = server.connect();
    let mut asking = server.connect();

    listening.send(&["SUBSCRIBE", "foo"]);
    listening.read_reply();

    // One client listening says nothing about what another may ask for.
    asking.send(&["SET", "key", "value"]);
    asking.expect_reply("+OK\r\n");
    asking.send(&["GET", "key"]);
    asking.expect_reply("$5\r\nvalue\r\n");
}

#[test]
fn will_not_open_a_transaction_for_a_client_that_is_listening() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.read_reply();

    // Nothing is queued either, since queuing a command is running it later.
    client.send(&["MULTI"]);
    let said = client.read_line();

    assert!(said.starts_with("-ERR Can't execute 'multi': "), "{said:?}");
}

#[test]
fn will_not_run_a_command_it_does_not_know_for_a_listening_client() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["SUBSCRIBE", "foo"]);
    client.read_reply();

    client.send(&["NONSENSE"]);
    let said = client.read_line();

    assert!(
        said.starts_with("-ERR Can't execute 'nonsense': "),
        "{said:?}"
    );
}
