//! Reading the file Redis keeps a dataset in between runs.
//!
//! A file opens with a magic word and the version it was written in, and then
//! runs through a series of sections: a few facts about the server that wrote
//! it, the databases themselves, and a marker for the end. Every section names
//! itself with a leading byte, so one that is not understood can be told apart
//! from one that is not there.

use crate::config::Config;
use crate::store::Store;
use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// What every file opens with, followed by four digits naming the version.
const MAGIC: &[u8] = b"REDIS";
const HEADER: usize = MAGIC.len() + 4;

/// A fact about the server that wrote the file, as a name and a value.
const AUXILIARY: u8 = 0xFA;
/// How many keys the database that follows holds, and how many of them expire.
const RESIZE: u8 = 0xFB;
/// When the key that follows is to go, in milliseconds since the epoch.
const EXPIRES_AT_MILLISECONDS: u8 = 0xFC;
/// The same, in whole seconds.
const EXPIRES_AT_SECONDS: u8 = 0xFD;
/// The database the keys that follow belong to.
const DATABASE: u8 = 0xFE;
/// The end of the file, followed by a checksum of everything before it.
const END: u8 = 0xFF;

/// A key holding a string. The other kinds are the packed layouts lists,
/// hashes and sets are saved in, which this server does not read yet.
const STRING: u8 = 0x00;

/// The shorter ways a string can be stored in place of its own bytes: as a
/// number of one of three widths, or squeezed down.
const AS_I8: u8 = 0;
const AS_I16: u8 = 1;
const AS_I32: u8 = 2;
const SQUEEZED: u8 = 3;

/// One key as it was saved, with what it held and how long it was to be held
/// for.
pub struct Record {
    pub key: Bytes,
    pub value: Bytes,
    /// When the key was to go, for the ones saved with a time on them.
    pub expires_at: Option<SystemTime>,
}

/// Fills the store from the dataset this server was told to keep, and says how
/// many keys it took on.
pub fn load(config: &Config, store: &Store) -> Result<usize> {
    let path = Path::new(&config.dir).join(&config.dbfilename);
    let records = read(&path).with_context(|| format!("reading {}", path.display()))?;

    let now = SystemTime::now();
    let mut loaded = 0;

    for record in records {
        // A key whose time had already passed is one this server never holds:
        // Redis lets it go on the way in rather than load it and drop it.
        let expires_in = match record.expires_at {
            None => None,
            Some(deadline) => match deadline.duration_since(now) {
                Ok(left) => Some(left),
                Err(_) => continue,
            },
        };

        store.set(record.key, record.value, expires_in);
        loaded += 1;
    }

    Ok(loaded)
}

