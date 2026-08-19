//! The append-only file: a record of every write as it happens, kept so that a
//! server coming back up can play its way to where it left off.

use crate::config::Config;
use crate::resp::Value;
use anyhow::{Context, Result};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// The directory the append-only files are kept in. It sits inside the
/// directory the dataset is kept in, under a name of its own, so that the two
/// ways of keeping the data do not tread on one another.
pub fn directory(config: &Config) -> PathBuf {
    Path::new(&config.dir).join(&config.appenddirname)
}

/// The first of the files the commands are recorded in as they come.
///
/// Redis numbers these and marks them `incr`, for the writes that have come in
/// since the last full copy of the dataset was written out. Only the first is
/// made here; the rest are for when a copy is written and the count starts over.
const FIRST: u32 = 1;

/// How the manifest marks a file holding the commands as they came in, one
/// after another, rather than a copy of the whole dataset at one moment.
const INCREMENTAL: &str = "i";

/// What `appendfsync` is set to when every write is to be pushed through to the
/// disk before the client is told it took.
const ALWAYS: &str = "always";

/// The record of the writes this server has been asked to make, kept open for
/// as long as it runs.
pub struct Aof {
    path: PathBuf,
    /// One lock over the file, so that two commands recorded at once come out
    /// one after the other rather than woven together.
    file: Mutex<tokio::fs::File>,
    /// Whether a write is pushed through to the disk before the client that
    /// asked for it is told it took.
    durable: bool,
}

