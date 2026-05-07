use anyhow::{Result, bail};

/// A value in the Redis serialization protocol.
#[derive(Debug, PartialEq)]
pub enum Value {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(String),
    Array(Vec<Value>),
    /// The null bulk string, used for replies such as a `GET` on a missing key.
    Null,
    /// The null array, which is what a blocking command replies on a timeout.
    NullArray,
}

impl Value {
    pub fn encode(&self) -> String {
        match self {
            Value::SimpleString(text) => format!("+{text}\r\n"),
            Value::Error(message) => format!("-{message}\r\n"),
            Value::Integer(number) => format!(":{number}\r\n"),
            Value::BulkString(text) => format!("${}\r\n{text}\r\n", text.len()),
            Value::Array(values) => {
                let mut encoded = format!("*{}\r\n", values.len());
                for value in values {
                    encoded.push_str(&value.encode());
                }
                encoded
            }
            Value::Null => "$-1\r\n".to_string(),
            Value::NullArray => "*-1\r\n".to_string(),
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
            b'+' => Ok(Some(Value::SimpleString(text(payload)?))),
            b'-' => Ok(Some(Value::Error(text(payload)?))),
            b':' => Ok(Some(Value::Integer(text(payload)?.parse()?))),
            b'$' => self.bulk_string(payload),
            b'*' => self.array(payload),
            _ => bail!("unknown type byte '{}'", kind as char),
        }
    }

    fn bulk_string(&mut self, payload: &[u8]) -> Result<Option<Value>> {
        let len: usize = text(payload)?.parse()?;

        let rest = &self.input[self.pos..];
        if rest.len() < len + 2 {
            return Ok(None);
        }

        self.pos += len + 2;
        Ok(Some(Value::BulkString(text(&rest[..len])?)))
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

fn text(bytes: &[u8]) -> Result<String> {
    Ok(str::from_utf8(bytes)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array(items: &[&str]) -> Value {
        Value::Array(
            items
                .iter()
                .map(|item| Value::BulkString(item.to_string()))
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
    fn encodes_a_bulk_string() {
        assert_eq!(Value::BulkString("hey".into()).encode(), "$3\r\nhey\r\n");
    }
}