/// Reads the dataset saved at `path`.
///
/// A file that is not there is not a failure: it is a server that has yet to
/// save anything, which is the same as one that saved nothing.
///
/// The whole file is taken in at once rather than read through as it goes,
/// which keeps the reading simple at the cost of holding a large dataset twice
/// over for as long as it takes to load.
pub fn read(path: &Path) -> Result<Vec<Record>> {
    let file = match std::fs::read(path) {
        Ok(file) => Bytes::from(file),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    Reader { file, at: 0 }.dataset()
}

/// Reads a file from the front, keeping its place as it goes.
struct Reader {
    file: Bytes,
    at: usize,
}

/// What the front of a length turned out to say: how long the field is, or
/// that it is stored in one of the shorter ways instead.
enum Length {
    Of(usize),
    Packed(u8),
}

impl Reader {
    /// Reads the whole file, gathering the keys it holds.
    fn dataset(&mut self) -> Result<Vec<Record>> {
        self.header()?;

        let mut records = Vec::new();
        // This server holds a single database, so only the first is taken on.
        // The others are still read through, that being the only way past them.
        let mut database = 0;
        // Set aside by the section before a key, for the keys that expire.
        let mut expires_at = None;

        loop {
            match self.byte()? {
                END => return Ok(records),
                AUXILIARY => {
                    // Nothing said here changes how this server runs, but both
                    // halves have to be read to find where the next section
                    // begins.
                    self.string()?;
                    self.string()?;
                }
                DATABASE => database = self.count()?,
                RESIZE => {
                    // How much room to set aside, which a `HashMap` that grows
                    // as it goes has no use for.
                    self.count()?;
                    self.count()?;
                }
                EXPIRES_AT_SECONDS => {
                    let seconds = u32::from_le_bytes(self.fixed()?);
                    expires_at = Some(UNIX_EPOCH + Duration::from_secs(seconds.into()));
                }
                EXPIRES_AT_MILLISECONDS => {
                    let milliseconds = u64::from_le_bytes(self.fixed()?);
                    expires_at = Some(UNIX_EPOCH + Duration::from_millis(milliseconds));
                }
                STRING => {
                    let record = Record {
                        key: self.string()?,
                        value: self.string()?,
                        expires_at: expires_at.take(),
                    };

                    if database == 0 {
                        records.push(record);
                    }
                }
                kind => bail!("a value of a kind this server cannot read ({kind:#04x})"),
            }
        }
    }

    /// Makes sure this is a dataset at all. The version is read past rather
    /// than checked: what little of the format is read here has not changed
    /// across the versions that write it.
    fn header(&mut self) -> Result<()> {
        let header = self.take(HEADER).context("too short to be a dataset")?;

        ensure!(
            header.starts_with(MAGIC),
            "not a dataset: it opens with {:?}",
            String::from_utf8_lossy(&header),
        );

        Ok(())
    }

    /// Reads a length, which is stored in as few bytes as it fits in. The top
    /// two bits of the first byte say which of the four ways was used.
    fn length(&mut self) -> Result<Length> {
        let first = self.byte()?;

        Ok(match first >> 6 {
            // Six bits, and the length is small enough to be one of them.
            0b00 => Length::Of((first & 0x3f) as usize),
            // Fourteen bits, the low eight of them in the byte that follows.
            0b01 => {
                let rest = self.byte()?;
                Length::Of((((first & 0x3f) as usize) << 8) | rest as usize)
            }
            // A whole number of bytes follows, most significant byte first,
            // which is the one place the format reads that way round.
            0b10 => match first {
                0x80 => Length::Of(u32::from_be_bytes(self.fixed()?) as usize),
                0x81 => Length::Of(usize::try_from(u64::from_be_bytes(self.fixed()?))?),
                _ => bail!("a length stored in a way this server cannot read ({first:#04x})"),
            },
            // Not a length at all: the six bits say how the field was packed.
            _ => Length::Packed(first & 0x3f),
        })
    }

    /// Reads a number stored the way lengths are, for the counts that can only
    /// ever be numbers.
    fn count(&mut self) -> Result<usize> {
        match self.length()? {
            Length::Of(count) => Ok(count),
            Length::Packed(how) => bail!("a count packed as if it were a string ({how})"),
        }
    }

    /// Reads a string: its own bytes, or whatever was stored in their place.
    fn string(&mut self) -> Result<Bytes> {
        match self.length()? {
            Length::Of(length) => self.take(length),
            // A string that spells a number is kept as that number, and is
            // spelled out again on the way back.
            Length::Packed(AS_I8) => Ok(spelled(i8::from_le_bytes(self.fixed()?))),
            Length::Packed(AS_I16) => Ok(spelled(i16::from_le_bytes(self.fixed()?))),
            Length::Packed(AS_I32) => Ok(spelled(i32::from_le_bytes(self.fixed()?))),
            Length::Packed(SQUEEZED) => self.squeezed(),
            Length::Packed(how) => {
                bail!("a string packed in a way this server cannot read ({how})")
            }
        }
    }

    /// Reads a string squeezed down with LZF, which Redis does to the longer
    /// ones. Its length is given twice: as it is stored, then as it will be
    /// once let out again.
    fn squeezed(&mut self) -> Result<Bytes> {
        let stored = self.count().context("the length of a squeezed string")?;
        let unpacked = self.count().context("the length of an unsqueezed string")?;

        let squeezed = self.take(stored)?;

        unsqueeze(&squeezed, unpacked)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    /// Reads a field whose width is known, so that it can be turned into a
    /// number without minding how long it was.
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.take(N)?;

        Ok(bytes
            .as_ref()
            .try_into()
            .expect("exactly as many bytes as were asked for"))
    }

    /// Takes the next `length` bytes, which costs nothing beyond a second hold
    /// on the file they are part of.
    fn take(&mut self, length: usize) -> Result<Bytes> {
        let end = self
            .at
            .checked_add(length)
            .context("a length larger than any file")?;

        ensure!(
            end <= self.file.len(),
            "wanted {length} bytes at {} of a file {} long",
            self.at,
            self.file.len(),
        );

        let taken = self.file.slice(self.at..end);
        self.at = end;

        Ok(taken)
    }
}

/// Lets a string out of the LZF packing Redis squeezed it into.
///
/// What is stored is a run of instructions, each led by a control byte. A small
/// one means the bytes that follow are the string's own; a larger one splits
/// into how much to repeat and how far back to find it, so that a stretch that
/// has already been written can be written again without being stored twice.
fn unsqueeze(squeezed: &[u8], unpacked: usize) -> Result<Bytes> {
    /// The largest control byte that means bytes stored as they are.
    const AS_THEY_ARE: u8 = 0x20;
    /// The count a control byte can hold before the rest spills into a byte of
    /// its own.
    const SPILLS: usize = 7;

    let mut out = Vec::with_capacity(unpacked);
    let mut at = 0;

    while let Some(&control) = squeezed.get(at) {
        at += 1;

        if control < AS_THEY_ARE {
            // A run of the string's own bytes, one longer than the count says
            // since a run of none would be nothing to say at all.
            let run = control as usize + 1;
            let taken = squeezed
                .get(at..at + run)
                .context("a run of bytes reaching past the end")?;

            out.extend_from_slice(taken);
            at += run;

            continue;
        }

        // The top three bits count what is to be repeated, and spill into the
        // next byte once they run out of room.
        let mut length = (control >> 5) as usize;
        if length == SPILLS {
            length += *squeezed.get(at).context("a length reaching past the end")? as usize;
            at += 1;
        }
        // The low five bits are the high bits of how far back to look.
        let low = *squeezed
            .get(at)
            .context("a distance reaching past the end")? as usize;
        let distance = (((control & 0x1f) as usize) << 8) | low;
        at += 1;

        let start = out
            .len()
            .checked_sub(distance + 1)
            .context("a distance reaching back past the start")?;

        // A repeat of one byte would be no shorter than the byte itself, so the
        // count is stored two short. Copying a byte at a time is what lets a
        // repeat reach into what it is itself writing, which is how a long
        // stretch of the same bytes fits into a few.
        for from in start..start + length + 2 {
            out.push(out[from]);
        }
    }

    ensure!(
        out.len() == unpacked,
        "a squeezed string let out to {} bytes rather than {unpacked}",
        out.len(),
    );

    Ok(Bytes::from(out))
}

/// A number as the string it stands for.
fn spelled(number: impl std::fmt::Display) -> Bytes {
    Bytes::from(number.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays out a dataset holding these keys, the way Redis saves one.
    fn saved(entries: &[(&str, &str)]) -> Bytes {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0]);
        file.extend_from_slice(&[RESIZE, entries.len() as u8, 0]);

        for (key, value) in entries {
            file.push(STRING);
            for part in [key, value] {
                file.push(part.len() as u8);
                file.extend_from_slice(part.as_bytes());
            }
        }

        file.push(END);
        file.extend_from_slice(&[0; 8]);

        Bytes::from(file)
    }

    fn records(file: Bytes) -> Result<Vec<Record>> {
        Reader { file, at: 0 }.dataset()
    }

    fn keys(file: Bytes) -> Vec<String> {
        records(file)
            .unwrap()
            .iter()
            .map(|record| String::from_utf8_lossy(&record.key).into_owned())
            .collect()
    }

    #[test]
    fn reads_the_one_key_a_dataset_holds() {
        assert_eq!(keys(saved(&[("foo", "bar")])), ["foo"]);
    }

    #[test]
    fn reads_every_key_a_dataset_holds() {
        assert_eq!(
            keys(saved(&[("foo", "bar"), ("baz", "qux")])),
            ["foo", "baz"],
        );
    }

    #[test]
    fn reads_what_the_keys_hold() {
        let records = records(saved(&[("foo", "bar")])).unwrap();

        assert_eq!(records[0].value, "bar");
        assert_eq!(records[0].expires_at, None);
    }

    #[test]
    fn reads_a_dataset_with_nothing_in_it() {
        assert!(keys(saved(&[])).is_empty());
    }

    #[test]
    fn reads_past_the_facts_about_the_server_that_saved_it() {
        let mut file = Vec::from(*b"REDIS0011");

        for (name, value) in [("redis-ver", "7.2.0"), ("redis-bits", "64")] {
            file.push(AUXILIARY);
            for part in [name, value] {
                file.push(part.len() as u8);
                file.extend_from_slice(part.as_bytes());
            }
        }

        file.extend_from_slice(&[DATABASE, 0, STRING, 3]);
        file.extend_from_slice(b"foo");
        file.push(3);
        file.extend_from_slice(b"bar");
        file.push(END);
        file.extend_from_slice(&[0; 8]);

        assert_eq!(keys(Bytes::from(file)), ["foo"]);
    }

    #[test]
    fn reads_a_dataset_saved_without_the_sizes_in_it() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, STRING, 1, b'a', 1, b'b', END]);
        file.extend_from_slice(&[0; 8]);

        assert_eq!(keys(Bytes::from(file)), ["a"]);
    }

    #[test]
    fn reads_the_time_a_key_is_to_go_at() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, EXPIRES_AT_MILLISECONDS]);
        // A Wednesday in 2033, spelled least significant byte first.
        file.extend_from_slice(&2_000_000_000_000_u64.to_le_bytes());
        file.extend_from_slice(&[STRING, 1, b'a', 1, b'b', END]);
        file.extend_from_slice(&[0; 8]);

        let records = records(Bytes::from(file)).unwrap();

        assert_eq!(
            records[0].expires_at,
            Some(UNIX_EPOCH + Duration::from_millis(2_000_000_000_000)),
        );
    }

    #[test]
    fn reads_the_time_a_key_is_to_go_at_in_whole_seconds() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, EXPIRES_AT_SECONDS]);
        file.extend_from_slice(&2_000_000_000_u32.to_le_bytes());
        file.extend_from_slice(&[STRING, 1, b'a', 1, b'b', END]);
        file.extend_from_slice(&[0; 8]);

        let records = records(Bytes::from(file)).unwrap();

        assert_eq!(
            records[0].expires_at,
            Some(UNIX_EPOCH + Duration::from_secs(2_000_000_000)),
        );
    }

    #[test]
    fn leaves_the_next_key_without_the_time_the_one_before_it_carried() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, EXPIRES_AT_SECONDS]);
        file.extend_from_slice(&2_000_000_000_u32.to_le_bytes());
        file.extend_from_slice(&[STRING, 1, b'a', 1, b'b']);
        file.extend_from_slice(&[STRING, 1, b'c', 1, b'd', END]);
        file.extend_from_slice(&[0; 8]);

        let records = records(Bytes::from(file)).unwrap();

        assert!(records[0].expires_at.is_some());
        assert_eq!(records[1].expires_at, None);
    }

    #[test]
    fn keeps_only_the_first_of_several_databases() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, STRING, 1, b'a', 1, b'b']);
        file.extend_from_slice(&[DATABASE, 1, STRING, 1, b'c', 1, b'd', END]);
        file.extend_from_slice(&[0; 8]);

        assert_eq!(keys(Bytes::from(file)), ["a"]);
    }

    #[test]
    fn reads_a_key_longer_than_six_bits_can_count() {
        let key = "k".repeat(300);
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, STRING]);
        // Fourteen bits: the top two say so, and the rest is the length.
        file.extend_from_slice(&[0x40 | (300 >> 8) as u8, (300 & 0xff) as u8]);
        file.extend_from_slice(key.as_bytes());
        file.extend_from_slice(&[1, b'b', END]);
        file.extend_from_slice(&[0; 8]);

        assert_eq!(keys(Bytes::from(file)), [key]);
    }

    #[test]
    fn reads_a_value_kept_as_the_number_it_spells() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, STRING, 1, b'a', 0xC0, 42]);
        file.extend_from_slice(&[STRING, 1, b'b', 0xC1]);
        file.extend_from_slice(&1000_i16.to_le_bytes());
        file.extend_from_slice(&[STRING, 1, b'c', 0xC2]);
        file.extend_from_slice(&(-100_000_i32).to_le_bytes());
        file.push(END);
        file.extend_from_slice(&[0; 8]);

        let records = records(Bytes::from(file)).unwrap();
        let values: Vec<_> = records.iter().map(|record| record.value.clone()).collect();

        assert_eq!(values, ["42", "1000", "-100000"]);
    }

    #[test]
    fn turns_down_what_is_not_a_dataset() {
        for file in [
            &b""[..],
            b"REDIS",
            b"REDIS001",
            b"NOTIT0011\xff\0\0\0\0\0\0\0\0",
        ] {
            assert!(records(Bytes::from_static(file)).is_err(), "{file:?}");
        }
    }

    #[test]
    fn turns_down_a_dataset_that_stops_part_way() {
        // A key that says it is three bytes long, with two of them there.
        let mut file = Vec::from(*b"REDIS0011");
        file.extend_from_slice(&[DATABASE, 0, STRING, 3, b'f', b'o']);

        assert!(records(Bytes::from(file)).is_err());
    }

    #[test]
    fn turns_down_a_dataset_that_never_ends() {
        let mut file = Vec::from(*b"REDIS0011");
        file.extend_from_slice(&[DATABASE, 0]);

        assert!(records(Bytes::from(file)).is_err());
    }

    #[test]
    fn turns_down_a_kind_of_value_it_cannot_read() {
        let mut file = Vec::from(*b"REDIS0011");
        // A hash saved in its packed layout.
        file.extend_from_slice(&[DATABASE, 0, 0x10, 1, b'a']);

        assert!(records(Bytes::from(file)).is_err());
    }

    #[test]
    fn lets_a_squeezed_string_out_again() {
        // A single `a`, then nineteen more taken from the one before each.
        let squeezed = [0x00, b'a', 0xE0, 0x0A, 0x00];

        assert_eq!(unsqueeze(&squeezed, 20).unwrap(), "a".repeat(20));
    }

    #[test]
    fn lets_a_squeezed_string_of_its_own_bytes_out_again() {
        let squeezed = [0x02, b'a', b'b', b'c'];

        assert_eq!(unsqueeze(&squeezed, 3).unwrap(), "abc");
    }

    #[test]
    fn reads_a_value_that_was_squeezed_down() {
        let mut file = Vec::from(*b"REDIS0011");

        file.extend_from_slice(&[DATABASE, 0, STRING, 1, b'a', 0xC3, 5, 20]);
        file.extend_from_slice(&[0x00, b'a', 0xE0, 0x0A, 0x00]);
        file.push(END);
        file.extend_from_slice(&[0; 8]);

        let records = records(Bytes::from(file)).unwrap();

        assert_eq!(records[0].value, "a".repeat(20));
    }

    #[test]
    fn turns_down_a_squeezed_string_that_does_not_add_up() {
        // A run saying it holds more bytes than are there.
        assert!(unsqueeze(&[0x05, b'a'], 6).is_err());
        // A repeat reaching back before anything was written.
        assert!(unsqueeze(&[0x20, 0x05], 2).is_err());
        // A string that came out shorter than it said it would.
        assert!(unsqueeze(&[0x00, b'a'], 20).is_err());
    }

    #[test]
    fn treats_a_dataset_that_was_never_saved_as_one_holding_nothing() {
        let path = std::env::temp_dir().join("a-dataset-that-was-never-saved.rdb");

        assert!(read(&path).unwrap().is_empty());
    }
}
