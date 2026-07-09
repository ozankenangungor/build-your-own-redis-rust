//! Matching keys against the patterns Redis picks them out by.

/// A pattern of the glob sort, ready to be tried against one key after another.
///
/// Reading the pattern is done once, up front, since a single `KEYS` tries it
/// against every key the server holds.
pub struct Pattern(Vec<Item>);

/// One step of a pattern, standing for one byte of a key or for any run of them.
enum Item {
    /// `*`: any run of bytes, however long, including none at all.
    Anything,
    /// `?`: any one byte.
    One,
    /// A byte standing for itself, which `\` can make of any of the others.
    Byte(u8),
    /// `[...]`: any one of the bytes named inside, or any byte but those.
    OneOf {
        ranges: Vec<(u8, u8)>,
        negated: bool,
    },
}

impl Pattern {
    /// Reads a pattern. Anything that cannot be made sense of stands for
    /// itself, the way Redis takes it: a pattern is never wrong, only unmet.
    pub fn parse(pattern: &[u8]) -> Self {
        let mut items: Vec<Item> = Vec::new();
        let mut at = 0;

        while let Some(&byte) = pattern.get(at) {
            at += 1;

            items.push(match byte {
                // Several runs in a row say no more than one does, and each is
                // somewhere a match may have to be tried again from.
                b'*' if matches!(items.last(), Some(Item::Anything)) => continue,
                b'*' => Item::Anything,
                b'?' => Item::One,
                b'[' => {
                    let (item, read) = set(&pattern[at..]);
                    at += read;
                    item
                }
                b'\\' => match pattern.get(at) {
                    Some(&escaped) => {
                        at += 1;
                        Item::Byte(escaped)
                    }
                    // With nothing left to stand for, it stands for itself.
                    None => Item::Byte(b'\\'),
                },
                byte => Item::Byte(byte),
            });
        }

        Self(items)
    }

    /// Whether `key` is one of those this pattern asks for.
    ///
    /// A run is given as little as it can be at first, and handed one more byte
    /// each time what follows it fails to match. Keeping only the last run to
    /// fall back on is what keeps this from trying every way of splitting the
    /// key at once.
    pub fn matches(&self, key: &[u8]) -> bool {
        let mut item = 0;
        let mut byte = 0;
        // The last run met, and how much of the key it had taken by then.
        let mut run: Option<(usize, usize)> = None;

        while byte < key.len() {
            match self.0.get(item) {
                Some(Item::Anything) => {
                    run = Some((item, byte));
                    item += 1;
                }
                Some(step) if step.matches(key[byte]) => {
                    item += 1;
                    byte += 1;
                }
                // Either the pattern ran out or this byte is not what it asked
                // for. Either way the last run was too short.
                _ => match run {
                    Some((at, taken)) => {
                        item = at + 1;
                        byte = taken + 1;
                        run = Some((at, taken + 1));
                    }
                    None => return false,
                },
            }
        }

        // The key is spent, so what is left of the pattern must be able to
        // stand for nothing at all.
        self.0[item..]
            .iter()
            .all(|item| matches!(item, Item::Anything))
    }
}

impl Item {
    fn matches(&self, byte: u8) -> bool {
        match self {
            Item::Anything | Item::One => true,
            Item::Byte(expected) => *expected == byte,
            Item::OneOf { ranges, negated } => {
                let named = ranges
                    .iter()
                    .any(|&(first, last)| (first..=last).contains(&byte));

                named != *negated
            }
        }
    }
}

