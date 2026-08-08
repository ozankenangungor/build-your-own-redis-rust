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

/// How finely the earth is cut up to give a place its score: this many squares
/// from side to side, and as many from top to bottom.
const BITS: u32 = 26;

/// How wide the earth is taken to be, in metres. Redis measures with this one,
/// so this server measures with it too: a distance is only ever compared with
/// another worked out the same way.
const EARTH_RADIUS: f64 = 6_372_797.560_856;

/// The unit a distance is given in unless another is asked for.
const METRES: f64 = 1.0;

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
        // Where each of the places named is, one answer to a place and in the
        // order they were asked after. A place the key does not hold is
        // answered with nothing at all rather than left out, so that the
        // answers still line up with the asking.
        "GEOPOS" => match args {
            [key, places @ ..] if !places.is_empty() => positions(store, key, places),
            _ => wrong_arity("geopos"),
        },
        // How far apart two places are. Either of them being one the key does
        // not hold leaves nothing to measure between.
        "GEODIST" => match args {
            [key, from, to] => distance(store, key, from, to, None),
            [key, from, to, unit] => distance(store, key, from, to, Some(unit)),
            _ => wrong_arity("geodist"),
        },
        _ => return None,
    };

    Some(reply)
}

/// How far apart two of the places a key holds are.
fn distance(store: &Store, key: &Bytes, from: &Bytes, to: &Bytes, unit: Option<&Bytes>) -> Value {
    let unit = match unit.map_or(Ok(METRES), measure) {
        Ok(unit) => unit,
        Err(error) => return error,
    };

    let (from, to) = match (store.zscore(key, from), store.zscore(key, to)) {
        (Ok(Some(from)), Ok(Some(to))) => (decode(from), decode(to)),
        (Err(WrongType), _) | (_, Err(WrongType)) => return wrong_type(),
        _ => return Value::Null,
    };

    // Four places after the point, as Redis answers: a fraction of a millimetre
    // is further than the whereabouts are known to in the first place.
    Value::BulkString(Bytes::from(format!("{:.4}", apart(from, to) / unit)))
}

/// How many metres the unit a distance was asked for is worth.
fn measure(unit: &Bytes) -> Result<f64, Value> {
    match text(unit).map(str::to_lowercase).as_deref() {
        Some("m") => Ok(METRES),
        Some("km") => Ok(1000.0),
        Some("mi") => Ok(1609.34),
        Some("ft") => Ok(0.3048),
        _ => Err(unsupported_unit()),
    }
}

/// How far apart two places on the earth are, in metres, going the shorter way
/// round it.
///
/// Worked out by the haversine formula, which takes the earth for a ball. It is
/// not one, so the answer is a little out for places far apart; Redis takes the
/// same shortcut, and off the same size of ball.
fn apart(from: Point, to: Point) -> f64 {
    let east = (to.longitude.to_radians() - from.longitude.to_radians()) / 2.0;
    let north = (to.latitude.to_radians() - from.latitude.to_radians()) / 2.0;

    let (from, to) = (from.latitude.to_radians(), to.latitude.to_radians());
    let haversine = north.sin().powi(2) + from.cos() * to.cos() * east.sin().powi(2);

    2.0 * EARTH_RADIUS * haversine.sqrt().asin()
}

fn unsupported_unit() -> Value {
    Value::Error("ERR unsupported unit provided. please use M, KM, FT, MI".into())
}

/// Says where each of these places is.
fn positions(store: &Store, key: &Bytes, places: &[Bytes]) -> Value {
    let mut found = Vec::with_capacity(places.len());

    for place in places {
        let position = match store.zscore(key, place) {
            Ok(Some(score)) => position(score),
            Ok(None) => Value::NullArray,
            Err(WrongType) => return wrong_type(),
        };

        found.push(position);
    }

    Value::Array(found)
}

/// The two numbers a score was made from, read back out of it.
fn position(score: f64) -> Value {
    let point = decode(score);

    Value::Array(vec![
        Value::BulkString(written(point.longitude)),
        Value::BulkString(written(point.latitude)),
    ])
}

