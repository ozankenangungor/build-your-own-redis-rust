mod common;

use common::{Client, Server};
use std::thread::sleep;
use std::time::Duration;

/// Long enough for the server to have registered a blocking read before the
/// next command is sent, so the tests exercise the waiting path.
const SETTLE: Duration = Duration::from_millis(100);

const UNBALANCED: &str = "-ERR Unbalanced XREAD list of streams: \
     for each stream key an ID or '$' must be specified.\r\n";

/// Fills `stream_key` with the three entries from the stage description.
fn three_entries(server: &Server) -> Client {
    let mut client = server.connect();

    for (id, field, value) in [
        ("0-1", "foo", "bar"),
        ("0-2", "bar", "baz"),
        ("0-3", "baz", "foo"),
    ] {
        client.send(&["XADD", "stream_key", id, field, value]);
        client.expect_reply(&format!("$3\r\n{id}\r\n"));
    }

    client
}

#[test]
fn reads_the_entries_recorded_after_an_id() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-1", "temperature", "96"]);
    client.expect_reply("$3\r\n0-1\r\n");

    client.send(&["XREAD", "STREAMS", "stream_key", "0-0"]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$11\r\ntemperature\r\n$2\r\n96\r\n",
    ));
}

#[test]
fn leaves_out_the_entry_carrying_the_given_id() {
    let server = Server::start();
    let mut client = three_entries(&server);

    // Unlike XRANGE, the entry named by the id is not part of the reply.
    client.send(&["XREAD", "STREAMS", "stream_key", "0-2"]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn replies_with_nothing_when_no_entry_is_newer() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "STREAMS", "stream_key", "0-3"]);
    client.expect_reply("*-1\r\n");

    client.send(&["XREAD", "STREAMS", "missing_stream", "0-0"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn accepts_any_casing_of_the_streams_keyword() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "streams", "stream_key", "0-2"]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn rejects_a_read_without_the_streams_keyword() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "COUNT", "stream_key", "0-0"]);
    client.expect_reply("-ERR syntax error\r\n");

    client.send(&["XREAD"]);
    client.expect_reply("-ERR wrong number of arguments for 'xread' command\r\n");
}

#[test]
fn rejects_a_read_whose_keys_and_ids_do_not_pair_up() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "STREAMS", "stream_key"]);
    client.expect_reply(UNBALANCED);

    client.send(&["XREAD", "STREAMS", "stream_key", "other_key", "0-0"]);
    client.expect_reply(UNBALANCED);

    client.send(&["XREAD", "STREAMS"]);
    client.expect_reply(UNBALANCED);
}

#[test]
fn reads_several_streams_in_the_order_they_were_asked_for() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "stream_key", "0-1", "temperature", "95"]);
    client.expect_reply("$3\r\n0-1\r\n");
    client.send(&["XADD", "other_stream_key", "0-2", "humidity", "97"]);
    client.expect_reply("$3\r\n0-2\r\n");

    client.send(&[
        "XREAD",
        "STREAMS",
        "stream_key",
        "other_stream_key",
        "0-0",
        "0-1",
    ]);
    client.expect_reply(concat!(
        "*2\r\n",
        "*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$11\r\ntemperature\r\n$2\r\n95\r\n",
        "*2\r\n$16\r\nother_stream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-2\r\n*2\r\n$8\r\nhumidity\r\n$2\r\n97\r\n",
    ));
}

#[test]
fn leaves_out_the_streams_with_nothing_new() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["XADD", "quiet_stream", "0-1", "a", "1"]);
    client.expect_reply("$3\r\n0-1\r\n");
    client.send(&["XADD", "busy_stream", "0-1", "b", "2"]);
    client.expect_reply("$3\r\n0-1\r\n");

    // Only the second stream has anything past the ids we ask from.
    client.send(&[
        "XREAD",
        "STREAMS",
        "quiet_stream",
        "busy_stream",
        "0-1",
        "0-0",
    ]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$11\r\nbusy_stream\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$1\r\nb\r\n$1\r\n2\r\n",
    ));
}

#[test]
fn rejects_a_stream_read_over_a_list() {
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["RPUSH", "list_key", "element"]);
    client.expect_reply(":1\r\n");

    client.send(&["XREAD", "STREAMS", "list_key", "0-0"]);
    client.expect_reply("-WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
}

#[test]
fn a_blocking_read_returns_what_is_already_there() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "BLOCK", "1000", "STREAMS", "stream_key", "0-2"]);
    client.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-3\r\n*2\r\n$3\r\nbaz\r\n$3\r\nfoo\r\n",
    ));
}

