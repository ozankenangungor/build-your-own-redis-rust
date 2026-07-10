mod common;

use common::Server;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A directory holding a saved dataset, swept up when it goes out of scope.
struct Saved(PathBuf);

impl Saved {
    fn new(file: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        let dir = std::env::temp_dir().join(format!(
            "redis-dataset-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));

        std::fs::create_dir_all(&dir).expect("failed to make a directory to save in");
        std::fs::write(dir.join("dump.rdb"), file).expect("failed to save a dataset");

        Self(dir)
    }

    /// A directory with nothing saved in it, for a server that has yet to save.
    fn nothing() -> Self {
        let saved = Self::new(b"");
        std::fs::remove_file(saved.0.join("dump.rdb")).expect("failed to unsave a dataset");

        saved
    }

    fn args(&self) -> [&str; 4] {
        ["--dir", self.dir(), "--dbfilename", "dump.rdb"]
    }

    fn dir(&self) -> &str {
        self.0.to_str().expect("a temporary directory is text")
    }

    fn server(&self) -> Server {
        Server::start_with(&self.args())
    }
}

impl Drop for Saved {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Lays out a dataset holding these keys, the way Redis saves one.
fn dataset(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut file = Vec::from(*b"REDIS0011");

    // Written by a server that named itself, as Redis does.
    file.extend_from_slice(&[0xFA, 9]);
    file.extend_from_slice(b"redis-ver");
    file.extend_from_slice(&[5]);
    file.extend_from_slice(b"7.2.0");

    // The first database, and how much room its keys want.
    file.extend_from_slice(&[0xFE, 0]);
    file.extend_from_slice(&[0xFB, entries.len() as u8, 0]);

    for (key, value) in entries {
        file.push(0x00);
        for part in [key, value] {
            file.push(part.len() as u8);
            file.extend_from_slice(part.as_bytes());
        }
    }

    file.push(0xFF);
    file.extend_from_slice(&[0; 8]);

    file
}

/// The same, for a key saved with a time on it.
fn dataset_expiring_at(key: &str, value: &str, at: SystemTime) -> Vec<u8> {
    let milliseconds = at
        .duration_since(UNIX_EPOCH)
        .expect("a time after the epoch")
        .as_millis() as u64;

    let mut file = Vec::from(*b"REDIS0011");

    file.extend_from_slice(&[0xFE, 0, 0xFC]);
    file.extend_from_slice(&milliseconds.to_le_bytes());
    file.push(0x00);
    for part in [key, value] {
        file.push(part.len() as u8);
        file.extend_from_slice(part.as_bytes());
    }
    file.push(0xFF);
    file.extend_from_slice(&[0; 8]);

    file
}

#[test]
fn lists_the_one_key_a_saved_dataset_holds() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*1\r\n$3\r\nfoo\r\n");
}

#[test]
fn lists_nothing_when_nothing_was_ever_saved() {
    let saved = Saved::nothing();
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn starts_up_without_a_dataset_to_read() {
    // Nothing points this server at a file at all, which is how every test
    // before this stage started it.
    let server = Server::start();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn holds_what_the_saved_key_held() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["GET", "foo"]);
    client.expect_reply("$3\r\nbar\r\n");
}

#[test]
fn lists_the_keys_it_was_given_alongside_the_ones_it_read() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["SET", "baz", "qux"]);
    client.expect_reply("+OK\r\n");

    client.send(&["KEYS", "*"]);
    let mut keys = client.read_command();
    keys.sort();

    assert_eq!(keys, ["baz", "foo"]);
}

#[test]
fn lists_only_the_keys_the_pattern_asks_for() {
    let saved = Saved::new(&dataset(&[("foo", "1"), ("food", "2"), ("bar", "3")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "foo*"]);
    let mut keys = client.read_command();
    keys.sort();

    assert_eq!(keys, ["foo", "food"]);
}

#[test]
fn lists_nothing_for_a_pattern_no_key_answers_to() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "nothing*"]);
    client.expect_reply("*0\r\n");
}

#[test]
fn keeps_a_saved_key_out_of_the_listing_once_its_time_has_passed() {
    let long_gone = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let saved = Saved::new(&dataset_expiring_at("gone", "value", long_gone));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*0\r\n");

    client.send(&["GET", "gone"]);
    client.expect_reply("$-1\r\n");
}

#[test]
fn keeps_a_saved_key_whose_time_has_yet_to_come() {
    let far_off = SystemTime::now() + std::time::Duration::from_secs(600);
    let saved = Saved::new(&dataset_expiring_at("later", "value", far_off));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.expect_reply("*1\r\n$5\r\nlater\r\n");
}

#[test]
fn refuses_a_keys_that_names_no_pattern() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS"]);
    client.expect_reply("-ERR wrong number of arguments for 'keys' command\r\n");

    client.send(&["KEYS", "a", "b"]);
    client.expect_reply("-ERR wrong number of arguments for 'keys' command\r\n");
}

#[test]
fn keeps_serving_the_connection_after_a_keys() {
    let saved = Saved::new(&dataset(&[("foo", "bar")]));
    let server = saved.server();
    let mut client = server.connect();

    client.send(&["KEYS", "*"]);
    client.read_reply();

    client.send(&["PING"]);
    client.expect_reply("+PONG\r\n");
}

#[test]
fn refuses_to_start_on_a_dataset_it_cannot_read() {
    let saved = Saved::new(b"this was never a dataset");

    // Coming up empty over a file that is there would look like the data had
    // gone, so the server says what is wrong and stops instead.
    let output = Command::new(env!("CARGO_BIN_EXE_codecrafters-redis"))
        .args(["--port", "0"])
        .args(saved.args())
        .output()
        .expect("failed to run the server");

    assert!(!output.status.success(), "the server started anyway");

    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("not a dataset"), "{said:?}");
    assert!(said.contains("dump.rdb"), "{said:?}");
}