/// Unpicks a score into the place it was made from.
///
/// The weaving is undone to find the square the place fell in, and the middle
/// of that square is given back. It is not quite where the place was: a square
/// stands for everything inside it, so what comes out is the same place to
/// within the width of one square and no closer. Redis loses the difference the
/// same way.
fn decode(score: f64) -> Point {
    let score = score as u64;
    let north = gather(score);
    let east = gather(score >> 1);

    Point {
        longitude: middle_of(east, &LONGITUDES),
        latitude: middle_of(north, &LATITUDES),
    }
}

/// The middle of the `square`th square along a range cut into `2^26` of them.
fn middle_of(square: u32, range: &std::ops::RangeInclusive<f64>) -> f64 {
    let span = range.end() - range.start();
    let squares = f64::from(1u32 << BITS);

    let low = range.start() + span * (f64::from(square) / squares);
    let high = range.start() + span * (f64::from(square + 1) / squares);

    (low + high) / 2.0
}

/// Gathers up every other bit of a number, closing the gaps that [`spread`]
/// opened. What comes back is one of the two numbers that were woven together.
fn gather(woven: u64) -> u32 {
    let mut gathered = woven & 0x5555555555555555;

    for (shift, keep) in [
        (1, 0x3333333333333333),
        (2, 0x0F0F0F0F0F0F0F0F),
        (4, 0x00FF00FF00FF00FF),
        (8, 0x0000FFFF0000FFFF),
        (16, 0x00000000FFFFFFFF),
    ] {
        gathered = (gathered | (gathered >> shift)) & keep;
    }

    gathered as u32
}

