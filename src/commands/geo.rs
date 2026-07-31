use super::wrong_arity;
use crate::resp::Value;
use bytes::Bytes;

/// How many arguments one location takes: where it is, in two numbers, and what
/// it is called.
const PER_LOCATION: usize = 3;

/// Handles the commands that work on places on the earth. `None` means the
/// command belongs to another module.
///
/// Nothing is kept yet. A location is counted as it comes in and let go of
/// again; where it is to be put, and what is to be made of the numbers, is
/// still to come.
pub fn run(command: &str, args: &[Bytes]) -> Option<Value> {
    let reply = match command {
        // A key and at least one whole location. Fewer words than that is a
        // command missing an argument; more, but not by a whole location, is
        // one whose arguments do not pair up.
        "GEOADD" => match args {
            [_key, located @ ..] if located.len() >= PER_LOCATION => {
                match located.len().is_multiple_of(PER_LOCATION) {
                    true => Value::Integer((located.len() / PER_LOCATION) as i64),
                    false => syntax_error(),
                }
            }
            _ => wrong_arity("geoadd"),
        },
        _ => return None,
    };

    Some(reply)
}

fn syntax_error() -> Value {
    Value::Error("ERR syntax error".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geoadd(words: &[&str]) -> Value {
        let args: Vec<Bytes> = words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect();

        run("GEOADD", &args).expect("geoadd belongs to this module")
    }

    #[test]
    fn counts_the_location_it_is_given() {
        let command = ["places", "-0.0884948", "51.5006479", "London"];

        assert_eq!(geoadd(&command), Value::Integer(1));
    }

    #[test]
    fn counts_each_of_the_locations_named_in_one_go() {
        let command = [
            "places",
            "-0.0884948",
            "51.5006479",
            "London",
            "11.5030378",
            "48.1642710",
            "Munich",
        ];

        assert_eq!(geoadd(&command), Value::Integer(2));
    }

    #[test]
    fn refuses_a_geoadd_short_of_one_whole_location() {
        for command in [
            vec![],
            vec!["places"],
            vec!["places", "11.5"],
            vec!["places", "11.5", "48.1"],
        ] {
            assert_eq!(geoadd(&command), wrong_arity("geoadd"), "{command:?}");
        }
    }

    #[test]
    fn refuses_locations_that_do_not_pair_up() {
        for command in [
            vec!["places", "11.5", "48.1", "Munich", "0.0"],
            vec!["places", "11.5", "48.1", "Munich", "0.0", "51.5"],
        ] {
            assert_eq!(geoadd(&command), syntax_error(), "{command:?}");
        }
    }

    #[test]
    fn leaves_alone_the_commands_that_are_not_its_own() {
        assert!(run("GET", &[]).is_none());
    }
}
