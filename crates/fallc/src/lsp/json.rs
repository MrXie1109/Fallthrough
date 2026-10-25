//! A small JSON reader and writer, for the language server.
//!
//! The server speaks JSON-RPC, whose messages are JSON, and the workspace has
//! no JSON dependency: the registry is not reachable from the machines this is
//! built on, and adding a dependency to the driver for two message shapes would
//! be the tail wagging the dog. So this is the part of JSON that a language
//! server needs -- objects, arrays, strings, numbers, booleans, null -- written
//! plainly. A malformed message is reported as a protocol error rather than
//! parsed leniently.

use std::collections::BTreeMap;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// A number, kept as its JSON text so that no precision is invented.
    Number(String),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object, with its keys in order.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Creates a string value.
    pub fn string(text: impl Into<String>) -> Value {
        Value::String(text.into())
    }

    /// Creates an integer value.
    pub fn number(value: impl std::fmt::Display) -> Value {
        Value::Number(value.to_string())
    }

    /// Returns the value under `key`, for an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Returns the string under `key`.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::String(text)) => Some(text),
            _ => None,
        }
    }

    /// Returns the integer under `key`.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Value::Number(text)) => text.parse().ok(),
            _ => None,
        }
    }

    /// Returns the object under `key`.
    pub fn get_object(&self, key: &str) -> Option<&BTreeMap<String, Value>> {
        match self.get(key) {
            Some(Value::Object(map)) => Some(map),
            _ => None,
        }
    }

    /// Returns this value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    /// Returns this value as an integer.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// Writes this value as JSON text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Number(text) => out.push_str(text),
            Value::String(text) => write_string(text, out),
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Value::Object(map) => {
                out.push('{');
                for (index, (key, value)) in map.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// Writes a JSON string, with the escapes the format requires.
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Why a message could not be read.
#[derive(Debug)]
pub struct ParseError {
    /// What was wrong, in the terms a protocol reader can act on.
    pub message: String,
}

impl ParseError {
    fn new(message: impl Into<String>) -> ParseError {
        ParseError {
            message: message.into(),
        }
    }
}

/// Parses one JSON value from `text`, which must contain nothing else.
pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut parser = Parser {
        text: text.as_bytes(),
        at: 0,
    };
    parser.skip_space();
    let value = parser.value()?;
    parser.skip_space();
    if parser.at != parser.text.len() {
        return Err(ParseError::new("trailing text after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while let Some(&byte) = self.text.get(self.at) {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(ParseError::new(format!(
                "expected `{}` at byte {}",
                byte as char, self.at
            )))
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            None => Err(ParseError::new("unexpected end of input")),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(_) => self.number(),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, ParseError> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(ParseError::new(format!("expected `{word}`")))
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_space();
            items.push(self.value()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(ParseError::new("expected `,` or `]` in an array")),
            }
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.expect(b':')?;
            self.skip_space();
            let value = self.value()?;
            map.insert(key, value);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(ParseError::new("expected `,` or `}` in an object")),
            }
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.at;
        while let Some(byte) = self.peek() {
            if matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                self.at += 1;
            } else {
                break;
            }
        }
        if start == self.at {
            return Err(ParseError::new("expected a value"));
        }
        let text = std::str::from_utf8(&self.text[start..self.at])
            .map_err(|_| ParseError::new("a number that is not ASCII"))?;
        Ok(Value::Number(text.to_owned()))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(ParseError::new("unterminated string"));
            };
            self.at += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(ParseError::new("unterminated escape"));
                    };
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code = self.hex4()?;
                            let character = if (0xd800..0xdc00).contains(&code) {
                                if self.peek() == Some(b'\\')
                                    && self.text.get(self.at + 1) == Some(&b'u')
                                {
                                    self.at += 2;
                                    let low = self.hex4()?;
                                    let combined =
                                        0x1_0000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                                    char::from_u32(combined)
                                } else {
                                    None
                                }
                            } else {
                                char::from_u32(code)
                            };
                            match character {
                                Some(character) => out.push(character),
                                None => out.push('\u{fffd}'),
                            }
                        }
                        other => {
                            return Err(ParseError::new(format!(
                                "unknown escape `\\{}`",
                                other as char
                            )));
                        }
                    }
                }
                first => {
                    let start = self.at - 1;
                    let width = match first {
                        0x00..=0x7f => 1,
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    self.at = start + width;
                    let bytes = self
                        .text
                        .get(start..self.at)
                        .ok_or_else(|| ParseError::new("truncated character"))?;
                    let text = std::str::from_utf8(bytes)
                        .map_err(|_| ParseError::new("invalid UTF-8 in a string"))?;
                    out.push_str(text);
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err(ParseError::new("truncated `\\u` escape"));
            };
            self.at += 1;
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => return Err(ParseError::new("a non-hex digit in `\\u`")),
            };
            value = value * 16 + digit;
        }
        Ok(value)
    }
}
