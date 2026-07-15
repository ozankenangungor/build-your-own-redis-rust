//! The append-only file: a record of every write as it happens, kept so that a
//! server coming back up can play its way to where it left off.

use crate::config::Config;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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

/// The file the commands coming in are recorded in, under the name Redis gives
/// the `sequence`th of them.
pub fn incremental(config: &Config, sequence: u32) -> PathBuf {
    directory(config).join(incremental_name(config, sequence))
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

/// Makes ready the place the writes are to be recorded, for a server that was
/// told to record them, and hands back the file they are to be recorded in.
///
/// Nothing is written here yet. This only sees to it that there is somewhere to
/// write, and sees to it at startup rather than at the first write, so that no
/// write is ever the one to find the file missing.
///
/// A server told to record nothing leaves the directory alone. Making one it
/// would never write to would be a puzzle for whoever found it.
pub fn prepare(config: &Config) -> Result<Option<PathBuf>> {
    if !config.appendonly {
        return Ok(None);
    }

    let dir = directory(config);

    // A directory already there is the ordinary case, not a mistake: it is what
    // a server restarted over its own data finds.
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let file = incremental(config, FIRST);

    // Opened to be written on the end of rather than made afresh. A server that
    // finds a record of its own writes has no business throwing it away, least
    // of all before anything has had the chance to read it back.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .with_context(|| format!("opening {}", file.display()))?;

    // The manifest says what the files are, not what is in them, so it is
    // written out as it stands rather than added to. Two servers over one
    // directory would be a worse mistake than either of their manifests.
    let manifest = manifest(config);
    std::fs::write(&manifest, listing(config))
        .with_context(|| format!("writing {}", manifest.display()))?;

    Ok(Some(file))
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

    #[test]
    fn makes_somewhere_to_write_when_it_is_to_write_as_it_goes() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);

        let file = prepare(&config).unwrap().expect("a file was to be made");
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
        let file = prepare(&config).unwrap().expect("a file was to be made");

        assert_eq!(std::fs::read(&file).unwrap(), b"");
    }

    #[test]
    fn makes_them_under_the_names_it_was_given() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.appenddirname = "my-writes".to_string();
        config.appendfilename = "writes.aof".to_string();

        let file = prepare(&config).unwrap().expect("a file was to be made");

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

    #[test]
    fn writes_the_manifest_out_again_on_a_restart() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(true);
        let manifest = somewhere
            .0
            .join("appendonlydir")
            .join("appendonly.aof.manifest");

        prepare(&config).unwrap();
        std::fs::write(&manifest, "file nothing.aof seq 9 type i\n")
            .expect("failed to leave a stale manifest behind");

        // The manifest says what the files are, not what is in them, so it is
        // rewritten rather than added to or left as it was found.
        prepare(&config).unwrap();

        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            "file appendonly.aof.1.incr.aof seq 1 type i\n"
        );
    }

    #[test]
    fn makes_nothing_when_it_is_to_write_nothing() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(false);

        assert_eq!(prepare(&config).unwrap(), None);
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

        let file = prepare(&config).unwrap().expect("a file was to be made");
        std::fs::write(&file, b"*1\r\n$4\r\nPING\r\n").expect("failed to record a command");

        prepare(&config).unwrap();

        // Starting up is no reason to throw away a record of what was done, and
        // certainly not before anything has had the chance to read it back.
        assert_eq!(std::fs::read(&file).unwrap(), b"*1\r\n$4\r\nPING\r\n");
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
