//! Decoding of literal text into the values it denotes.
//!
//! Both the type checker and the code generator need to know what a character
//! literal means: the checker to reject one that does not fit its type, and
//! codegen to emit the constant. Decoding it twice, in two crates, is how the
//! two would come to disagree about an escape sequence — the checker accepting
//! a literal that codegen then silently truncates, or the reverse. The decoder
//! therefore lives here, in the crate both of them depend on, and there is one
//! copy of it.
//!
//! The escape set is C's: `\n`, `\t`, `\r`, `\0`, `\\`, `\'`, `\"`, `\a`,
//! `\b`, `\f`, `\v`, and `\xHH` with one or two hex digits. A character literal
//! also takes `\u{...}` for a code point written in hex. An unrecognised escape
//! keeps its backslash in a string and yields the bare character in a character
//! literal, so neither silently loses the text.

/// Returns the code point a character literal denotes, or `None` if its text
/// is not a well-formed one-character literal.
///
/// The text includes the quotes, and may carry a width prefix in front of them:
/// `'x'` and `u16'x'` both arrive here.
#[must_use]
pub fn decode_char_code_point(text: &str) -> Option<u32> {
    let body = match text.find('\'') {
        Some(open) => text[open + 1..]
            .strip_suffix('\'')
            .unwrap_or(&text[open + 1..]),
        None => text,
    };

    // An escape sequence yields a single code point.
    if let Some(rest) = body.strip_prefix('\\') {
        return Some(decode_one_escape(rest));
    }

    let mut characters = body.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Some(character as u32),
        _ => None,
    }
}

/// Decodes one escape sequence, given the text after the backslash.
///
/// An unrecognised escape yields the character itself, which is the
/// conservative reading: dropping it would silently change the value.
#[must_use]
pub fn decode_one_escape(rest: &str) -> u32 {
    let mut characters = rest.chars();
    match characters.next() {
        Some('n') => b'\n' as u32,
        Some('t') => b'\t' as u32,
        Some('r') => b'\r' as u32,
        Some('0') => 0,
        Some('\\') => b'\\' as u32,
        Some('\'') => b'\'' as u32,
        Some('"') => b'"' as u32,
        Some('a') => 0x07,
        Some('b') => 0x08,
        Some('f') => 0x0c,
        Some('v') => 0x0b,
        Some('x') => {
            let digits: String = characters.take(2).collect();
            u32::from_str_radix(&digits, 16).unwrap_or(0)
        }
        // `\u{1F600}` takes a braced code point.
        Some('u') => {
            let rest: String = characters.collect();
            let trimmed = rest.trim_start_matches('{').trim_end_matches('}');
            u32::from_str_radix(trimmed, 16).unwrap_or(0)
        }
        Some(other) => other as u32,
        None => b'\\' as u32,
    }
}

/// Returns the body of a string literal, without its delimiters.
///
/// Three spellings share this: `".."`, the multi-line `""".."""`, and the
/// width-prefixed forms `u16".."`. The body is what the delimiters enclose,
/// with escapes still undecoded -- [`decode_escapes`] is the next step.
///
/// A raw string is not one of them: `r".."` and `r#tag".."tag#` take their
/// characters verbatim, and their body is found by their own rule.
#[must_use]
pub fn string_body(text: &str) -> &str {
    let Some(open) = text.find('"') else {
        return "";
    };
    let after_open = open + 1;
    // `"""` opens a literal that may span lines and closes on three quotes.
    // Everything between is the body, including any single quote.
    if text[open..].starts_with("\"\"\"") {
        let body_start = open + 3;
        let Some(close) = text.rfind("\"\"\"") else {
            return "";
        };
        return if body_start <= close {
            &text[body_start..close]
        } else {
            ""
        };
    }
    let Some(close) = text.rfind('"') else {
        return "";
    };
    if after_open <= close {
        &text[after_open..close]
    } else {
        ""
    }
}

