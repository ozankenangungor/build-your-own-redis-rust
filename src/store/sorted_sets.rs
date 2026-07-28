use super::{Data, Entry, Store, WrongType, drop_if_expired, resolve_index};
use bytes::Bytes;
use std::cmp::Ordering;

/// One member of a sorted set: what it is called, and where in the order it
/// falls.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub score: f64,
    pub name: Bytes,
}

/// The members of a sorted set, kept in the order Redis keeps them.
///
/// A `Vec` held in order rather than a skip list. Finding a member by where it
/// falls, or reading off a run of them, is what a sorted set is asked for most,
/// and both are as quick here as anywhere. Adding one costs a shift of what
/// comes after it, which for the sets this server holds is no great matter.
#[derive(Default)]
pub struct SortedSet(Vec<Member>);

impl SortedSet {
    /// Puts a member in its place, and says whether it is one this set had
    /// never held. A member that was already here is moved rather than added,
    /// since a name appears in a sorted set once and once only.
    fn add(&mut self, score: f64, name: &Bytes) -> bool {
        let known = self.take(name);

        let member = Member {
            score,
            name: name.clone(),
        };
        let at = self.0.partition_point(|other| precedes(other, &member));
        self.0.insert(at, member);

        !known
    }

    /// Lifts a member out by name, if it is here at all, and says whether it
    /// was. Scores are no help in the finding: it is the new score that is
    /// being put in place of the old.
    fn take(&mut self, name: &Bytes) -> bool {
        match self.at(name) {
            Some(at) => {
                self.0.remove(at);
                true
            }
            None => false,
        }
    }

    /// Where a member falls in the order, counting from the first.
    ///
    /// Looked for by name rather than by score, since the name is all the asker
    /// has. What it gets back is a place in an order the scores decided.
    fn at(&self, name: &Bytes) -> Option<usize> {
        self.0.iter().position(|member| member.name == name)
    }
}

/// Whether one member comes before another: by score, and by name where the
/// scores are equal.
///
/// Two scores always compare one way or the other here, since a member is never
/// admitted with a score that is not a number.
fn precedes(member: &Member, other: &Member) -> bool {
    match member.score.partial_cmp(&other.score) {
        Some(Ordering::Less) => true,
        Some(Ordering::Equal) => member.name < other.name,
        _ => false,
    }
}

