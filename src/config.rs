use anyhow::{Result, bail};

/// The port Redis listens on when nothing says otherwise.
const DEFAULT_PORT: u16 = 6379;

/// Where Redis keeps its dataset when nothing says otherwise: a file named
/// `dump.rdb`, in whichever directory the server was started from.
const DEFAULT_DIR: &str = ".";
const DEFAULT_DBFILENAME: &str = "dump.rdb";

/// How the server was asked to run.
#[derive(Debug, PartialEq)]
pub struct Config {
    pub port: u16,
    /// The master this server was told to follow, if it is a replica.
    pub replicaof: Option<Master>,
    /// The directory the dataset is kept in, and the name it is kept under.
    /// Together they say where this server's data is to be found between runs.
    pub dir: String,
    pub dbfilename: String,
}

/// Where to find the master a replica follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Master {
    pub host: String,
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            replicaof: None,
            dir: DEFAULT_DIR.to_string(),
            dbfilename: DEFAULT_DBFILENAME.to_string(),
        }
    }
}

impl Config {
    /// Reads the settings the server was started with.
    pub fn from_args() -> Result<Self> {
        // The first argument is the path this program was run as.
        Self::parse(std::env::args().skip(1))
    }

    /// Reads settings from `--name value` pairs, in the style Redis takes them
    /// on the command line. Anything unrecognised is refused rather than
    /// ignored, so that a mistyped flag is not quietly left out.
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut config = Self::default();
        let mut args = args.into_iter();

        while let Some(flag) = args.next() {
            let Some(name) = flag.strip_prefix("--") else {
                bail!("expected a flag, found '{flag}'");
            };
            let Some(value) = args.next() else {
                bail!("'{flag}' takes a value, and none was given");
            };

            match name {
                "port" => config.port = value.parse()?,
                "replicaof" => config.replicaof = Some(Master::parse(&value)?),
                "dir" => config.dir = value,
                "dbfilename" => config.dbfilename = value,
                _ => bail!("unknown flag '{flag}'"),
            }
        }

        Ok(config)
    }

    /// A setting under the name `CONFIG GET` knows it by, along with that name
    /// spelled the way Redis spells it, since a client may ask in any case.
    ///
    /// The port is left out on purpose. The server may end up listening on
    /// another one than it was asked for, and a setting that reports the asking
    /// rather than the outcome would be worse than no setting at all.
    pub fn setting(&self, name: &str) -> Option<(&'static str, &str)> {
        Some(match name.to_ascii_lowercase().as_str() {
            "dir" => ("dir", &self.dir),
            "dbfilename" => ("dbfilename", &self.dbfilename),
            _ => return None,
        })
    }
}

impl Master {
    /// Redis takes both halves of the master's address as one argument, as in
    /// `--replicaof "localhost 6379"`.
    fn parse(value: &str) -> Result<Self> {
        let mut parts = value.split_whitespace();

        let (Some(host), Some(port), None) = (parts.next(), parts.next(), parts.next()) else {
            bail!("expected a host and a port, found '{value}'");
        };

        Ok(Self {
            host: host.to_string(),
            port: port.parse()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config> {
        Config::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn listens_on_the_usual_port_when_not_told_otherwise() {
        assert_eq!(parse(&[]).unwrap().port, 6379);
    }

    #[test]
    fn takes_the_port_it_is_given() {
        assert_eq!(parse(&["--port", "6380"]).unwrap().port, 6380);
    }

    #[test]
    fn takes_the_last_of_a_repeated_flag() {
        assert_eq!(
            parse(&["--port", "6380", "--port", "6381"]).unwrap().port,
            6381
        );
    }

    #[test]
    fn follows_no_master_unless_told_to() {
        assert_eq!(parse(&[]).unwrap().replicaof, None);
    }

    #[test]
    fn takes_the_master_to_follow_as_one_argument() {
        let config = parse(&["--replicaof", "localhost 6379"]).unwrap();

        assert_eq!(
            config.replicaof,
            Some(Master {
                host: "localhost".to_string(),
                port: 6379,
            })
        );
    }

    #[test]
    fn takes_the_master_alongside_the_port_to_listen_on() {
        let config = parse(&["--port", "6380", "--replicaof", "127.0.0.1 6379"]).unwrap();

        assert_eq!(config.port, 6380);
        assert_eq!(config.replicaof.unwrap().port, 6379);
    }

    #[test]
    fn keeps_its_dataset_where_redis_does_unless_told_otherwise() {
        let config = parse(&[]).unwrap();

        assert_eq!(config.dir, ".");
        assert_eq!(config.dbfilename, "dump.rdb");
    }

    #[test]
    fn takes_the_dataset_to_keep_and_where_to_keep_it() {
        let config = parse(&["--dir", "/tmp/redis-files", "--dbfilename", "dump.rdb"]).unwrap();

        assert_eq!(config.dir, "/tmp/redis-files");
        assert_eq!(config.dbfilename, "dump.rdb");
    }

    #[test]
    fn hands_back_the_settings_it_was_given() {
        let config = parse(&["--dir", "/tmp/redis-files", "--dbfilename", "rdbfile"]).unwrap();

        assert_eq!(config.setting("dir"), Some(("dir", "/tmp/redis-files")));
        assert_eq!(
            config.setting("dbfilename"),
            Some(("dbfilename", "rdbfile"))
        );
    }

    #[test]
    fn hands_back_a_setting_whichever_way_it_is_spelled() {
        let config = parse(&["--dir", "/tmp"]).unwrap();

        assert_eq!(config.setting("DIR"), Some(("dir", "/tmp")));
        assert_eq!(config.setting("Dir"), Some(("dir", "/tmp")));
    }

    #[test]
    fn has_no_setting_it_was_never_given() {
        let config = parse(&[]).unwrap();

        // The port is one this server keeps to itself, since what it was asked
        // for is not always what it ended up listening on.
        assert_eq!(config.setting("port"), None);
        assert_eq!(config.setting("maxmemory"), None);
        assert_eq!(config.setting(""), None);
    }

    #[test]
    fn refuses_a_master_it_cannot_make_sense_of() {
        for value in ["localhost", "localhost 6379 extra", "", "localhost http"] {
            assert!(parse(&["--replicaof", value]).is_err(), "{value:?}");
        }
    }

    #[test]
    fn refuses_arguments_it_cannot_make_sense_of() {
        for args in [
            // A flag it does not know, rather than one silently ignored.
            vec!["--colour", "red"],
            // A flag with nothing after it.
            vec!["--port"],
            // A value where a flag belongs.
            vec!["6380"],
            // Ports that are not numbers, or are too large to be one.
            vec!["--port", "http"],
            vec!["--port", "65536"],
            vec!["--port", "-1"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
}