impl Aof {
    /// The file the writes are being recorded in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The commands recorded so far, in the order they were recorded.
    ///
    /// A record that ends mid-command is taken for what it is: the last write
    /// of a server that was stopped in the middle of writing it down. The part
    /// that arrived is dropped, since a command half written was a command
    /// never answered. Anything else out of shape is refused, because a record
    /// that cannot be read through is one whose later commands are lost.
    pub fn recorded(&self) -> Result<Vec<Value>> {
        let recorded = std::fs::read(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;

        let mut commands = Vec::new();
        let mut at = 0;

        while at < recorded.len() {
            let read = crate::resp::parse(&recorded[at..])
                .with_context(|| format!("reading {} at byte {at}", self.path.display()))?;

            let Some((command, taken)) = read else {
                eprintln!(
                    "{} ends mid-command, {} bytes in; leaving the rest of it",
                    self.path.display(),
                    recorded.len() - at,
                );
                break;
            };

            commands.push(command);
            at += taken;
        }

        Ok(commands)
    }

    /// Records one command, exactly as the client sent it.
    ///
    /// The command is written down before the caller replies, so that a client
    /// is never told a write took when nothing knows of it but memory.
    pub async fn record(&self, command: &Value) -> Result<()> {
        let recorded = command.encode();
        let mut file = self.file.lock().await;

        file.write_all(&recorded)
            .await
            .with_context(|| format!("recording a command in {}", self.path.display()))?;

        // Written all the way out to the file rather than left in hand: a write
        // still waiting in this process is one nothing else can see, whatever
        // the client has been told.
        file.flush()
            .await
            .with_context(|| format!("writing out to {}", self.path.display()))?;

        if self.durable {
            file.sync_data()
                .await
                .with_context(|| format!("pushing {} through to the disk", self.path.display()))?;
        }

        Ok(())
    }
}

fn incremental_name(config: &Config, sequence: u32) -> String {
    format!("{}.{sequence}.incr.aof", config.appendfilename)
}

/// The manifest: a listing of the files the writes are spread over, and of the
/// order they are to be played back in.
///
/// A server coming back up reads this before anything else. Working the files
/// out from what happens to be in the directory would leave it guessing at
/// which of them came first.
fn manifest(config: &Config) -> PathBuf {
    directory(config).join(format!("{}.manifest", config.appendfilename))
}

/// The manifest as it stands with only the first file written to.
///
/// One line to a file, each a run of words separated by a single space and
/// closed by a newline, which is the shape Redis reads back.
fn listing(config: &Config) -> String {
    format!(
        "file {} seq {FIRST} type {INCREMENTAL}\n",
        incremental_name(config, FIRST)
    )
}

/// The name of the file the incoming commands belong in, according to a
/// manifest that is already there. `None` means there is no manifest to go on.
///
/// The last incremental file listed is the one still being added to: any before
/// it were closed off when a copy of the dataset was written out.
fn recorded_in(manifest: &Path) -> Result<Option<String>> {
    let listing = match std::fs::read_to_string(manifest) {
        Ok(listing) => listing,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", manifest.display())),
    };

    Ok(listing
        .lines()
        .filter_map(listed)
        .filter(|(_, kind)| kind == INCREMENTAL)
        .map(|(name, _)| name)
        .next_back())
}

/// One line of a manifest read as the file it names and what that file holds.
///
/// A line is a run of fields, each a name followed by its value. They are read
/// by name rather than by where they sit, and the ones this server has no use
/// for are passed over: a manifest written by a fuller Redis says more than
/// this one needs.
fn listed(line: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut kind = None;
    let mut words = line.split_whitespace();

    while let (Some(field), Some(value)) = (words.next(), words.next()) {
        match field {
            "file" => name = Some(value.to_string()),
            "type" => kind = Some(value.to_string()),
            _ => {}
        }
    }

    Some((name?, kind?))
}

/// Makes ready the record of the writes, for a server that was told to keep one.
///
/// Nothing is written to it yet. This only sees to it that there is somewhere to
/// write, and sees to it at startup rather than at the first write, so that no
/// write is ever the one to find the file missing.
///
/// A server told to keep no record leaves the directory alone. Making one it
/// would never write to would be a puzzle for whoever found it.
pub fn prepare(config: &Config) -> Result<Option<Aof>> {
    if !config.appendonly {
        return Ok(None);
    }

    let dir = directory(config);

    // A directory already there is the ordinary case, not a mistake: it is what
    // a server restarted over its own data finds.
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let manifest = manifest(config);

    // A manifest already there is what the writes are to follow, whatever this
    // server was started with. It is the record of what was written and in what
    // order, and settings that disagree with it are settings, not history.
    let name = match recorded_in(&manifest)? {
        Some(name) => name,
        None => {
            // Nothing has been recorded here before, so this server says what
            // the first file is to be.
            std::fs::write(&manifest, listing(config))
                .with_context(|| format!("writing {}", manifest.display()))?;

            incremental_name(config, FIRST)
        }
    };

    let path = dir.join(name);

    // Opened to be written on the end of rather than made afresh. A server that
    // finds a record of its own writes has no business throwing it away, least
    // of all before anything has had the chance to read it back.
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;

    Ok(Some(Aof {
        path,
        file: Mutex::new(tokio::fs::File::from_std(file)),
        durable: config.appendfsync.eq_ignore_ascii_case(ALWAYS),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A directory to work in, swept up when it goes out of scope.
    struct Somewhere(PathBuf);

    impl Somewhere {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);

            let dir = std::env::temp_dir().join(format!(
                "redis-aof-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));

            std::fs::create_dir_all(&dir).expect("failed to make a directory to work in");

            Self(dir)
        }

        /// A config pointing at this directory, as the flags would leave it.
        fn config(&self, appendonly: bool) -> Config {
            Config {
                dir: self.0.to_string_lossy().into_owned(),
                appendonly,
                ..Config::default()
            }
        }
    }

    impl Drop for Somewhere {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The file a server set up this way would record its writes in.
    fn made(config: &Config) -> PathBuf {
        prepare(config)
            .unwrap()
            .expect("a file was to be made")
            .path()
            .to_path_buf()
    }

    #[test]
    fn makes_somewhere_to_write_when_it_is_to_write_as_it_goes() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        let file = made(&config);
        let dir = somewhere.0.join("appendonlydir");

        assert!(dir.is_dir(), "{}", dir.display());
        assert_eq!(file, dir.join("appendonly.aof.1.incr.aof"));
        assert!(file.is_file(), "{}", file.display());
    }

    #[test]
    fn leaves_the_file_it_makes_empty() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // The file is there to be written to, not written to yet: what it holds
        // is what the commands to come put in it, and nothing besides.
        let file = made(&config);

        assert_eq!(std::fs::read(&file).unwrap(), b"");
    }

    #[test]
    fn makes_them_under_the_names_it_was_given() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.appenddirname = "my-writes".to_string();
        config.appendfilename = "writes.aof".to_string();

        let file = made(&config);

        assert!(somewhere.0.join("my-writes").is_dir());
        assert_eq!(
            file,
            somewhere.0.join("my-writes").join("writes.aof.1.incr.aof")
        );
        assert!(file.is_file());
    }

    #[test]
    fn writes_a_manifest_naming_the_file_it_made() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        prepare(&config).unwrap();

        let manifest = somewhere
            .0
            .join("appendonlydir")
            .join("appendonly.aof.manifest");

        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            "file appendonly.aof.1.incr.aof seq 1 type i\n"
        );
    }

