mod common;

use common::Server;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A directory for a server to keep its data in, swept up when it goes out of
/// scope.
struct Data(PathBuf);

impl Data {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        let dir = std::env::temp_dir().join(format!(
            "redis-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));

        std::fs::create_dir_all(&dir).expect("failed to make a directory to keep data in");

        Self(dir)
    }

    fn dir(&self) -> &str {
        self.0.to_str().expect("a temporary directory is text")
    }

    fn holds(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A file inside the directory the writes are recorded in.
    fn records(&self, name: &str) -> PathBuf {
        self.0.join("appendonlydir").join(name)
    }

    /// Leaves a manifest and the empty file it names behind, as a server that
    /// had been running here would have.
    fn left_recording_in(&self, name: &str) {
        self.left_recorded(name, b"");
    }

    /// The same, for a record that already holds these bytes.
    fn left_recorded(&self, name: &str, recorded: &[u8]) {
        let dir = self.0.join("appendonlydir");
        std::fs::create_dir_all(&dir).expect("failed to make a directory to leave it in");

        std::fs::write(dir.join(name), recorded).expect("failed to leave a file behind");
        std::fs::write(
            dir.join("appendonly.aof.manifest"),
            format!("file {name} seq 1 type i\n"),
        )
        .expect("failed to leave a manifest behind");
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn makes_somewhere_to_record_its_writes_before_it_takes_a_client() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);

    // The server says it is listening before this test connects, so both are
    // already there by the time any command could be sent.
    let dir = data.holds("appendonlydir");

    assert!(dir.is_dir());
    assert!(dir.join("appendonly.aof.1.incr.aof").is_file());

    let mut client = server.connect();
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn leaves_the_file_it_makes_empty() {
    let data = Data::new();
    let _server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);

    // Nothing is recorded until there is something to record.
    let file = data
        .holds("appendonlydir")
        .join("appendonly.aof.1.incr.aof");

    assert_eq!(std::fs::read(&file).unwrap(), b"");
}

#[test]
fn writes_a_manifest_naming_the_file_it_made() {
    let data = Data::new();
    let _server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);

    let manifest = data.holds("appendonlydir").join("appendonly.aof.manifest");

    assert_eq!(
        std::fs::read_to_string(&manifest).unwrap(),
        "file appendonly.aof.1.incr.aof seq 1 type i\n"
    );
}

#[test]
fn records_under_the_names_it_was_given() {
    let data = Data::new();
    let _server = Server::start_with(&[
        "--dir",
        data.dir(),
        "--appendonly",
        "yes",
        "--appenddirname",
        "my_aof_dir",
        "--appendfilename",
        "my_writes.aof",
    ]);

    assert!(data.holds("my_aof_dir").is_dir());
    assert!(
        data.holds("my_aof_dir")
            .join("my_writes.aof.1.incr.aof")
            .is_file()
    );
    assert_eq!(
        std::fs::read_to_string(data.holds("my_aof_dir").join("my_writes.aof.manifest")).unwrap(),
        "file my_writes.aof.1.incr.aof seq 1 type i\n"
    );
    assert!(!data.holds("appendonlydir").exists());
}

#[test]
fn makes_nowhere_to_record_when_it_was_not_asked_to() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "no"]);

    assert!(!data.holds("appendonlydir").exists());

    let mut client = server.connect();
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn makes_nowhere_to_record_when_it_was_told_nothing() {
    let data = Data::new();
    let _server = Server::start_with(&["--dir", data.dir()]);

    assert!(!data.holds("appendonlydir").exists());
}

#[test]
fn starts_over_what_it_left_behind_last_time() {
    let data = Data::new();
    let args = ["--dir", data.dir(), "--appendonly", "yes"];
    let file = data
        .holds("appendonlydir")
        .join("appendonly.aof.1.incr.aof");

    let first = Server::start_with(&args);
    std::fs::write(&file, b"*1\r\n$4\r\nPING\r\n").expect("failed to record a command");
    drop(first);

    let second = Server::start_with(&args);
    let mut client = second.connect();

    // What is already there is what a restart finds, and the server takes it as
    // it stands rather than refusing or sweeping it away.
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");

    assert_eq!(std::fs::read(&file).unwrap(), b"*1\r\n$4\r\nPING\r\n");
}