/// Writes a coordinate back out the way Redis writes one: to seventeen places
/// after the point, which is as fine as a double is worth, with the noughts on
/// the end left off as having nothing to say.
fn written(coordinate: f64) -> Bytes {
    let written = format!("{coordinate:.17}");
    let written = written.trim_end_matches('0').trim_end_matches('.');

    Bytes::copy_from_slice(written.as_bytes())
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
/// The earth is cut into a grid of `2^26` by `2^26` squares, and the place is
/// numbered by the square it falls in. The two numbers of that square are then
/// woven together bit by bit, north-south into the even bits and east-west into
/// the odd ones, which is what makes one number out of two.
///
/// Woven this way, places that are near one another come out with scores near
/// one another, so a sorted set holding them is already sorted by whereabouts.
/// That is the whole reason Redis keeps places in a sorted set at all.
///
/// A place anywhere inside the grid comes to 52 bits, woven from two of 26,
/// against the 53 a double counts exactly, so the score comes back out of a
/// sorted set as it went in.
///
/// A place on the very edge is the one exception: the last square along counts
/// as one past the end, and the score runs a bit wider than a double is sure
/// of. Redis works the number out the same way and keeps it in a double too, so
/// it stands where Redis stands.
fn score(point: Point) -> f64 {
    let north = squares_along(point.latitude, &LATITUDES);
    let east = squares_along(point.longitude, &LONGITUDES);

    (spread(north) | (spread(east) << 1)) as f64
}

/// How many squares along its range a coordinate falls, counting from the low
/// end. The grid is `2^26` squares wide either way.
fn squares_along(coordinate: f64, range: &std::ops::RangeInclusive<f64>) -> u32 {
    let along = (coordinate - range.start()) / (range.end() - range.start());

    (along * f64::from(1u32 << BITS)) as u32
}

/// Spreads the bits of a number out, a gap left after each, so that two numbers
/// spread this way slot into one another without meeting.
///
/// Done by halves: the number is split in two and moved apart, then each half
/// is split and moved apart again, five times over, until every bit stands
/// alone.
fn spread(value: u32) -> u64 {
    let mut spread = u64::from(value);

    for (shift, keep) in [
        (16, 0x0000FFFF0000FFFF),
        (8, 0x00FF00FF00FF00FF),
        (4, 0x0F0F0F0F0F0F0F0F),
        (2, 0x3333333333333333),
        (1, 0x5555555555555555),
    ] {
        spread = (spread | (spread << shift)) & keep;
    }

    spread
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

    fn scored(longitude: f64, latitude: f64) -> f64 {
        score(Point {
            longitude,
            latitude,
        })
    }

    #[test]
    fn scores_a_place_the_way_redis_scores_it() {
        // Paris, and the number this stage says it comes to.
        assert_eq!(scored(2.2944692, 48.8584625), 3663832614298053.0);
        assert_eq!(scored(-0.1277583, 51.5073509), 2163557714754256.0);
    }

    #[test]
    fn scores_the_places_of_the_world() {
        for (place, longitude, latitude, expected) in [
            ("Bangkok", 100.5252, 13.7220, 3962257306574459.0),
            ("Beijing", 116.3972, 39.9075, 4069885364908765.0),
            ("Berlin", 13.4105, 52.5244, 3673983964876493.0),
            ("Copenhagen", 12.5655, 55.6759, 3685973395504349.0),
            ("New Delhi", 77.2167, 28.6667, 3631527070936756.0),
            ("Kathmandu", 85.3206, 27.7017, 3639507404773204.0),
            ("London", -0.1278, 51.5074, 2163557714755072.0),
            ("New York", -74.0060, 40.7128, 1791873974549446.0),
            ("Paris", 2.3488, 48.8534, 3663832752681684.0),
            ("Sydney", 151.2093, -33.8688, 3252046221964352.0),
            ("Tokyo", 139.6917, 35.6895, 4171231230197045.0),
            ("Vienna", 16.3731, 48.2085, 3673109837763887.0),
        ] {
            assert_eq!(scored(longitude, latitude), expected, "{place}");
        }
    }

    #[test]
    fn scores_a_place_at_a_number_a_double_holds_whole() {
        // Two numbers of 26 bits woven together come to 52, one short of the 53
        // a double counts exactly, so no score of a place inside the grid is
        // ever rounded on its way out.
        for (longitude, latitude) in [
            (179.9999, 85.0511),
            (-180.0, -85.05112878),
            (0.0, 0.0),
            (2.2944692, 48.8584625),
        ] {
            let score = scored(longitude, latitude);

            assert_eq!(score, score.trunc(), "{longitude},{latitude}");
            assert!(score < 2f64.powi(53), "{longitude},{latitude}");
        }
    }

    #[test]
    fn counts_the_far_edge_of_the_world_as_one_square_past_the_end() {
        // Redis works the number out this way and keeps it in a double too, so
        // the one place where the score runs wide is the one place Redis's does.
        assert_eq!(scored(180.0, 85.05112878), 3.0 * 2f64.powi(52));
    }

    #[test]
    fn scores_places_further_east_and_north_the_higher() {
        // Weaving the two together leaves the order of each of them intact
        // along its own line, which is what makes a sorted set of places good
        // for anything.
        assert!(scored(0.0, 0.0) < scored(1.0, 0.0));
        assert!(scored(0.0, 0.0) < scored(0.0, 1.0));
        assert!(scored(-180.0, 0.0) < scored(180.0, 0.0));
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

    fn geopos(words: &[&str], store: &Store) -> Value {
        let args: Vec<Bytes> = words.iter().copied().map(named).collect();

        run("GEOPOS", &args, store).expect("geopos belongs to this module")
    }

    #[test]
    fn writes_a_coordinate_back_out_the_way_redis_writes_one() {
        for (coordinate, spelled) in [
            (0.0, "0"),
            (100.0, "100"),
            (-0.5, "-0.5"),
            (0.25, "0.25"),
            // Seventeen places, and no rounding to a shorter spelling that
            // would read back as the same number: this says what the double
            // really holds, as Redis does.
            (51.5073509, "51.50735089999999872"),
        ] {
            assert_eq!(written(coordinate), spelled, "{coordinate}");
        }
    }

    /// How close a place read back out of a score has to be to where it went
    /// in: within the width of one square. The middle of the square is what
    /// comes back, and the place was somewhere inside it — on the very edge of
    /// it, if the place fell on a boundary.
    fn within_a_square(range: &std::ops::RangeInclusive<f64>) -> f64 {
        (range.end() - range.start()) / f64::from(1u32 << BITS)
    }

    /// How close a place read back has to be to the numbers worked out for it
    /// elsewhere. These are the same arithmetic, so nothing but the last few
    /// figures should differ.
    const CLOSE_ENOUGH: f64 = 0.000001;

    #[test]
    fn reads_a_place_back_out_of_its_score() {
        for (score, longitude, latitude) in [
            (3663832614298053.0, 2.29447156190872, 48.85846255040141),
            (3876464048901851.0, 49.12499874830245, 72.99100027813946),
            (3468915414364476.0, 10.08720070123672, 34.50260034107078),
            (3781709020344510.0, 41.12499922513961, 73.99100100464303),
        ] {
            let point = decode(score);

            assert!(
                (point.longitude - longitude).abs() < CLOSE_ENOUGH,
                "{score}: {} is not {longitude}",
                point.longitude
            );
            assert!(
                (point.latitude - latitude).abs() < CLOSE_ENOUGH,
                "{score}: {} is not {latitude}",
                point.latitude
            );
        }
    }

    #[test]
    fn reads_a_place_back_out_to_where_it_went_in() {
        for (longitude, latitude) in [
            (2.2944692, 48.8584625),
            (-0.1277583, 51.5073509),
            (139.6917, 35.6895),
            (-74.0060, 40.7128),
            (151.2093, -33.8688),
            (0.0, 0.0),
            (-180.0, -85.05112878),
        ] {
            let point = Point {
                longitude,
                latitude,
            };
            let read_back = decode(score(point));

            assert!(
                (read_back.longitude - longitude).abs() <= within_a_square(&LONGITUDES),
                "{longitude},{latitude}: {} is not {longitude}",
                read_back.longitude
            );
            assert!(
                (read_back.latitude - latitude).abs() <= within_a_square(&LATITUDES),
                "{longitude},{latitude}: {} is not {latitude}",
                read_back.latitude
            );
        }
    }

    #[test]
    fn gathers_up_what_it_spread_out() {
        for value in [0, 1, 2, 3, 42, 1 << 25, (1 << 26) - 1, u32::MAX] {
            assert_eq!(gather(spread(value)), value, "{value}");
        }
    }

    #[test]
    fn gathers_each_of_the_two_numbers_woven_together() {
        // Woven north into the even bits and east into the odd, so each comes
        // back out without a trace of the other.
        let woven = spread(12345) | (spread(54321) << 1);

        assert_eq!(gather(woven), 12345);
        assert_eq!(gather(woven >> 1), 54321);
    }

    #[test]
    fn says_where_each_of_the_places_asked_after_is() {
        let store = Store::default();
        geoadd_to(
            &[
                "places",
                "-0.0884948",
                "51.5006479",
                "London",
                "11.5030378",
                "48.1642721",
                "Munich",
            ],
            &store,
        );

        // Two places asked after, two answers, each a pair of numbers.
        let Value::Array(answers) = geopos(&["places", "London", "Munich"], &store) else {
            panic!("one answer to a place");
        };

        assert_eq!(answers.len(), 2);
        for answer in &answers {
            let Value::Array(pair) = answer else {
                panic!("a place is answered with the two numbers it lies at");
            };

            assert_eq!(pair.len(), 2);
        }
    }

    #[test]
    fn says_a_place_is_where_it_was_put() {
        let store = Store::default();
        geoadd_to(&["places", "2.2944692", "48.8584625", "Paris"], &store);

        // What comes back is the middle of the square the place fell in, near
        // enough to where it went in that a client could not tell them apart on
        // any map.
        assert_eq!(
            geopos(&["places", "Paris"], &store).encode(),
            b"*1\r\n*2\r\n$19\r\n2.29447156190872192\r\n$20\r\n48.85846255040141273\r\n"
        );
    }

    #[test]
    fn says_nothing_of_a_place_the_key_does_not_hold() {
        let store = Store::default();
        geoadd_to(&["places", "-0.0884948", "51.5006479", "London"], &store);

        assert_eq!(
            geopos(&["places", "nowhere"], &store).encode(),
            b"*1\r\n*-1\r\n"
        );
    }

    #[test]
    fn answers_for_every_place_asked_after_of_a_key_that_is_not_there() {
        let store = Store::default();

        // One answer to a place either way, so that what comes back still lines
        // up with what was asked.
        assert_eq!(
            geopos(&["nothing", "London", "Munich"], &store).encode(),
            b"*2\r\n*-1\r\n*-1\r\n"
        );
    }

    fn geodist(words: &[&str], store: &Store) -> Value {
        let args: Vec<Bytes> = words.iter().copied().map(named).collect();

        run("GEODIST", &args, store).expect("geodist belongs to this module")
    }

    /// The two places the tester measures between.
    fn munich_and_paris(store: &Store) {
        geoadd_to(
            &[
                "places",
                "11.5030378",
                "48.164271",
                "Munich",
                "2.2944692",
                "48.8584625",
                "Paris",
            ],
            store,
        );
    }

    #[test]
    fn says_how_far_apart_two_places_are() {
        let store = Store::default();
        munich_and_paris(&store);

        assert_eq!(
            geodist(&["places", "Munich", "Paris"], &store),
            Value::BulkString(named("682477.7582"))
        );
    }

    #[test]
    fn measures_the_same_distance_whichever_way_round_it_is_asked() {
        let store = Store::default();
        munich_and_paris(&store);

        assert_eq!(
            geodist(&["places", "Paris", "Munich"], &store),
            geodist(&["places", "Munich", "Paris"], &store)
        );
    }

    #[test]
    fn measures_no_distance_at_all_from_a_place_to_itself() {
        let store = Store::default();
        munich_and_paris(&store);

        assert_eq!(
            geodist(&["places", "Munich", "Munich"], &store),
            Value::BulkString(named("0.0000"))
        );
    }

    #[test]
    fn measures_in_the_unit_it_was_asked_for() {
        let store = Store::default();
        munich_and_paris(&store);

        for (unit, expected) in [
            ("m", "682477.7582"),
            ("M", "682477.7582"),
            ("km", "682.4778"),
            ("mi", "424.0731"),
            ("ft", "2239100.2564"),
        ] {
            assert_eq!(
                geodist(&["places", "Munich", "Paris", unit], &store),
                Value::BulkString(named(expected)),
                "{unit}"
            );
        }
    }

    #[test]
    fn refuses_a_unit_it_cannot_measure_in() {
        let store = Store::default();
        munich_and_paris(&store);

        assert_eq!(
            geodist(&["places", "Munich", "Paris", "leagues"], &store),
            unsupported_unit()
        );
    }

    #[test]
    fn says_nothing_of_the_distance_to_a_place_that_is_not_there() {
        let store = Store::default();
        munich_and_paris(&store);

        assert_eq!(
            geodist(&["places", "Munich", "nowhere"], &store),
            Value::Null
        );
        assert_eq!(
            geodist(&["places", "nowhere", "Paris"], &store),
            Value::Null
        );
        assert_eq!(
            geodist(&["nothing", "Munich", "Paris"], &store),
            Value::Null
        );
    }

    #[test]
    fn refuses_a_geodist_that_names_fewer_than_two_places() {
        let store = Store::default();

        for command in [
            vec![],
            vec!["places"],
            vec!["places", "Munich"],
            vec!["places", "Munich", "Paris", "km", "extra"],
        ] {
            assert_eq!(
                geodist(&command, &store),
                wrong_arity("geodist"),
                "{command:?}"
            );
        }
    }

    #[test]
    fn will_not_measure_between_places_of_a_key_holding_something_else() {
        let store = Store::default();

        store.set(named("places"), named("a string"), None);

        assert_eq!(
            geodist(&["places", "Munich", "Paris"], &store),
            wrong_type()
        );
    }

    #[test]
    fn refuses_a_geopos_that_names_no_place() {
        let store = Store::default();

        assert_eq!(geopos(&[], &store), wrong_arity("geopos"));
        assert_eq!(geopos(&["places"], &store), wrong_arity("geopos"));
    }

    #[test]
    fn will_not_place_a_member_of_a_key_holding_something_else() {
        let store = Store::default();

        store.set(named("places"), named("a string"), None);

        assert_eq!(geopos(&["places", "London"], &store), wrong_type());
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
