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

/// Makes ready the place the writes are to be recorded, for a server that was
/// told to record them.
///
/// Nothing is written here yet. This only sees to it that there is somewhere to
/// write, and sees to it at startup rather than at the first write, so that no
/// write is ever the one to find the directory missing.
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

    Ok(Some(dir))
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

        let dir = prepare(&config)
            .unwrap()
            .expect("a directory was to be made");

        assert_eq!(dir, somewhere.0.join("appendonlydir"));
        assert!(dir.is_dir(), "{}", dir.display());
    }

    #[test]
    fn makes_it_under_the_name_it_was_given() {
        let somewhere = Somewhere::new();
        let mut config = somewhere.config(true);
        config.appenddirname = "my-writes".to_string();

        prepare(&config).unwrap();

        assert!(somewhere.0.join("my-writes").is_dir());
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
}