#[test]
fn records_a_write_in_the_file_the_manifest_names() {
    let data = Data::new();

    // A manifest naming a file that is not the one the settings would have
    // picked, which is the whole reason to read the manifest at all.
    data.left_recording_in("elsewhere.aof.1.incr.aof");

    let server = Server::start_with(&[
        "--dir",
        data.dir(),
        "--appendonly",
        "yes",
        "--appendfsync",
        "always",
    ]);
    let mut client = server.connect();

    client.send(&["SET", "foo", "100"]);
    client.expect_reply("+OK\r\n");

    // Read the moment the reply arrives: the write is to be in the file by the
    // time the client hears that it took.
    assert_eq!(
        std::fs::read(data.records("elsewhere.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n100\r\n"
    );
    assert!(!data.records("appendonly.aof.1.incr.aof").exists());
}

#[test]
fn records_one_write_after_another_in_the_order_they_came() {
    let data = Data::new();
    data.left_recording_in("elsewhere.aof.1.incr.aof");

    let server = Server::start_with(&[
        "--dir",
        data.dir(),
        "--appendonly",
        "yes",
        "--appendfsync",
        "always",
    ]);
    let mut client = server.connect();

    client.send(&["SET", "foo", "100"]);
    client.expect_reply("+OK\r\n");
    client.send(&["SET", "bar", "200"]);
    client.expect_reply("+OK\r\n");

    // One command straight after the other, with nothing between them: the RESP
    // framing is what says where one ends and the next begins.
    assert_eq!(
        std::fs::read(data.records("elsewhere.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n100\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$3\r\n200\r\n"
    );
}

#[test]
fn records_the_writes_out_of_a_mix_and_leaves_the_rest() {
    let data = Data::new();
    data.left_recording_in("elsewhere.aof.1.incr.aof");

    let server = Server::start_with(&[
        "--dir",
        data.dir(),
        "--appendonly",
        "yes",
        "--appendfsync",
        "always",
    ]);
    let mut client = server.connect();

    // A command that only looks at the store leaves it as it found it, so
    // playing it back later would be so much wasted breath.
    for command in [
        ["SET", "foo", "1"].as_slice(),
        ["GET", "foo"].as_slice(),
        ["PING"].as_slice(),
        ["ECHO", "hello"].as_slice(),
        ["CONFIG", "GET", "appendonly"].as_slice(),
        ["KEYS", "*"].as_slice(),
        ["TYPE", "foo"].as_slice(),
        ["SET", "bar", "2"].as_slice(),
    ] {
        client.send(command);
        client.read_reply();
    }

    assert_eq!(
        std::fs::read(data.records("elsewhere.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n"
    );
}

#[test]
fn records_nothing_at_all_when_nothing_was_changed() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    for command in [
        ["GET", "nobody"].as_slice(),
        ["PING"].as_slice(),
        ["ECHO", "hello"].as_slice(),
        ["LRANGE", "l", "0", "-1"].as_slice(),
    ] {
        client.send(command);
        client.read_reply();
    }

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b""
    );
}

#[test]
fn records_every_kind_of_write_it_knows() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // Whatever leaves the store different from how it found it belongs in the
    // record, not `SET` alone.
    for command in [
        ["SET", "n", "1"].as_slice(),
        ["INCR", "n"].as_slice(),
        ["RPUSH", "l", "a"].as_slice(),
        ["LPOP", "l"].as_slice(),
    ] {
        client.send(command);
        client.read_reply();
    }

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$1\r\nn\r\n$1\r\n1\r\n\
          *2\r\n$4\r\nINCR\r\n$1\r\nn\r\n\
          *3\r\n$5\r\nRPUSH\r\n$1\r\nl\r\n$1\r\na\r\n\
          *2\r\n$4\r\nLPOP\r\n$1\r\nl\r\n"
    );
}

#[test]
fn records_the_writes_of_one_client_after_those_of_another() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut first = server.connect();
    let mut second = server.connect();

    // Each is answered before the next is sent, so the order they arrived in is
    // not in doubt, and it is the order the record is to keep them in.
    first.send(&["SET", "foo", "1"]);
    first.expect_reply("+OK\r\n");
    second.send(&["SET", "bar", "2"]);
    second.expect_reply("+OK\r\n");
    first.send(&["SET", "baz", "3"]);
    first.expect_reply("+OK\r\n");

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbaz\r\n$1\r\n3\r\n"
    );
}

