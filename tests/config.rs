mod common;

use common::Server;

/// A server started the way the RDB stages start it, told where to keep its
/// dataset and under what name.
fn with_a_dataset() -> Server {
    Server::start_with(&["--dir", "/tmp/redis-files", "--dbfilename", "dump.rdb"])
}

#[test]
fn reports_the_directory_it_was_given() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "dir"]);
    client.expect_reply("*2\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n");
}

#[test]
fn reports_the_filename_it_was_given() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "dbfilename"]);
    client.expect_reply("*2\r\n$10\r\ndbfilename\r\n$8\r\ndump.rdb\r\n");
}

#[test]
fn reports_where_redis_keeps_its_dataset_when_told_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    // The server is started from wherever these tests run, and answers with
    // that directory named in full rather than as a `.`.
    let here = std::env::current_dir().unwrap();
    let here = here.to_string_lossy();

    client.send(&["CONFIG", "GET", "dir"]);
    client.expect_reply(&format!("*2\r\n$3\r\ndir\r\n${}\r\n{here}\r\n", here.len()));

    client.send(&["CONFIG", "GET", "dbfilename"]);
    client.expect_reply("*2\r\n$10\r\ndbfilename\r\n$8\r\ndump.rdb\r\n");
}

#[test]
fn reports_what_it_would_write_down_as_it_goes_when_told_nothing() {
    let server = Server::start();
    let mut client = server.connect();

    // Nothing is written down yet; these say only what would be, and where.
    client.send(&["CONFIG", "GET", "appendonly"]);
    client.expect_reply("*2\r\n$10\r\nappendonly\r\n$2\r\nno\r\n");

    client.send(&["CONFIG", "GET", "appenddirname"]);
    client.expect_reply("*2\r\n$13\r\nappenddirname\r\n$13\r\nappendonlydir\r\n");

    client.send(&["CONFIG", "GET", "appendfilename"]);
    client.expect_reply("*2\r\n$14\r\nappendfilename\r\n$14\r\nappendonly.aof\r\n");

    client.send(&["CONFIG", "GET", "appendfsync"]);
    client.expect_reply("*2\r\n$11\r\nappendfsync\r\n$8\r\neverysec\r\n");
}

#[test]
fn reports_the_append_only_settings_alongside_the_dataset_ones() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "dbfilename", "appendfilename"]);
    client.expect_reply(
        "*4\r\n$10\r\ndbfilename\r\n$8\r\ndump.rdb\r\n$14\r\nappendfilename\r\n$14\r\nappendonly.aof\r\n",
    );
}

#[test]
fn accepts_any_casing_of_an_append_only_setting() {
    let server = Server::start();
    let mut client = server.connect();

    for name in ["appendfsync", "APPENDFSYNC", "AppendFsync"] {
        client.send(&["CONFIG", "GET", name]);
        client.expect_reply("*2\r\n$11\r\nappendfsync\r\n$8\r\neverysec\r\n");
    }
}

#[test]
fn accepts_any_casing_of_the_command_and_the_setting() {
    let server = with_a_dataset();
    let mut client = server.connect();

    for command in [
        ["CONFIG", "GET", "dir"],
        ["config", "get", "DIR"],
        ["Config", "Get", "Dir"],
    ] {
        client.send(&command);
        // The name goes back as Redis spells it, however it was asked for.
        client.expect_reply("*2\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n");
    }
}

#[test]
fn reports_several_settings_at_once() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "dir", "dbfilename"]);
    client.expect_reply(
        "*4\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n$10\r\ndbfilename\r\n$8\r\ndump.rdb\r\n",
    );
}

#[test]
fn reports_nothing_for_a_setting_it_does_not_have() {
    let server = with_a_dataset();
    let mut client = server.connect();

    // Redis answers rather than refuses, since a setting it lacks is not a
    // mistake on the client's part.
    client.send(&["CONFIG", "GET", "maxmemory"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn leaves_out_a_setting_it_does_not_have_and_keeps_the_rest() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "maxmemory", "dir"]);
    client.expect_reply("*2\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n");
}

#[test]
fn refuses_a_get_that_names_no_setting() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET"]);
    client.expect_reply("-ERR wrong number of arguments for 'config|get' command\r\n");
}

#[test]
fn refuses_a_config_that_says_nothing() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG"]);
    client.expect_reply("-ERR wrong number of arguments for 'config' command\r\n");
}

#[test]
fn refuses_to_change_a_setting() {
    let server = with_a_dataset();
    let mut client = server.connect();

    // Only reading settings is supported, and a client is told so rather than
    // left to believe the change took.
    client.send(&["CONFIG", "SET", "dir", "/tmp"]);
    let reply = client.read_line();

    assert!(
        reply.starts_with("-ERR Unknown CONFIG subcommand"),
        "{reply:?}"
    );

    client.send(&["CONFIG", "GET", "dir"]);
    client.expect_reply("*2\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n");
}

#[test]
fn keeps_serving_the_connection_after_a_config() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["CONFIG", "GET", "dir"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn queues_a_config_inside_a_transaction() {
    let server = with_a_dataset();
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");
    client.send(&["CONFIG", "GET", "dir"]);
    client.expect_reply("+QUEUED\r\n");

    client.send(&["EXEC"]);
    client.expect_reply("*1\r\n*2\r\n$3\r\ndir\r\n$16\r\n/tmp/redis-files\r\n");
}
