use super::{Data, Entry, Store, WrongType, drop_if_expired};
use bytes::Bytes;
use std::time::{Duration, Instant};

impl Store {
    pub fn set(&self, key: Bytes, value: Bytes, expires_in: Option<Duration>) {
        let mut state = self.state();
        let entry = Entry {
            data: Data::String(value),
            expires_at: expires_in.map(|delay| Instant::now() + delay),
            version: state.next_version(),
        };
        state.entries.insert(key, entry);
    }

    pub fn get(&self, key: &Bytes) -> Result<Option<Bytes>, WrongType> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        match state.entries.get(key) {
            None => Ok(None),
            Some(Entry {
                data: Data::String(value),
                ..
            }) => Ok(Some(value.clone())),
            Some(_) => Err(WrongType),
        }
    }

    /// Adds one to the number held at `key`, storing and returning the result.
    /// A key that is not there counts as zero, so counting up from nothing
    /// leaves a one behind.
    pub fn increment(&self, key: &Bytes) -> Result<i64, IncrementError> {
        let mut state = self.state();
        drop_if_expired(&mut state.entries, key);

        let version = state.next_version();
        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| Entry::new(Data::String(Bytes::from_static(b"0")), version));

        let Data::String(value) = &mut entry.data else {
            return Err(IncrementError::WrongType);
        };

        let number = as_number(value).ok_or(IncrementError::NotAnInteger)?;
        let incremented = number.checked_add(1).ok_or(IncrementError::Overflow)?;

        // Writing through the entry leaves whatever expiry it carries in place,
        // since counting up is not the same as setting the key anew.
        *value = Bytes::from(incremented.to_string());
        entry.version = version;

        Ok(incremented)
    }
}

/// Why an increment could not go through.
pub enum IncrementError {
    NotAnInteger,
    Overflow,
    WrongType,
}

/// Reads a stored value as a number.
///
/// Redis only counts a value as one when it is spelled the single way it would
/// print it back, which is stricter than Rust's own parser: `+1`, `007` and
/// `-0` are all values that happen to look numeric, not numbers.
fn as_number(value: &[u8]) -> Option<i64> {
    let text = str::from_utf8(value).ok()?;
    let number: i64 = text.parse().ok()?;

    (number.to_string() == text).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::as_number;

    #[test]
    fn reads_the_numbers_redis_would_read() {
        for value in ["0", "1", "42", "-1", "9223372036854775807"] {
            assert_eq!(as_number(value.as_bytes()), value.parse().ok());
        }
    }

    #[test]
    fn turns_down_values_that_only_look_numeric() {
        for value in [
            "",
            "xyz",
            "+1",
            "007",
            "-0",
            " 1",
            "1 ",
            "1.0",
            "1e3",
            "0x1",
            // One past the largest number, and bytes that are not text at all.
            "9223372036854775808",
            "\u{fffd}",
        ] {
            assert_eq!(as_number(value.as_bytes()), None, "{value:?}");
        }

        assert_eq!(as_number(b"\xff"), None);
    }
}