#[test]
fn records_the_writes_a_transaction_held_back_when_it_lets_them_go() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["MULTI"]);
    client.expect_reply("+OK\r\n");
    client.send(&["SET", "foo", "1"]);
    client.expect_reply("+QUEUED\r\n");

    // Nothing has happened to the store yet, so there is nothing to record yet.
    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b""
    );

    client.send(&["SET", "bar", "2"]);
    client.expect_reply("+QUEUED\r\n");
    client.send(&["EXEC"]);
    client.read_reply();

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n"
    );
}

#[test]
fn records_a_write_the_client_asked_for_word_for_word() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // Whatever the client sent is what goes down, spelling and all: it is to be
    // played back as a command, not summarised.
    client.send(&["set", "foo", "bar", "px", "100000"]);
    client.expect_reply("+OK\r\n");

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b"*5\r\n$3\r\nset\r\n$3\r\nfoo\r\n$3\r\nbar\r\n$2\r\npx\r\n$6\r\n100000\r\n"
    );
}

#[test]
fn records_nothing_for_a_write_that_was_refused() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // A command that failed changed nothing, so there is nothing to play back.
    client.send(&["SET", "foo"]);
    client.read_reply();
    client.send(&["INCR"]);
    client.read_reply();

    assert_eq!(
        std::fs::read(data.records("appendonly.aof.1.incr.aof")).unwrap(),
        b""
    );
}

#[test]
fn records_nothing_when_it_was_not_asked_to_record() {
    let data = Data::new();
    let server = Server::start_with(&["--dir", data.dir()]);
    let mut client = server.connect();

    client.send(&["SET", "foo", "100"]);
    client.expect_reply("+OK\r\n");
    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\n100\r\n");

    assert!(!data.holds("appendonlydir").exists());
}

#[test]
fn comes_up_holding_what_the_recorded_command_put_there() {
    let data = Data::new();

    // The record names a file the settings would never have picked, which is
    // the whole reason to read the manifest before replaying anything.
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n123\r\n",
    );

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\n123\r\n");
}

#[test]
fn comes_up_holding_what_every_recorded_command_put_there() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$4\r\nkey1\r\n$6\r\nvalue1\r\n\
          *3\r\n$3\r\nSET\r\n$4\r\nkey2\r\n$6\r\nvalue2\r\n\
          *3\r\n$3\r\nSET\r\n$4\r\nkey3\r\n$6\r\nvalue3\r\n",
    );

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // Every command in the file, not the first alone.
    for (key, value) in [("key1", "value1"), ("key2", "value2"), ("key3", "value3")] {
        client.send(&["GET", key]);
        client.expect_reply(&format!("$6\r\n{value}\r\n"));
    }
}

#[test]
fn comes_up_where_the_recorded_commands_left_off_between_them() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$1\r\nn\r\n$1\r\n5\r\n\
          *2\r\n$4\r\nINCR\r\n$1\r\nn\r\n\
          *2\r\n$4\r\nINCR\r\n$1\r\nn\r\n\
          *4\r\n$5\r\nRPUSH\r\n$1\r\nl\r\n$1\r\na\r\n$1\r\nb\r\n\
          *2\r\n$4\r\nLPOP\r\n$1\r\nl\r\n",
    );

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // Each command is done against what the ones before it left, so what comes
    // up is the sum of them rather than any one of them.
    client.send(&["GET", "n"]);
    client.expect_reply("$1\r\n7\r\n");

    client.send(&["LRANGE", "l", "0", "-1"]);
    client.expect_reply("*1\r\n$1\r\nb\r\n");
}

