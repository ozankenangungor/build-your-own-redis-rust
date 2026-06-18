use anyhow::{Result, bail};

/// The port Redis listens on when nothing says otherwise.
const DEFAULT_PORT: u16 = 6379;

/// How the server was asked to run.
pub struct Config {
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self { port: DEFAULT_PORT }
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
                _ => bail!("unknown flag '{flag}'"),
            }
        }

        Ok(config)
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