    #[test]
    fn names_the_file_in_the_manifest_without_the_way_to_it() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.appenddirname = "my-writes".to_string();
        config.appendfilename = "writes.aof".to_string();

        prepare(&config).unwrap();

        // The manifest sits beside the files it lists, so it names them and
        // says nothing of where they are.
        let manifest = somewhere.0.join("my-writes").join("writes.aof.manifest");

        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            "file writes.aof.1.incr.aof seq 1 type i\n"
        );
    }

    /// Leaves a manifest behind for the next server to find, as one that had
    /// been running here would have.
    fn left_behind(somewhere: &Somewhere, listing: &str) -> PathBuf {
        let dir = somewhere.0.join("appendonlydir");
        std::fs::create_dir_all(&dir).expect("failed to make a directory to leave it in");

        let manifest = dir.join("appendonly.aof.manifest");
        std::fs::write(&manifest, listing).expect("failed to leave a manifest behind");

        manifest
    }

    #[test]
    fn records_in_the_file_the_manifest_it_finds_names() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        left_behind(&somewhere, "file elsewhere.aof.1.incr.aof seq 1 type i\n");

        // The manifest is the record of what was written and in what order.
        // Settings that disagree with it are settings, not history.
        let file = made(&config);

        assert_eq!(
            file,
            somewhere
                .0
                .join("appendonlydir")
                .join("elsewhere.aof.1.incr.aof")
        );
    }

    #[test]
    fn leaves_a_manifest_it_finds_as_it_found_it() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let listing = "file elsewhere.aof.1.incr.aof seq 4 type i\n";
        let manifest = left_behind(&somewhere, listing);

        prepare(&config).unwrap();

        assert_eq!(std::fs::read_to_string(&manifest).unwrap(), listing);
    }

    #[test]
    fn records_in_the_last_of_the_files_a_manifest_lists() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // A copy of the whole dataset, then the writes since. The last of them
        // is the one still being added to.
        left_behind(
            &somewhere,
            "file base.aof.1.base.rdb seq 1 type b\n\
             file writes.aof.1.incr.aof seq 1 type i\n\
             file writes.aof.2.incr.aof seq 2 type i\n",
        );

        assert_eq!(
            made(&config),
            somewhere
                .0
                .join("appendonlydir")
                .join("writes.aof.2.incr.aof")
        );
    }

    #[test]
    fn passes_over_what_a_manifest_says_that_it_has_no_use_for() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // A fuller Redis writes more fields than this server reads, and writes
        // them in whatever order it pleases.
        left_behind(
            &somewhere,
            "seq 7 type i file elsewhere.aof.7.incr.aof startoffset 12345\n",
        );

        assert_eq!(
            made(&config),
            somewhere
                .0
                .join("appendonlydir")
                .join("elsewhere.aof.7.incr.aof")
        );
    }

    #[test]
    fn writes_a_manifest_of_its_own_when_it_finds_none_it_can_follow() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // Lines that name no file, or say nothing of what one holds, are no
        // help in finding where the writes belong.
        let manifest = left_behind(&somewhere, "\n# nothing to go on\nfile lonely.aof\n");

        assert_eq!(
            made(&config),
            somewhere
                .0
                .join("appendonlydir")
                .join("appendonly.aof.1.incr.aof")
        );
        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            "file appendonly.aof.1.incr.aof seq 1 type i\n"
        );
    }

    #[test]
    fn makes_nothing_when_it_is_to_write_nothing() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(false);

        assert!(prepare(&config).unwrap().is_none());
        assert!(!somewhere.0.join("appendonlydir").exists());
        assert!(!somewhere.0.join("appendonly.aof.manifest").exists());
    }

    #[test]
    fn is_content_to_find_the_directory_already_there() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // A server restarted over its own data finds what it left behind, and
        // that is no reason to refuse to start.
        prepare(&config).unwrap();
        std::fs::write(somewhere.0.join("appendonlydir").join("kept"), b"kept")
            .expect("failed to leave something behind");

        prepare(&config).unwrap();

        assert!(somewhere.0.join("appendonlydir").join("kept").exists());
    }

    #[test]
    fn keeps_what_was_written_down_the_last_time_it_ran() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        let file = made(&config);
        std::fs::write(&file, b"*1\r\n$4\r\nPING\r\n").expect("failed to record a command");

        prepare(&config).unwrap();

        // Starting up is no reason to throw away a record of what was done, and
        // certainly not before anything has had the chance to read it back.
        assert_eq!(std::fs::read(&file).unwrap(), b"*1\r\n$4\r\nPING\r\n");
    }

    /// A command as a client would have sent it.
    fn sent(words: &[&str]) -> Value {
        Value::Array(
            words
                .iter()
                .map(|word| Value::BulkString(bytes::Bytes::copy_from_slice(word.as_bytes())))
                .collect(),
        )
    }

    #[tokio::test]
    async fn records_a_command_as_the_client_sent_it() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let aof = prepare(&config).unwrap().expect("a file was to be made");

        aof.record(&sent(&["SET", "foo", "100"])).await.unwrap();

        assert_eq!(
            std::fs::read(aof.path()).unwrap(),
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n100\r\n"
        );
    }

    #[tokio::test]
    async fn records_one_command_after_another() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let aof = prepare(&config).unwrap().expect("a file was to be made");

        aof.record(&sent(&["SET", "foo", "1"])).await.unwrap();
        aof.record(&sent(&["SET", "bar", "2"])).await.unwrap();

        assert_eq!(
            std::fs::read(aof.path()).unwrap(),
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n*3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n"
        );
    }

    #[tokio::test]
    async fn records_on_the_end_of_what_was_written_before() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        {
            let aof = prepare(&config).unwrap().expect("a file was to be made");
            aof.record(&sent(&["SET", "foo", "1"])).await.unwrap();
        }

        // A restart carries on from where the last run left off rather than
        // over it.
        let aof = prepare(&config).unwrap().expect("a file was to be made");
        aof.record(&sent(&["SET", "bar", "2"])).await.unwrap();

        assert_eq!(
            std::fs::read(aof.path()).unwrap(),
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n*3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n"
        );
    }

    #[tokio::test]
    async fn records_a_value_that_is_not_text() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let aof = prepare(&config).unwrap().expect("a file was to be made");

        // A value is a run of bytes and may hold anything, newlines included.
        let value = bytes::Bytes::from_static(b"\r\n\x00\xff");
        let command = Value::Array(vec![
            Value::BulkString(bytes::Bytes::from_static(b"SET")),
            Value::BulkString(bytes::Bytes::from_static(b"k")),
            Value::BulkString(value),
        ]);

        aof.record(&command).await.unwrap();

        assert_eq!(
            std::fs::read(aof.path()).unwrap(),
            b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$4\r\n\r\n\x00\xff\r\n"
        );
    }

    /// A server set up over a record that already holds these bytes.
    fn over(somewhere: &Somewhere, recorded: &[u8]) -> Aof {
        let dir = somewhere.0.join("appendonlydir");
        std::fs::create_dir_all(&dir).expect("failed to make a directory to leave it in");
        std::fs::write(dir.join("appendonly.aof.1.incr.aof"), recorded)
            .expect("failed to leave a record behind");

        prepare(&somewhere.config(true))
            .unwrap()
            .expect("a file was to be made")
    }

    #[test]
    fn reads_back_nothing_from_a_record_with_nothing_in_it() {
        let somewhere = Somewhere::new();

        assert_eq!(over(&somewhere, b"").recorded().unwrap(), []);
    }

    #[test]
    fn reads_back_the_one_command_a_record_holds() {
        let somewhere = Somewhere::new();
        let aof = over(&somewhere, b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\n123\r\n");

        assert_eq!(aof.recorded().unwrap(), [sent(&["SET", "foo", "123"])]);
    }

    #[test]
    fn reads_back_the_commands_in_the_order_they_were_recorded() {
        let somewhere = Somewhere::new();
        let aof = over(
            &somewhere,
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n\
              *2\r\n$4\r\nINCR\r\n$3\r\nfoo\r\n\
              *3\r\n$3\r\nSET\r\n$3\r\nbar\r\n$1\r\n2\r\n",
        );

        assert_eq!(
            aof.recorded().unwrap(),
            [
                sent(&["SET", "foo", "1"]),
                sent(&["INCR", "foo"]),
                sent(&["SET", "bar", "2"]),
            ]
        );
    }

    #[test]
    fn leaves_off_a_command_the_record_stops_in_the_middle_of() {
        let somewhere = Somewhere::new();

        // A server stopped while writing a command down leaves part of it
        // behind. A command half written was a command never answered, so what
        // arrived of it is dropped rather than guessed at.
        let aof = over(
            &somewhere,
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n*3\r\n$3\r\nSET\r\n$3\r\nba",
        );

        assert_eq!(aof.recorded().unwrap(), [sent(&["SET", "foo", "1"])]);
    }

    #[test]
    fn says_so_when_a_record_cannot_be_read_through() {
        let somewhere = Somewhere::new();

        // Not a partial command but a wrong one, which says nothing about where
        // the next begins: everything recorded after it would be lost.
        let aof = over(&somewhere, b"*3\r\n$3\r\nSET\r\nnonsense\r\n");

        assert!(aof.recorded().is_err());
    }

    #[tokio::test]
    async fn reads_back_what_it_has_just_recorded() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let aof = prepare(&config).unwrap().expect("a file was to be made");

        aof.record(&sent(&["SET", "foo", "1"])).await.unwrap();
        aof.record(&sent(&["SET", "bar", "2"])).await.unwrap();

        assert_eq!(
            aof.recorded().unwrap(),
            [sent(&["SET", "foo", "1"]), sent(&["SET", "bar", "2"])]
        );
    }

    #[tokio::test]
    async fn pushes_a_write_through_to_the_disk_when_told_to() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.appendfsync = "always".to_string();

        let aof = prepare(&config).unwrap().expect("a file was to be made");

        // What is asked for here cannot be seen from inside the process: all
        // that can be checked is that asking for it works and loses nothing.
        assert!(aof.durable);
        aof.record(&sent(&["SET", "foo", "1"])).await.unwrap();

        assert_eq!(
            std::fs::read(aof.path()).unwrap(),
            b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$1\r\n1\r\n"
        );
    }

    #[test]
    fn takes_how_often_to_push_through_to_the_disk_however_it_is_spelled() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);

        for spelling in ["always", "ALWAYS", "Always"] {
            config.appendfsync = spelling.to_string();
            assert!(
                prepare(&config).unwrap().expect("a file").durable,
                "{spelling}"
            );
        }

        for other in ["everysec", "no"] {
            config.appendfsync = other.to_string();
            assert!(
                !prepare(&config).unwrap().expect("a file").durable,
                "{other}"
            );
        }
    }

    #[test]
    fn makes_the_directory_the_dataset_is_kept_in_too_if_it_has_to() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.dir = somewhere.0.join("data").to_string_lossy().into_owned();

        prepare(&config).unwrap();

        assert!(somewhere.0.join("data").join("appendonlydir").is_dir());
    }

    #[test]
    fn says_so_when_it_cannot_make_somewhere_to_write() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // A file where the directory belongs. Coming up as though all were well
        // would leave every write afterwards with nowhere to go.
        std::fs::write(somewhere.0.join("appendonlydir"), b"not a directory")
            .expect("failed to put a file in the way");

        assert!(prepare(&config).is_err());
    }

    #[test]
    fn says_so_when_it_cannot_make_the_file_to_write_in() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        // A directory where the file belongs, which is nothing to be written to.
        std::fs::create_dir_all(
            somewhere
                .0
                .join("appendonlydir")
                .join("appendonly.aof.1.incr.aof"),
        )
        .expect("failed to put a directory in the way");

        assert!(prepare(&config).is_err());
    }
}