/// Decodes the escape sequences in a string or byte-string body.
pub fn decode_escapes(body: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(body.len());
    let mut characters = body.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            continue;
        }

        match characters.next() {
            Some('n') => bytes.push(b'\n'),
            Some('t') => bytes.push(b'\t'),
            Some('r') => bytes.push(b'\r'),
            Some('0') => bytes.push(0),
            Some('\\') => bytes.push(b'\\'),
            Some('"') => bytes.push(b'"'),
            Some('\'') => bytes.push(b'\''),
            Some('a') => bytes.push(0x07),
            Some('b') => bytes.push(0x08),
            Some('f') => bytes.push(0x0c),
            Some('v') => bytes.push(0x0b),
            Some('x') => {
                // `\xHH`, one or two hex digits.
                let mut value: u32 = 0;
                let mut digits = 0;
                while digits < 2 {
                    match characters.clone().next().and_then(|c| c.to_digit(16)) {
                        Some(digit) => {
                            value = value * 16 + digit;
                            characters.next();
                            digits += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    // Not a valid `\x`: keep it as written.
                    bytes.push(b'\\');
                    bytes.push(b'x');
                } else {
                    bytes.push(value as u8);
                }
            }
            // `\u{...}`, a Unicode scalar value. The characters it stands for
            // are encoded the way the string is: a string is a sequence of
            // bytes, and a code point above 0x7F occupies several of them.
            Some('u') if characters.clone().next() == Some('{') => {
                characters.next();
                let mut value: u32 = 0;
                let mut digits = 0;
                let mut closed = false;
                while let Some(next) = characters.clone().next() {
                    if next == '}' {
                        characters.next();
                        closed = true;
                        break;
                    }
                    let Some(digit) = next.to_digit(16) else {
                        break;
                    };
                    value = value.saturating_mul(16).saturating_add(digit);
                    characters.next();
                    digits += 1;
                }
                match (closed, digits, char::from_u32(value)) {
                    (true, 1..=6, Some(character)) => {
                        let mut buffer = [0u8; 4];
                        bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                    }
                    // Not a scalar value, or not written the way `\u` is: the
                    // text stays as it was written, as for any other escape.
                    _ => {
                        bytes.push(b'\\');
                        bytes.push(b'u');
                    }
                }
            }
            // An unrecognised escape keeps both characters.
            Some(other) => {
                bytes.push(b'\\');
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
            None => bytes.push(b'\\'),
        }
    }
    bytes
}

/// Decodes an integer literal's digits into its value.
///
/// A literal is written with a radix prefix -- `0x2A`, `0o52`, `0b101010` -- and
/// may use `_` between digits. Decoding it is the same job everywhere a value is
/// wanted, and doing it by `str::parse` at each site quietly made every
/// non-decimal literal "not a constant", because `0x80` does not parse as a
/// decimal number.
///
/// `None` when the digits are not a number this compiler can hold, which is the
/// only case a caller has to report.
#[must_use]
pub fn decode_int_value(digits: &str) -> Option<i64> {
    // Separators carry no value, and the scanner has already checked that they
    // sit between digits.
    let cleaned: String = digits
        .chars()
        .filter(|character| *character != '_')
        .collect();
    let (radix, body) = if let Some(rest) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        (2, rest)
    } else {
        (10, cleaned.as_str())
    };
    i64::from_str_radix(body, radix).ok()
}

#[cfg(test)]
mod tests {
    use super::{decode_char_code_point, decode_escapes};

    #[test]
    fn a_plain_character_decodes_to_its_code_point() {
        assert_eq!(decode_char_code_point("'A'"), Some(65));
        assert_eq!(decode_char_code_point("'中'"), Some(0x4E2D));
        // A width prefix is skipped.
        assert_eq!(decode_char_code_point("u16'A'"), Some(65));
    }

    #[test]
    fn escapes_decode_the_same_way_with_and_without_a_prefix() {
        assert_eq!(decode_char_code_point(r"'\n'"), Some(10));
        assert_eq!(decode_char_code_point(r"u32'\u{4E2D}'"), Some(0x4E2D));
        assert_eq!(decode_char_code_point(r"'\x41'"), Some(65));
    }

    #[test]
    fn a_body_that_is_not_one_character_does_not_decode() {
        assert_eq!(decode_char_code_point("''"), None);
        assert_eq!(decode_char_code_point("'ab'"), None);
    }

    #[test]
    fn escapes_in_a_string_body_are_decoded_in_place() {
        assert_eq!(decode_escapes(r"a\tb"), b"a\tb");
        assert_eq!(decode_escapes(r"\x41\x42"), b"AB");
        // A multi-byte character keeps its UTF-8 encoding.
        assert_eq!(decode_escapes("中"), "\u{4E2D}".as_bytes());
        // And an escape after one is still found.
        assert_eq!(decode_escapes(r"中\n"), "\u{4E2D}\n".as_bytes());
    }
}
