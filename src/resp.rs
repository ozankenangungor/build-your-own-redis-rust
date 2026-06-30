use anyhow::{Result, bail};
use bytes::Bytes;
use std::io::Write;

/// The largest bulk string Redis accepts, and so the largest we do.
const MAX_BULK_LENGTH: usize = 512 * 1024 * 1024;

/// A value in the Redis serialization protocol.
///
/// Bulk strings carry raw bytes, because Redis keys and values are binary safe:
/// a client may store an image or a compressed blob just as easily as a word.
/// The other kinds only ever come from the server, so they stay text.
#[derive(Debug, PartialEq)]
pub enum Value {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Bytes),
    Array(Vec<Value>),
    /// The null bulk string, used for replies such as a `GET` on a missing key.
    Null,
    /// The null array, which is what a blocking command replies on a timeout.
    NullArray,
    /// A file, laid out like a bulk string but with no CRLF after it: what
    /// follows a file is not another value. This is how a master hands a
    /// replica the whole of its dataset.
    File(Bytes),
    /// Values written one after another, for the answers that are more than one
    /// value. Only ever written, never read.
    Sequence(Vec<Value>),
}

impl Value {
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        self.encode_into(&mut encoded);
        encoded
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        // Writing into a `Vec` cannot fail, so the results are safe to unwrap.
        match self {
            Value::SimpleString(text) => write!(out, "+{text}\r\n").unwrap(),
            Value::Error(message) => write!(out, "-{message}\r\n").unwrap(),
            Value::Integer(number) => write!(out, ":{number}\r\n").unwrap(),
            Value::BulkString(bytes) => {
                write!(out, "${}\r\n", bytes.len()).unwrap();
                out.extend_from_slice(bytes);
                out.extend_from_slice(b"\r\n");
            }
            Value::Array(values) => {
                write!(out, "*{}\r\n", values.len()).unwrap();
                for value in values {
                    value.encode_into(out);
                }
            }
            Value::Null => out.extend_from_slice(b"$-1\r\n"),
            Value::NullArray => out.extend_from_slice(b"*-1\r\n"),
            Value::File(bytes) => {
                write!(out, "${}\r\n", bytes.len()).unwrap();
                out.extend_from_slice(bytes);
            }
            Value::Sequence(values) => {
                for value in values {
                    value.encode_into(out);
                }
            }
        }
    }
}

/// Parses the value at the start of `input`, returning it together with the
/// number of bytes it occupied. `Ok(None)` means `input` holds only part of a
/// value and the caller should read more before trying again.
pub fn parse(input: &[u8]) -> Result<Option<(Value, usize)>> {
    let mut parser = Parser { input, pos: 0 };
    Ok(parser.value()?.map(|value| (value, parser.pos)))
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Consumes up to the next CRLF, returning the bytes before it.
    fn line(&mut self) -> Option<&'a [u8]> {
        let rest = &self.input[self.pos..];
        let end = rest.windows(2).position(|pair| pair == b"\r\n")?;
        self.pos += end + 2;
        Some(&rest[..end])
    }

    fn value(&mut self) -> Result<Option<Value>> {
        let Some(line) = self.line() else {
            return Ok(None);
        };
        let Some((&kind, payload)) = line.split_first() else {
            bail!("empty frame");
        };

        match kind {
            b'+' => Ok(Some(Value::SimpleString(text(payload)?.to_string()))),
            b'-' => Ok(Some(Value::Error(text(payload)?.to_string()))),
            b':' => Ok(Some(Value::Integer(text(payload)?.parse()?))),
            b'$' => self.bulk_string(payload),
            b'*' => self.array(payload),
            _ => bail!("unknown type byte '{}'", kind as char),
        }
    }

    fn bulk_string(&mut self, payload: &[u8]) -> Result<Option<Value>> {
        let len: usize = text(payload)?.parse()?;
        // Refusing absurd lengths keeps a bogus one from being taken at face
        // value, and keeps the arithmetic below from overflowing.
        if len > MAX_BULK_LENGTH {
            bail!("invalid bulk length");
        }

        let rest = &self.input[self.pos..];
        if rest.len() < len + 2 {
            return Ok(None);
        }

        self.pos += len + 2;
        Ok(Some(Value::BulkString(Bytes::copy_from_slice(
            &rest[..len],
        ))))
    }

    fn array(&mut self, payload: &[u8]) -> Result<Option<Value>> {
        let len: usize = text(payload)?.parse()?;

        let mut values = Vec::new();
        for _ in 0..len {
            let Some(value) = self.value()? else {
                return Ok(None);
            };
            values.push(value);
        }

        Ok(Some(Value::Array(values)))
    }
}

/// Only the sizes and the server's own replies are read as text; the payload of
/// a bulk string is left as bytes.
fn text(bytes: &[u8]) -> Result<&str> {
    Ok(str::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array(items: &[&str]) -> Value {
        Value::Array(
            items
                .iter()
                .map(|item| Value::BulkString(Bytes::copy_from_slice(item.as_bytes())))
                .collect(),
        )
    }

    #[test]
    fn parses_a_command() {
        let input = b"*2\r\n$4\r\nECHO\r\n$3\r\nhey\r\n";

        assert_eq!(
            parse(input).unwrap(),
            Some((array(&["ECHO", "hey"]), input.len()))
        );
    }

    #[test]
    fn reports_how_much_it_consumed_so_pipelined_commands_are_not_lost() {
        let input = b"*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n";
        let (_, consumed) = parse(input).unwrap().unwrap();

        assert_eq!(consumed, input.len() / 2);
        assert_eq!(
            parse(&input[consumed..]).unwrap(),
            Some((array(&["PING"]), consumed))
        );
    }

    #[test]
    fn waits_for_the_rest_of_a_split_command() {
        assert_eq!(parse(b"*2\r\n$4\r\nECHO\r\n$3\r\nh").unwrap(), None);
    }

    #[test]
    fn refuses_a_bulk_length_it_could_never_hold() {
        // Taken at face value, this length would overflow the arithmetic that
        // looks for the end of the string.
        assert!(parse(b"$18446744073709551615\r\n").is_err());
    }

    #[test]
    fn carries_bytes_that_are_not_text() {
        let input = b"*1\r\n$3\r\n\xff\x00\xfe\r\n";
        let expected = Value::Array(vec![Value::BulkString(Bytes::from_static(b"\xff\x00\xfe"))]);

        assert_eq!(parse(input).unwrap(), Some((expected, input.len())));
    }

    #[test]
    fn encodes_a_file_without_the_crlf_a_bulk_string_ends_in() {
        let value = Value::File(Bytes::from_static(b"\x00\x01"));

        assert_eq!(value.encode(), b"$2\r\n\x00\x01");
    }

    #[test]
    fn encodes_a_sequence_as_one_value_after_another() {
        let value = Value::Sequence(vec![
            Value::SimpleString("FIRST".into()),
            Value::SimpleString("SECOND".into()),
        ]);

        assert_eq!(value.encode(), b"+FIRST\r\n+SECOND\r\n");
    }

    #[test]
    fn encodes_a_bulk_string_verbatim() {
        let value = Value::BulkString(Bytes::from_static(b"\xff\x00"));

        assert_eq!(value.encode(), b"$2\r\n\xff\x00\r\n");
    }
}
