use anyhow::{Result, bail};

/// The port Redis listens on when nothing says otherwise.
const DEFAULT_PORT: u16 = 6379;

/// Where Redis keeps its dataset when nothing says otherwise: a file named
/// `dump.rdb`, in whichever directory the server was started from.
const DEFAULT_DBFILENAME: &str = "dump.rdb";

/// What Redis calls the append-only file, and the directory it keeps it in,
/// when nothing says otherwise.
const DEFAULT_APPENDDIRNAME: &str = "appendonlydir";
const DEFAULT_APPENDFILENAME: &str = "appendonly.aof";

/// How often Redis pushes what it has written through to the disk when nothing
/// says otherwise: once a second, which loses at most a second's work.
const DEFAULT_APPENDFSYNC: &str = "everysec";

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
    /// Whether every write is also written down as it happens, so that the
    /// dataset can be built back up command by command.
    pub appendonly: bool,
    /// The directory under [`Config::dir`] that the append-only file lives in,
    /// and the name it lives under.
    pub appenddirname: String,
    pub appendfilename: String,
    /// How often what has been written is pushed through to the disk.
    pub appendfsync: String,
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
            dir: starting_dir(),
            dbfilename: DEFAULT_DBFILENAME.to_string(),
            appendonly: false,
            appenddirname: DEFAULT_APPENDDIRNAME.to_string(),
            appendfilename: DEFAULT_APPENDFILENAME.to_string(),
            appendfsync: DEFAULT_APPENDFSYNC.to_string(),
        }
    }
}

/// The directory the server was started in, which is where it keeps its data
/// unless told otherwise.
///
/// Redis answers with a path rather than a `.`, since a client asking where the
/// data is wants somewhere it can go looking. A working directory that cannot
/// be read is no reason to refuse to start, so fall back to naming it as Redis
/// writes it in its own configuration file.
fn starting_dir() -> String {
    match std::env::current_dir() {
        Ok(dir) => dir.to_string_lossy().into_owned(),
        Err(_) => ".".to_string(),
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
                "appendonly" => config.appendonly = yes_or_no_given(&value)?,
                "appenddirname" => config.appenddirname = value,
                "appendfilename" => config.appendfilename = value,
                "appendfsync" => config.appendfsync = appendfsync_given(value)?,
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
            // A setting Redis keeps as a yes or a no is asked after the same
            // way as any other, and so has to answer in words.
            "appendonly" => ("appendonly", yes_or_no(self.appendonly)),
            "appenddirname" => ("appenddirname", &self.appenddirname),
            "appendfilename" => ("appendfilename", &self.appendfilename),
            "appendfsync" => ("appendfsync", &self.appendfsync),
            _ => return None,
        })
    }
}

/// How Redis writes a setting that is either on or off.
fn yes_or_no(setting: bool) -> &'static str {
    if setting { "yes" } else { "no" }
}

/// Reads a setting that is either on or off, in the words Redis takes it in.
///
/// Anything else is refused. There are only two answers to give, so a third is
/// a mistake, and a server that took `--appendonly ye` for a no would go on to
/// lose every write the flag was there to keep.
fn yes_or_no_given(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => bail!("expected 'yes' or 'no', found '{value}'"),
    }
}

/// The answers Redis takes to how often what has been written is pushed through
/// to the disk: on every write, once a second, or whenever the system sees fit.
const APPENDFSYNCS: [&str; 3] = ["always", "everysec", "no"];