/// Reads what a `[` opened, up to the `]` that closes it, and says how many
/// bytes it took up. An opening never closed runs to the end of the pattern.
fn set(pattern: &[u8]) -> (Item, usize) {
    let negated = pattern.first() == Some(&b'^');
    let mut at = usize::from(negated);
    let mut ranges = Vec::new();

    while let Some(&byte) = pattern.get(at) {
        at += 1;

        if byte == b']' {
            break;
        }

        let first = match (byte, pattern.get(at)) {
            (b'\\', Some(&escaped)) => {
                at += 1;
                escaped
            }
            _ => byte,
        };

        // A dash between two bytes names every byte from one to the other. One
        // with nothing after it is a byte like any other.
        let last = match (pattern.get(at), pattern.get(at + 1)) {
            (Some(b'-'), Some(&end)) if end != b']' => {
                at += 2;
                end
            }
            _ => first,
        };

        ranges.push((first.min(last), first.max(last)));
    }

    (Item::OneOf { ranges, negated }, at)
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn matches(pattern: &str, key: &str) -> bool {
        Pattern::parse(pattern.as_bytes()).matches(key.as_bytes())
    }

    #[test]
    fn a_run_stands_for_every_key_there_is() {
        for key in ["", "foo", "a much longer key"] {
            assert!(matches("*", key), "{key:?}");
        }
    }

    #[test]
    fn a_pattern_of_bytes_stands_for_itself_alone() {
        assert!(matches("foo", "foo"));
        assert!(!matches("foo", "bar"));
        assert!(!matches("foo", "foobar"));
        assert!(!matches("foo", "fo"));
    }

    #[test]
    fn a_run_stands_for_nothing_at_all_as_readily_as_for_something() {
        assert!(matches("foo*", "foo"));
        assert!(matches("*foo", "foo"));
        assert!(matches("*foo*", "foo"));
        assert!(matches("f*o", "fo"));
    }

    #[test]
    fn a_run_reaches_as_far_as_it_has_to() {
        assert!(matches("foo*", "foobar"));
        assert!(matches("*bar", "foobar"));
        assert!(matches("f*r", "foobar"));
        assert!(matches("*oob*", "foobar"));
        assert!(!matches("foo*", "barfoo"));
    }

    #[test]
    fn several_runs_pick_out_the_pieces_between_them() {
        assert!(matches("*a*b*c*", "xxaxxbxxcxx"));
        assert!(!matches("*a*b*c*", "xxaxxcxxbxx"));
    }

    #[test]
    fn a_run_backs_up_when_what_follows_it_falls_short() {
        // The first `ab` is a false start: only the second is followed by a `c`.
        assert!(matches("*abc", "abxabc"));
        assert!(matches("*ab*", "xxxab"));
    }

    #[test]
    fn several_runs_in_a_row_say_no_more_than_one_does() {
        assert!(matches("**", "foo"));
        assert!(matches("f***o", "foo"));
        assert!(!matches("f***o", "bar"));
    }

    #[test]
    fn a_question_mark_stands_for_one_byte_and_no_fewer() {
        assert!(matches("f?o", "foo"));
        assert!(matches("???", "foo"));
        assert!(!matches("f?o", "fo"));
        assert!(!matches("???", "foobar"));
    }

    #[test]
    fn a_set_stands_for_any_one_of_the_bytes_in_it() {
        assert!(matches("f[ao]o", "foo"));
        assert!(matches("f[ao]o", "fao"));
        assert!(!matches("f[ao]o", "fio"));
    }

    #[test]
    fn a_set_can_name_a_stretch_of_bytes_at_once() {
        assert!(matches("key:[0-9]", "key:7"));
        assert!(!matches("key:[0-9]", "key:x"));
        assert!(matches("[a-c][a-c]", "ba"));
        assert!(!matches("[a-c][a-c]", "bd"));
    }

    #[test]
    fn a_set_can_be_turned_on_its_head() {
        assert!(matches("f[^a]o", "foo"));
        assert!(!matches("f[^a]o", "fao"));
        assert!(matches("[^0-9]", "a"));
        assert!(!matches("[^0-9]", "5"));
    }

    #[test]
    fn a_dash_at_the_end_of_a_set_is_a_byte_like_any_other() {
        assert!(matches("[a-]", "-"));
        assert!(matches("[a-]", "a"));
        assert!(!matches("[a-]", "b"));
    }

    #[test]
    fn a_backslash_makes_a_byte_of_what_would_otherwise_stand_for_more() {
        assert!(matches("f\\*o", "f*o"));
        assert!(!matches("f\\*o", "foo"));
        assert!(matches("\\?", "?"));
        assert!(!matches("\\?", "x"));
        assert!(matches("\\[a]", "[a]"));
    }

    #[test]
    fn a_pattern_of_nothing_stands_for_a_key_of_nothing() {
        assert!(matches("", ""));
        assert!(!matches("", "foo"));
    }

    #[test]
    fn an_opening_never_closed_runs_to_the_end_of_the_pattern() {
        assert!(matches("[abc", "a"));
        assert!(!matches("[abc", "d"));
    }

    #[test]
    fn keeps_its_footing_against_a_key_built_to_make_it_stumble() {
        // Every one of the runs could be tried at every point of the key. Tried
        // one way at a time, this would take longer than the world has left.
        let pattern = "*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let key = "a".repeat(2000);

        assert!(!matches(pattern, &key));
    }
}
