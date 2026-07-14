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

    // The server says it is listening before this test connects, so the
    // directory is already there by the time any command could be sent.
    assert!(data.holds("appendonlydir").is_dir());

    let mut client = server.connect();
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn records_under_the_name_it_was_given() {
    let data = Data::new();
    let _server = Server::start_with(&[
        "--dir",
        data.dir(),
        "--appendonly",
        "yes",
        "--appenddirname",
        "my_aof_dir",
    ]);

    assert!(data.holds("my_aof_dir").is_dir());
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
fn starts_over_a_directory_it_left_behind_last_time() {
    let data = Data::new();
    let args = ["--dir", data.dir(), "--appendonly", "yes"];

    let first = Server::start_with(&args);
    std::fs::write(data.holds("appendonlydir").join("kept"), b"kept")
        .expect("failed to leave something behind");
    drop(first);

    let second = Server::start_with(&args);
    let mut client = second.connect();

    // A directory that is already there is what a restart finds, and the server
    // takes it as it stands rather than refusing or sweeping it away.
    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");

    assert!(data.holds("appendonlydir").join("kept").exists());
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