#[test]
fn comes_up_holding_the_last_word_on_a_key_recorded_twice() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$5\r\nfirst\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$4\r\nlast\r\n",
    );

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    // Played back in the order recorded, so the later write is the one standing.
    client.send(&["GET", "foo"]);
    client.expect_reply("$4\r\nlast\r\n");
}

#[test]
fn leaves_off_the_command_a_record_stops_in_the_middle_of() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nba",
    );

    // A server stopped while writing a command down leaves part of it behind.
    // What arrived of it was never answered, so it is dropped and the rest of
    // the record still stands.
    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["GET", "foo"]);
    client.expect_reply("$1\r\n1\r\n");
    client.send(&["KEYS", "*"]);
    client.expect_reply("*1\r\n$3\r\nfoo\r\n");
}

#[test]
fn does_not_record_again_what_it_has_just_played_back() {
    let data = Data::new();
    let recorded = b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n123\r\n";
    data.left_recorded("elsewhere.aof.1.incr.aof", recorded);

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\n123\r\n");

    // A command read out of the record and done again is not a new write. One
    // written back would double with every restart.
    assert_eq!(
        std::fs::read(data.records("elsewhere.aof.1.incr.aof")).unwrap(),
        recorded
    );
}

#[test]
fn goes_on_recording_after_what_it_played_back() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n",
    );

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["SET", "bar", "2"]);
    client.expect_reply("+OK\r\n");

    assert_eq!(
        std::fs::read(data.records("elsewhere.aof.1.incr.aof")).unwrap(),
        b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
          *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n"
    );
}

#[test]
fn comes_up_where_the_last_run_left_off() {
    let data = Data::new();
    let args = ["--dir", data.dir(), "--appendonly", "yes"];

    let first = Server::start_with(&args);
    let mut client = first.connect();
    client.send(&["SET", "foo", "123"]);
    client.expect_reply("+OK\r\n");
    drop(first);

    // What one run wrote down is what the next comes up holding, which is what
    // the record is for.
    let second = Server::start_with(&args);
    let mut client = second.connect();

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\n123\r\n");
}

#[test]
fn comes_up_holding_nothing_over_a_record_with_nothing_in_it() {
    let data = Data::new();
    data.left_recording_in("elsewhere.aof.1.incr.aof");

    let server = Server::start_with(&["--dir", data.dir(), "--appendonly", "yes"]);
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn will_not_start_over_a_record_it_cannot_read_through() {
    let data = Data::new();
    data.left_recorded(
        "elsewhere.aof.1.incr.aof",
        b"*3\r\n$3\r\nSET\r\nnonsense\r\n",
    );

    // Coming up over a record it could not read would be coming up short of
    // where the last run left off, and saying nothing about it.
    let args = ["--port", "0", "--dir", data.dir(), "--appendonly", "yes"];
    let output =
        common::gives_up(&args, Duration::from_secs(10)).expect("the server came up and stayed up");

    assert!(!output.status.success(), "the server started anyway");
}

#[test]
fn will_not_start_with_nowhere_to_record() {
    let data = Data::new();
    std::fs::write(data.holds("appendonlydir"), b"not a directory")
        .expect("failed to put a file in the way");

    // Coming up as though all were well would leave every write afterwards
    // with nowhere to go, and say nothing about it until the data was wanted.
    let args = ["--port", "0", "--dir", data.dir(), "--appendonly", "yes"];
    let output =
        common::gives_up(&args, Duration::from_secs(10)).expect("the server came up and stayed up");

    assert!(!output.status.success(), "the server started anyway");

    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("appendonlydir"), "{said:?}");
}