impl Store {
    /// Puts these members into the sorted set at `key`, making one if there is
    /// none, and says how many of them the set had never held.
    pub fn zadd(&self, key: &Bytes, members: &[(f64, Bytes)]) -> Result<usize, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let version = state.next_version();
        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| Entry::new(Data::SortedSet(SortedSet::default()), version));

        let Data::SortedSet(set) = &mut entry.data else {
            return Err(WrongType);
        };

        let mut added = 0;
        for (score, name) in members {
            if set.add(*score, name) {
                added += 1;
            }
        }

        entry.version = version;

        Ok(added)
    }

    /// Where a member falls in the sorted set at `key`, counting from the first.
    ///
    /// `None` covers both a member the set does not hold and a key holding no
    /// set at all: neither has a place in an order, and Redis makes no
    /// distinction between them here.
    pub fn zrank(&self, key: &Bytes, member: &Bytes) -> Result<Option<usize>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        match state.entries.get(key) {
            None => Ok(None),
            Some(Entry {
                data: Data::SortedSet(set),
                ..
            }) => Ok(set.at(member)),
            Some(_) => Err(WrongType),
        }
    }

    /// The members of the sorted set at `key` between `start` and `stop`, both
    /// inclusive and counted from the first.
    ///
    /// A window falling outside the set is no error: it is drawn in, and yields
    /// fewer members or none at all. A key holding no set yields none, as an
    /// empty set would.
    pub fn zrange(&self, key: &Bytes, start: i64, stop: i64) -> Result<Vec<Bytes>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let set = match state.entries.get(key) {
            None => return Ok(Vec::new()),
            Some(Entry {
                data: Data::SortedSet(set),
                ..
            }) => set,
            Some(_) => return Err(WrongType),
        };

        let start = resolve_index(start, set.0.len());
        let stop = resolve_index(stop, set.0.len());

        if start > stop || start >= set.0.len() {
            return Ok(Vec::new());
        }

        let stop = stop.min(set.0.len() - 1);
        Ok(set.0[start..=stop]
            .iter()
            .map(|member| member.name.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Bytes {
        Bytes::copy_from_slice(name.as_bytes())
    }

    /// The set's members in the order it holds them.
    fn in_order(set: &SortedSet) -> Vec<(f64, &str)> {
        set.0
            .iter()
            .map(|member| {
                (
                    member.score,
                    str::from_utf8(&member.name).expect("a name a test gave it"),
                )
            })
            .collect()
    }

    #[test]
    fn holds_the_one_member_it_is_given() {
        let mut set = SortedSet::default();

        assert!(set.add(8.0, &named("Sam")));
        assert_eq!(in_order(&set), [(8.0, "Sam")]);
    }

    #[test]
    fn holds_its_members_in_the_order_of_their_scores() {
        let mut set = SortedSet::default();

        for (score, name) in [(14.5, "Prickett"), (6.1, "Ford"), (8.2, "Royce")] {
            assert!(set.add(score, &named(name)));
        }

        assert_eq!(
            in_order(&set),
            [(6.1, "Ford"), (8.2, "Royce"), (14.5, "Prickett")]
        );
    }

    #[test]
    fn settles_a_tie_on_score_by_the_name() {
        let mut set = SortedSet::default();

        for name in ["Sam-Bodden", "Royce", "Castilla"] {
            set.add(8.2, &named(name));
        }

        assert_eq!(
            in_order(&set),
            [(8.2, "Castilla"), (8.2, "Royce"), (8.2, "Sam-Bodden")]
        );
    }

    #[test]
    fn holds_a_name_once_however_often_it_is_added() {
        let mut set = SortedSet::default();

        assert!(set.add(1.0, &named("Sam")));
        assert!(!set.add(1.0, &named("Sam")));
        assert_eq!(in_order(&set), [(1.0, "Sam")]);
    }

    #[test]
    fn moves_a_member_whose_score_has_changed() {
        let mut set = SortedSet::default();

        set.add(1.0, &named("first"));
        set.add(2.0, &named("second"));
        set.add(3.0, &named("third"));

        assert!(!set.add(9.0, &named("first")));
        assert_eq!(
            in_order(&set),
            [(2.0, "second"), (3.0, "third"), (9.0, "first")]
        );
    }

    #[test]
    fn holds_the_members_whose_scores_are_beyond_counting() {
        let mut set = SortedSet::default();

        set.add(0.0, &named("nought"));
        set.add(f64::INFINITY, &named("most"));
        set.add(f64::NEG_INFINITY, &named("least"));

        assert_eq!(
            in_order(&set),
            [
                (f64::NEG_INFINITY, "least"),
                (0.0, "nought"),
                (f64::INFINITY, "most"),
            ]
        );
    }

    #[test]
    fn makes_a_set_for_a_key_that_had_none() {
        let store = Store::default();

        assert_eq!(store.zadd(&named("racers"), &[(8.0, named("Sam"))]), Ok(1));
    }

    #[test]
    fn adds_to_the_set_a_key_already_holds() {
        let store = Store::default();
        let key = named("racers");

        store.zadd(&key, &[(8.0, named("Sam"))]).unwrap();

        assert_eq!(store.zadd(&key, &[(6.1, named("Ford"))]), Ok(1));
    }

    #[test]
    fn counts_only_the_members_the_set_had_never_held() {
        let store = Store::default();
        let key = named("racers");

        store.zadd(&key, &[(8.0, named("Sam"))]).unwrap();

        // One already here at another score, one never seen before.
        let members = [(9.0, named("Sam")), (6.1, named("Ford"))];
        assert_eq!(store.zadd(&key, &members), Ok(1));
    }

    #[test]
    fn counts_the_members_added_in_one_go() {
        let store = Store::default();
        let members = [(1.0, named("a")), (2.0, named("b")), (3.0, named("c"))];

        assert_eq!(store.zadd(&named("racers"), &members), Ok(3));
    }

    #[test]
    fn will_not_add_to_a_key_holding_something_else() {
        let store = Store::default();
        let key = named("racers");

        store.set(key.clone(), named("a string"), None);

        assert_eq!(store.zadd(&key, &[(8.0, named("Sam"))]), Err(WrongType));
    }

    #[test]
    fn says_where_each_member_falls_in_the_order() {
        let store = Store::default();
        let key = named("racers");
        let members = [
            (14.5, named("Prickett")),
            (6.1, named("Ford")),
            (8.2, named("Royce")),
        ];

        store.zadd(&key, &members).unwrap();

        assert_eq!(store.zrank(&key, &named("Ford")), Ok(Some(0)));
        assert_eq!(store.zrank(&key, &named("Royce")), Ok(Some(1)));
        assert_eq!(store.zrank(&key, &named("Prickett")), Ok(Some(2)));
    }

    #[test]
    fn says_where_a_member_falls_once_its_score_has_moved() {
        let store = Store::default();
        let key = named("racers");

        store
            .zadd(&key, &[(1.0, named("first")), (2.0, named("second"))])
            .unwrap();
        store.zadd(&key, &[(9.0, named("first"))]).unwrap();

        assert_eq!(store.zrank(&key, &named("second")), Ok(Some(0)));
        assert_eq!(store.zrank(&key, &named("first")), Ok(Some(1)));
    }

    #[test]
    fn says_nothing_of_a_member_the_set_does_not_hold() {
        let store = Store::default();
        let key = named("racers");

        store.zadd(&key, &[(8.0, named("Sam"))]).unwrap();

        assert_eq!(store.zrank(&key, &named("nobody")), Ok(None));
    }

    #[test]
    fn says_nothing_of_a_member_of_a_set_that_is_not_there() {
        let store = Store::default();

        assert_eq!(store.zrank(&named("nothing"), &named("Sam")), Ok(None));
    }

    /// A set holding four members, in the order they come out in.
    fn racers(store: &Store) -> Bytes {
        let key = named("racers");
        let members = [
            (8.1, named("Sam-Bodden")),
            (10.2, named("Royce")),
            (6.0, named("Ford")),
            (14.1, named("Prickett")),
        ];

        store.zadd(&key, &members).unwrap();

        key
    }

    /// The names `zrange` hands back, as text.
    fn listed(members: Result<Vec<Bytes>, WrongType>) -> Vec<String> {
        members
            .expect("a set the test made")
            .iter()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect()
    }

    #[test]
    fn lists_the_members_between_two_places_in_the_order() {
        let store = Store::default();
        let key = racers(&store);

        assert_eq!(
            listed(store.zrange(&key, 0, 2)),
            ["Ford", "Sam-Bodden", "Royce"]
        );
    }

    #[test]
    fn lists_the_one_member_a_window_of_one_holds() {
        let store = Store::default();
        let key = racers(&store);

        assert_eq!(listed(store.zrange(&key, 1, 1)), ["Sam-Bodden"]);
    }

    #[test]
    fn lists_as_far_as_the_set_goes_when_the_window_reaches_past_it() {
        let store = Store::default();
        let key = racers(&store);

        assert_eq!(
            listed(store.zrange(&key, 2, 99)),
            ["Royce", "Prickett"],
            "the far end is drawn in to the last member"
        );
    }

    #[test]
    fn lists_nothing_when_the_window_starts_past_the_set() {
        let store = Store::default();
        let key = racers(&store);

        assert!(listed(store.zrange(&key, 4, 9)).is_empty());
        assert!(listed(store.zrange(&key, 99, 99)).is_empty());
    }

    #[test]
    fn lists_nothing_when_the_window_ends_before_it_starts() {
        let store = Store::default();
        let key = racers(&store);

        assert!(listed(store.zrange(&key, 2, 1)).is_empty());
    }

    #[test]
    fn lists_nothing_of_a_set_that_is_not_there() {
        let store = Store::default();

        assert!(listed(store.zrange(&named("nothing"), 0, 9)).is_empty());
    }

    #[test]
    fn lists_the_members_in_the_order_their_scores_put_them() {
        let store = Store::default();
        let key = racers(&store);

        // Added in one order, listed in another: the scores decide, not the
        // asking.
        assert_eq!(
            listed(store.zrange(&key, 0, 9)),
            ["Ford", "Sam-Bodden", "Royce", "Prickett"]
        );
    }

    #[test]
    fn will_not_list_the_members_of_a_key_holding_something_else() {
        let store = Store::default();
        let key = named("racers");

        store.set(key.clone(), named("a string"), None);

        assert_eq!(store.zrange(&key, 0, 9), Err(WrongType));
    }

    #[test]
    fn will_not_place_a_member_of_a_key_holding_something_else() {
        let store = Store::default();
        let key = named("racers");

        store.set(key.clone(), named("a string"), None);

        assert_eq!(store.zrank(&key, &named("Sam")), Err(WrongType));
    }
}
