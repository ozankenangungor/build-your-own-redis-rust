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

/// The file the commands coming in are recorded in, under the name Redis gives
/// the `sequence`th of them.
pub fn incremental(config: &Config, sequence: u32) -> PathBuf {
    directory(config).join(format!("{}.{sequence}.incr.aof", config.appendfilename))
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
    fn makes_nothing_when_it_is_to_write_nothing() {
        let somewhere = Somewhere::new();
        let config = somewhere.config(false);

        assert_eq!(prepare(&config).unwrap(), None);
        assert!(!somewhere.0.join("appendonlydir").exists());
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
