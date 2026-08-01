use super::{text, wrong_arity};
use crate::resp::Value;
use bytes::Bytes;

/// How many arguments one location takes: where it is, in two numbers, and what
/// it is called.
const PER_LOCATION: usize = 3;

/// How far east and west a location may lie: the whole way round, with the line
/// where the two meet counted at both ends.
const LONGITUDES: std::ops::RangeInclusive<f64> = -180.0..=180.0;

/// How far north and south. Not the whole way to the poles: Redis lays the
/// earth out on a square by the Mercator projection, and the poles are nowhere
/// on such a square. This is the latitude the square's edge falls at.
const LATITUDES: std::ops::RangeInclusive<f64> = -85.05112878..=85.05112878;

/// Where a location is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub longitude: f64,
    pub latitude: f64,
}

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
            [_key, located @ ..] if located.len() >= PER_LOCATION => add(located),
            _ => wrong_arity("geoadd"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Takes in the locations named, and says how many they were.
///
/// Nothing is kept yet, but a location is still read and looked over before it
/// is counted: a pair of numbers that names no place on the earth is refused
/// here rather than stored and puzzled over later.
fn add(located: &[Bytes]) -> Value {
    if !located.len().is_multiple_of(PER_LOCATION) {
        return syntax_error();
    }

    for location in located.chunks_exact(PER_LOCATION) {
        if let Err(error) = point(&location[0], &location[1]) {
            return error;
        }
    }

    Value::Integer((located.len() / PER_LOCATION) as i64)
}

/// Reads a place on the earth from the two numbers that name it.
///
/// Both are read before either is looked over, so that a pair of words that are
/// not numbers at all is answered as such rather than as a place out of range.
pub fn point(longitude: &Bytes, latitude: &Bytes) -> Result<Point, Value> {
    let (Some(longitude), Some(latitude)) = (coordinate(longitude), coordinate(latitude)) else {
        return Err(not_a_float());
    };

    if !LONGITUDES.contains(&longitude) || !LATITUDES.contains(&latitude) {
        return Err(out_of_range(longitude, latitude));
    }

    Ok(Point {
        longitude,
        latitude,
    })
}

/// Reads one of the two numbers. `nan` is turned down along with what is not a
/// number at all: it lies in no range, and a place that lies nowhere is no place.
fn coordinate(coordinate: &Bytes) -> Option<f64> {
    text(coordinate)
        .and_then(|coordinate| coordinate.parse::<f64>().ok())
        .filter(|coordinate| !coordinate.is_nan())
}

/// What Redis says of a pair naming no place on the earth. It names both
/// numbers whichever of them was at fault, and writes them out as it stores
/// them rather than as they were spelled.
fn out_of_range(longitude: f64, latitude: f64) -> Value {
    Value::Error(format!(
        "ERR invalid longitude,latitude pair {longitude:.6},{latitude:.6}"
    ))
}

fn not_a_float() -> Value {
    Value::Error("ERR value is not a valid float".into())
}

fn syntax_error() -> Value {
    Value::Error("ERR syntax error".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(word: &str) -> Bytes {
        Bytes::copy_from_slice(word.as_bytes())
    }

    fn read(longitude: &str, latitude: &str) -> Result<Point, Value> {
        point(&named(longitude), &named(latitude))
    }

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
    fn reads_a_place_on_the_earth() {
        assert_eq!(
            read("11.5030378", "48.1642721"),
            Ok(Point {
                longitude: 11.5030378,
                latitude: 48.1642721,
            })
        );
    }

    #[test]
    fn takes_a_place_on_the_very_edge_of_the_world() {
        // Both limits are the last place that counts, not the first that does
        // not.
        for (longitude, latitude) in [
            ("-180", "-85.05112878"),
            ("180", "85.05112878"),
            ("-180", "85.05112878"),
            ("0", "0"),
        ] {
            assert!(read(longitude, latitude).is_ok(), "{longitude},{latitude}");
        }
    }

    #[test]
    fn turns_down_a_place_past_the_edge_of_the_world() {
        for (longitude, latitude) in [
            ("180.1", "0"),
            ("-180.1", "0"),
            ("0", "85.05112879"),
            ("0", "-85.05112879"),
            // Within a longitude's range but past the latitude Redis lays the
            // earth out to, which stops short of the poles.
            ("0", "90"),
            ("0", "-90"),
        ] {
            assert!(read(longitude, latitude).is_err(), "{longitude},{latitude}");
        }
    }

    #[test]
    fn names_both_numbers_when_it_turns_a_place_down() {
        let Err(Value::Error(said)) = read("181", "0.3") else {
            panic!("a place past the edge of the world is refused");
        };

        assert_eq!(
            said,
            "ERR invalid longitude,latitude pair 181.000000,0.300000"
        );
        assert!(said.starts_with("ERR "), "{said:?}");
        assert!(
            said.contains("longitude") && said.contains("latitude"),
            "{said:?}"
        );
    }

    #[test]
    fn turns_down_numbers_that_are_not_numbers() {
        for (longitude, latitude) in [("east", "0"), ("0", "north"), ("", "0"), ("nan", "0")] {
            assert_eq!(
                read(longitude, latitude),
                Err(not_a_float()),
                "{longitude},{latitude}"
            );
        }
    }

    #[test]
    fn turns_down_a_location_naming_no_place_on_the_earth() {
        assert_eq!(
            geoadd(&["places", "180", "90", "test1"]),
            out_of_range(180.0, 90.0)
        );
    }

    #[test]
    fn counts_nothing_when_one_of_the_locations_will_not_read() {
        // The command is refused whole, as `ZADD` is: one bad location is not
        // a reason to take in the good ones beside it.
        let command = ["places", "11.5", "48.1", "Munich", "181", "0.3", "nowhere"];

        assert_eq!(geoadd(&command), out_of_range(181.0, 0.3));
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
