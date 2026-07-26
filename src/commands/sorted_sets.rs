use super::{text, wrong_arity, wrong_type};
use crate::resp::Value;
use crate::store::{Store, WrongType};
use bytes::Bytes;

/// Handles the commands that work on sorted sets: members held in the order of
/// the scores given to them. `None` means the command belongs to another module.
pub fn run(command: &str, args: &[Bytes], store: &Store) -> Option<Value> {
    let reply = match command {
        "ZADD" => match args {
            [key, scored @ ..] if !scored.is_empty() => add(store, key, scored),
            _ => wrong_arity("zadd"),
        },
        _ => return None,
    };

    Some(reply)
}

/// Adds the members named to the sorted set at `key`, answering with how many
/// of them it had never held.
///
/// A member updated in place is not one added, however much its score moved:
/// what is counted is names the set is holding that it was not.
fn add(store: &Store, key: &Bytes, scored: &[Bytes]) -> Value {
    let members = match members(scored) {
        Ok(members) => members,
        Err(error) => return error,
    };

    match store.zadd(key, &members) {
        Ok(added) => Value::Integer(added as i64),
        Err(WrongType) => wrong_type(),
    }
}

/// Reads the scores and members that follow the key, each score before the
/// member it belongs to.
///
/// Nothing is added until every pair has been read. A command half carried out
/// is worse than one refused, and Redis refuses this one whole.
fn members(scored: &[Bytes]) -> Result<Vec<(f64, Bytes)>, Value> {
    if !scored.len().is_multiple_of(2) {
        return Err(syntax_error());
    }

    scored
        .chunks_exact(2)
        .map(|pair| Ok((score(&pair[0])?, pair[1].clone())))
        .collect()
}

/// Reads a score as Redis reads one: any number a double can hold, including
/// the infinities, which stand for the ends of the order.
///
/// A score that is not a number at all is refused, and so is one that is `nan`:
/// it would compare with nothing, and a member that falls nowhere has no place
/// in an order.
fn score(score: &Bytes) -> Result<f64, Value> {
    text(score)
        .and_then(|score| score.parse::<f64>().ok())
        .filter(|score| !score.is_nan())
        .ok_or_else(not_a_float)
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

    fn scored(words: &[&str]) -> Vec<Bytes> {
        words
            .iter()
            .map(|word| Bytes::copy_from_slice(word.as_bytes()))
            .collect()
    }

    fn named(name: &str) -> Bytes {
        Bytes::copy_from_slice(name.as_bytes())
    }

    fn zadd(words: &[&str], store: &Store) -> Value {
        run("ZADD", &scored(words), store).expect("zadd belongs to this module")
    }

    #[test]
    fn counts_the_member_it_adds() {
        let store = Store::default();

        assert_eq!(zadd(&["racers", "8.0", "Sam"], &store), Value::Integer(1));
    }

    #[test]
    fn counts_nothing_for_a_member_the_set_already_held() {
        let store = Store::default();

        zadd(&["racers", "8.0", "Sam"], &store);

        assert_eq!(zadd(&["racers", "9.0", "Sam"], &store), Value::Integer(0));
    }

    #[test]
    fn counts_each_of_the_members_it_adds_in_one_go() {
        let store = Store::default();
        let command = ["racers", "6.1", "Ford", "8.2", "Royce"];

        assert_eq!(zadd(&command, &store), Value::Integer(2));
    }

    #[test]
    fn reads_the_scores_redis_reads() {
        for (spelled, meant) in [
            ("8", 8.0),
            ("8.0", 8.0),
            ("-1.5", -1.5),
            ("0", 0.0),
            ("1e3", 1000.0),
            ("inf", f64::INFINITY),
            ("+inf", f64::INFINITY),
            ("-inf", f64::NEG_INFINITY),
            ("infinity", f64::INFINITY),
        ] {
            assert_eq!(score(&named(spelled)), Ok(meant), "{spelled}");
        }
    }

    #[test]
    fn turns_down_a_score_that_is_not_a_number() {
        // `nan` compares with nothing, so a member carrying one would fall
        // nowhere in an order that is the whole point of the thing.
        for spelled in ["", "eight", "8.0.0", "8 ", " 8", "nan", "-nan"] {
            assert_eq!(score(&named(spelled)), Err(not_a_float()), "{spelled:?}");
        }

        assert_eq!(score(&Bytes::from_static(b"\xff")), Err(not_a_float()));
    }

    #[test]
    fn refuses_a_zadd_that_is_missing_a_score_or_a_member() {
        let store = Store::default();

        for command in [vec!["racers"], vec!["racers", "8.0"]] {
            let reply = zadd(&command, &store);
            let expected = match command.len() {
                1 => wrong_arity("zadd"),
                _ => syntax_error(),
            };

            assert_eq!(reply, expected, "{command:?}");
        }

        assert_eq!(
            run("ZADD", &[], &Store::default()),
            Some(wrong_arity("zadd"))
        );
    }

    #[test]
    fn adds_nothing_at_all_when_one_of_the_scores_will_not_read() {
        let store = Store::default();

        assert_eq!(
            zadd(&["racers", "8.0", "Sam", "later", "Ford"], &store),
            not_a_float()
        );

        // The pair that could be read went nowhere either: the command was
        // refused whole rather than carried half way out.
        assert_eq!(store.zadd(&named("racers"), &[]), Ok(0));
        assert_eq!(zadd(&["racers", "8.0", "Sam"], &store), Value::Integer(1));
    }

    #[test]
    fn will_not_add_to_a_key_holding_something_else() {
        let store = Store::default();

        store.set(named("racers"), named("a string"), None);

        assert_eq!(zadd(&["racers", "8.0", "Sam"], &store), wrong_type());
    }

    #[test]
    fn leaves_alone_the_commands_that_are_not_its_own() {
        assert!(run("GET", &[], &Store::default()).is_none());
    }
}
