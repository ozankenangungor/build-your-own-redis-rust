use super::{text, wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{Store, WrongType};
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
/// A location is kept as a member of an ordinary sorted set, which is how Redis
/// keeps one: the two numbers are worked into a single score, and everything a
/// sorted set can already do works on places as it does on anything else.
pub fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
    let reply = match command {
        // A key and at least one whole location. Fewer words than that is a
        // command missing an argument; more, but not by a whole location, is
        // one whose arguments do not pair up.
        "GEOADD" => match args {
            [key, located @ ..] if located.len() >= PER_LOCATION => add(store, key, located),
            _ => wrong_arity("geoadd"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Puts the locations named into the sorted set at `key`, and says how many of
/// them were places it had never held.
///
/// Every location is read and looked over before any is put away, so that a
/// pair of numbers naming no place on the earth turns the whole command down
/// rather than half of it.
fn add(store: &Store, key: &Bytes, located: &[Bytes]) -> Value {
    if !located.len().is_multiple_of(PER_LOCATION) {
        return syntax_error();
    }

    let mut members = Vec::with_capacity(located.len() / PER_LOCATION);

    for location in located.chunks_exact(PER_LOCATION) {
        match point(&location[0], &location[1]) {
            Ok(point) => members.push((score(point), location[2].clone())),
            Err(error) => return error,
        }
    }

    match store.zadd(key, &members) {
        Ok(added) => Value::Integer(added as i64),
        Err(WrongType) => wrong_type(),
    }
}

/// The single number a place is kept under.
///
/// Still to be worked out from the two the place was named by. Until it is,
/// every place is kept under the same one, which is enough to hold a place by
/// name and no more.
fn score(_point: Point) -> f64 {
    0.0
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
        geoadd_to(words, &Store::default())
    }

    fn geoadd_to(words: &[&str], store: &Store) -> Value {
        let args: Vec<Bytes> = words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect();

        run("GEOADD", &args, store).expect("geoadd belongs to this module")
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
    fn keeps_the_location_as_a_member_of_a_sorted_set() {
        let store = Store::default();

        geoadd_to(&["places", "2.2944692", "48.8584625", "Paris"], &store);

        assert_eq!(store.zrank(&named("places"), &named("Paris")), Ok(Some(0)));
        assert_eq!(store.zcard(&named("places")), Ok(1));
    }

    #[test]
    fn counts_nothing_for_a_place_the_key_already_held() {
        let store = Store::default();

        geoadd_to(&["places", "2.2944692", "48.8584625", "Paris"], &store);

        assert_eq!(
            geoadd_to(&["places", "11.5030378", "48.1642721", "Paris"], &store),
            Value::Integer(0)
        );
        assert_eq!(store.zcard(&named("places")), Ok(1));
    }

    #[test]
    fn keeps_nothing_when_one_of_the_locations_will_not_read() {
        let store = Store::default();
        let command = ["places", "11.5", "48.1", "Munich", "181", "0.3", "nowhere"];

        geoadd_to(&command, &store);

        // The good location beside the bad one went nowhere either.
        assert_eq!(store.zcard(&named("places")), Ok(0));
    }

    #[test]
    fn will_not_keep_a_location_under_a_key_holding_something_else() {
        let store = Store::default();

        store.set(named("places"), named("a string"), None);

        assert_eq!(
            geoadd_to(&["places", "2.2944692", "48.8584625", "Paris"], &store),
            wrong_type()
        );
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
        assert!(run("GET", &[], &Store::default()).is_none());
    }
}