#[test]
fn a_blocking_read_waits_for_an_entry_to_arrive() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut writer = server.connect();

    writer.send(&["XADD", "stream_key", "0-1", "temperature", "96"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    blocked.send(&["XREAD", "BLOCK", "1000", "STREAMS", "stream_key", "0-1"]);
    sleep(SETTLE);

    writer.send(&["XADD", "stream_key", "0-2", "temperature", "95"]);
    writer.expect_reply("$3\r\n0-2\r\n");

    blocked.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-2\r\n*2\r\n$11\r\ntemperature\r\n$2\r\n95\r\n",
    ));
}

#[test]
fn a_zero_timeout_waits_for_as_long_as_it_takes() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut writer = server.connect();

    writer.send(&["XADD", "stream_key", "0-1", "temperature", "96"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    blocked.send(&["XREAD", "BLOCK", "0", "STREAMS", "stream_key", "0-1"]);

    // Well past any timeout a non-zero BLOCK would have used here.
    sleep(Duration::from_millis(500));

    writer.send(&["XADD", "stream_key", "0-2", "temperature", "95"]);
    writer.expect_reply("$3\r\n0-2\r\n");

    blocked.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-2\r\n*2\r\n$11\r\ntemperature\r\n$2\r\n95\r\n",
    ));
}

#[test]
fn a_dollar_reads_only_what_arrives_after_the_command() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut writer = server.connect();

    writer.send(&["XADD", "stream_key", "0-1", "temperature", "96"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    blocked.send(&["XREAD", "BLOCK", "0", "STREAMS", "stream_key", "$"]);
    sleep(SETTLE);

    writer.send(&["XADD", "stream_key", "0-2", "temperature", "95"]);
    writer.expect_reply("$3\r\n0-2\r\n");

    // The entry that was already there is not part of the reply.
    blocked.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-2\r\n*2\r\n$11\r\ntemperature\r\n$2\r\n95\r\n",
    ));
}

#[test]
fn a_dollar_times_out_when_nothing_new_arrives() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "BLOCK", "100", "STREAMS", "stream_key", "$"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn a_dollar_on_a_stream_that_does_not_exist_yet_waits_for_its_first_entry() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut writer = server.connect();

    blocked.send(&["XREAD", "BLOCK", "0", "STREAMS", "new_stream", "$"]);
    sleep(SETTLE);

    writer.send(&["XADD", "new_stream", "0-1", "a", "1"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    blocked.expect_reply(concat!(
        "*1\r\n*2\r\n$10\r\nnew_stream\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$1\r\na\r\n$1\r\n1\r\n",
    ));
}

#[test]
fn a_blocking_read_gives_up_once_the_timeout_passes() {
    let server = Server::start();
    let mut client = three_entries(&server);

    client.send(&["XREAD", "BLOCK", "100", "STREAMS", "stream_key", "0-3"]);
    client.expect_reply("*-1\r\n");
}

#[test]
fn one_entry_wakes_every_blocked_reader() {
    let server = Server::start();
    let mut first = server.connect();
    let mut second = server.connect();
    let mut writer = server.connect();

    // Reading an entry does not consume it, so both clients get to see it.
    first.send(&["XREAD", "BLOCK", "1000", "STREAMS", "stream_key", "0-0"]);
    second.send(&["XREAD", "BLOCK", "1000", "STREAMS", "stream_key", "0-0"]);
    sleep(SETTLE);

    writer.send(&["XADD", "stream_key", "0-1", "a", "1"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    let expected = concat!(
        "*1\r\n*2\r\n$10\r\nstream_key\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$1\r\na\r\n$1\r\n1\r\n",
    );
    first.expect_reply(expected);
    second.expect_reply(expected);
}

#[test]
fn a_blocking_read_wakes_on_whichever_stream_grows() {
    let server = Server::start();
    let mut blocked = server.connect();
    let mut writer = server.connect();

    blocked.send(&[
        "XREAD",
        "BLOCK",
        "1000",
        "STREAMS",
        "first_stream",
        "second_stream",
        "0-0",
        "0-0",
    ]);
    sleep(SETTLE);

    writer.send(&["XADD", "second_stream", "0-1", "b", "2"]);
    writer.expect_reply("$3\r\n0-1\r\n");

    blocked.expect_reply(concat!(
        "*1\r\n*2\r\n$13\r\nsecond_stream\r\n",
        "*1\r\n*2\r\n$3\r\n0-1\r\n*2\r\n$1\r\nb\r\n$1\r\n2\r\n",
    ));
}