/// Reads how often to push writes through to the disk, in the words Redis takes
/// it in. The value is kept as it was given, since it is answered for by
/// `CONFIG GET` and read back case insensitively.
///
/// Anything outside the three is refused, for the reason [`yes_or_no_given`]
/// refuses a third answer: this setting decides how much work a sudden stop can
/// cost, and a server that quietly took `--appendfsync alway` for the once a
/// second it was never asked for would be less safe than whoever started it
/// believes.
fn appendfsync_given(value: String) -> Result<String> {
    if !APPENDFSYNCS
        .iter()
        .any(|known| value.eq_ignore_ascii_case(known))
    {
        bail!("expected 'always', 'everysec' or 'no', found '{value}'");
    }

    Ok(value)
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
    use std::path::Path;

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

        // Wherever the server was started, said as a path a client can follow
        // rather than as the `.` that only means anything from here.
        assert_eq!(
            config.dir,
            std::env::current_dir().unwrap().to_string_lossy()
        );
        assert!(Path::new(&config.dir).is_absolute(), "{}", config.dir);
        assert_eq!(config.dbfilename, "dump.rdb");
    }

    #[test]
    fn writes_nothing_down_as_it_goes_unless_told_otherwise() {
        let config = parse(&[]).unwrap();

        assert!(!config.appendonly);
        assert_eq!(config.appenddirname, "appendonlydir");
        assert_eq!(config.appendfilename, "appendonly.aof");
        assert_eq!(config.appendfsync, "everysec");
    }

    #[test]
    fn takes_the_settings_the_append_only_file_is_to_be_kept_under() {
        let config = parse(&[
            "--appendonly",
            "yes",
            "--appenddirname",
            "aof",
            "--appendfilename",
            "writes.aof",
            "--appendfsync",
            "always",
        ])
        .unwrap();

        assert!(config.appendonly);
        assert_eq!(config.appenddirname, "aof");
        assert_eq!(config.appendfilename, "writes.aof");
        assert_eq!(config.appendfsync, "always");
    }

    #[test]
    fn takes_each_of_the_ways_of_pushing_writes_through_to_the_disk() {
        for spelling in ["always", "everysec", "no", "ALWAYS", "EverySec"] {
            let config = parse(&["--appendfsync", spelling]).unwrap();

            assert_eq!(config.appendfsync, spelling, "{spelling}");
        }
    }

    #[test]
    fn refuses_a_way_of_pushing_writes_through_that_it_has_never_heard_of() {
        // Taken quietly for the default, this would leave the server less safe
        // than whoever started it asked for.
        for spelling in ["alway", "always ", "sometimes", "yes", ""] {
            assert!(parse(&["--appendfsync", spelling]).is_err(), "{spelling:?}");
        }
    }

    #[test]
    fn keeps_the_defaults_for_the_append_only_settings_it_was_not_given() {
        // A flag left out is not a flag set to nothing: the rest stand as they
        // were, which is what makes passing one of the five safe.
        let config = parse(&["--appendonly", "yes"]).unwrap();

        assert!(config.appendonly);
        assert_eq!(config.appenddirname, "appendonlydir");
        assert_eq!(config.appendfilename, "appendonly.aof");
        assert_eq!(config.appendfsync, "everysec");
    }

    #[test]
    fn takes_the_append_only_settings_alongside_the_dataset_ones() {
        let config = parse(&["--dir", "/tmp/redis-files", "--appendonly", "yes"]).unwrap();

        assert_eq!(config.dir, "/tmp/redis-files");
        assert!(config.appendonly);
    }

    #[test]
    fn takes_a_yes_or_a_no_however_it_is_spelled() {
        for value in ["yes", "YES", "Yes"] {
            assert!(
                parse(&["--appendonly", value]).unwrap().appendonly,
                "{value}"
            );
        }

        for value in ["no", "NO", "No"] {
            assert!(
                !parse(&["--appendonly", value]).unwrap().appendonly,
                "{value}"
            );
        }
    }

    #[test]
    fn refuses_an_append_only_setting_that_is_neither_a_yes_nor_a_no() {
        // Taking a `ye` for a no would lose every write the flag was there to
        // keep, and say nothing about it.
        for value in ["ye", "true", "1", ""] {
            assert!(parse(&["--appendonly", value]).is_err(), "{value:?}");
        }
    }

    #[test]
    fn hands_back_the_settings_the_append_only_file_is_kept_under() {
        let config = parse(&[]).unwrap();

        assert_eq!(config.setting("appendonly"), Some(("appendonly", "no")));
        assert_eq!(
            config.setting("appenddirname"),
            Some(("appenddirname", "appendonlydir"))
        );
        assert_eq!(
            config.setting("appendfilename"),
            Some(("appendfilename", "appendonly.aof"))
        );
        assert_eq!(
            config.setting("appendfsync"),
            Some(("appendfsync", "everysec"))
        );
    }

    #[test]
    fn says_in_words_whether_it_writes_as_it_goes() {
        let mut config = parse(&[]).unwrap();

        assert_eq!(config.setting("APPENDONLY"), Some(("appendonly", "no")));

        config.appendonly = true;
        assert_eq!(config.setting("appendonly"), Some(("appendonly", "yes")));
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
