//! A byte-oriented cursor over one source file.
//!
//! The cursor is the only place in the lexer that touches raw bytes. Keeping
//! it separate means the scanning rules in [`crate::lexer`] can be written in
//! terms of "peek / bump / eat_while" without worrying about byte-index
//! arithmetic or UTF-8 boundaries.
//!
//! ```text
//! A name's character classes are Unicode's: `is_ident_start_char` and
//! `is_ident_continue_char` test UAX #31's `XID_Start` and `XID_Continue` plus
//! `_`. The byte predicates are the ASCII fast path, for a position where only
//! an ASCII name can appear -- a numeric literal's type suffix.
//! ```

use fallc_span::{FileId, Span};

/// A position-tracked cursor over the bytes of a single source file.
#[derive(Clone, Debug)]
pub struct Cursor<'src> {
    file: FileId,
    text: &'src str,
    pos: usize,
}

impl<'src> Cursor<'src> {
    /// Creates a cursor at the start of `text`, which must belong to `file`.
    #[must_use]
    pub fn new(file: FileId, text: &'src str) -> Self {
        Cursor { file, text, pos: 0 }
    }

    /// Returns the file this cursor reads from.
    #[must_use]
    pub fn file(&self) -> FileId {
        self.file
    }

    /// Returns the whole source text.
    #[must_use]
    pub fn text(&self) -> &'src str {
        self.text
    }

    /// Returns the current byte offset.
    #[must_use]
    pub fn offset(&self) -> u32 {
        u32::try_from(self.pos).expect("source file exceeds 4 GiB")
    }

    /// Returns `true` if the cursor has consumed everything.
    #[must_use]
    pub fn is_at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    /// Returns the remaining text.
    #[must_use]
    pub fn rest(&self) -> &'src str {
        &self.text[self.pos..]
    }

    /// Returns the next byte without consuming it.
    #[must_use]
    pub fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    /// Returns the byte `n` positions ahead without consuming anything.
    #[must_use]
    pub fn peek_at(&self, n: usize) -> Option<u8> {
        self.text.as_bytes().get(self.pos + n).copied()
    }

    /// Returns the next character without consuming it.
    #[must_use]
    pub fn peek_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// Consumes one byte.
    ///
    /// Only safe to use when the next byte is known to be ASCII; use
    /// [`Cursor::bump_char`] otherwise.
    pub fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    /// Consumes one whole character, returning it.
    pub fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    /// Consumes `byte` if it is next, reporting whether it was consumed.
    pub fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consumes `text` if the input starts with it.
    pub fn eat_str(&mut self, text: &str) -> bool {
        if self.rest().starts_with(text) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    /// Consumes bytes while `pred` holds, returning the consumed slice.
    pub fn eat_while(&mut self, pred: impl Fn(u8) -> bool) -> &'src str {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if !pred(byte) {
                break;
            }
            self.pos += 1;
        }
        &self.text[start..self.pos]
    }

    /// Returns `true` if the remaining input starts with `text`.
    #[must_use]
    pub fn starts_with(&self, text: &str) -> bool {
        self.rest().starts_with(text)
    }

    /// Returns the source text covered by `[from, current)`.
    ///
    /// # Panics
    ///
    /// Panics if `from` is beyond the current position or not on a character
    /// boundary. Both indicate a lexer bug.
    #[must_use]
    pub fn slice_from(&self, from: u32) -> &'src str {
        let from = from as usize;
        assert!(from <= self.pos, "slice start is ahead of the cursor");
        &self.text[from..self.pos]
    }

    /// Builds a span `[from, current)`.
    #[must_use]
    pub fn span_from(&self, from: u32) -> Span {
        Span::new(self.file, from, self.offset())
    }

    /// Moves the cursor back to an earlier offset.
    ///
    /// Used to un-consume a run of bytes that turned out not to be what it
    /// looked like, such as a numeric suffix that is not a type name.
    ///
    /// # Panics
    ///
    /// Panics if `offset` is ahead of the cursor or not on a character
    /// boundary, both of which are lexer bugs.
    pub fn rewind_to(&mut self, offset: u32) {
        let offset = offset as usize;
        assert!(offset <= self.pos, "cannot rewind forwards");
        assert!(
            self.text.is_char_boundary(offset),
            "cannot rewind to a non-character boundary"
        );
        self.pos = offset;
    }

    /// Builds a span around the last consumed byte.
    #[must_use]
    pub fn span_of_last(&self) -> Span {
        let hi = self.offset();
        Span::new(self.file, hi.saturating_sub(1), hi)
    }

    /// Builds an empty span at the current position.
    #[must_use]
    pub fn span_point(&self) -> Span {
        Span::point(self.file, self.offset())
    }
}

/// Returns `true` if `byte` can start an ASCII identifier.
///
/// The byte form is what a numeric literal's type suffix is scanned with; a
/// *name* goes through [`is_ident_start_char`], which is the whole of Unicode.
#[must_use]
pub const fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

/// Returns `true` if `byte` can continue an ASCII identifier.
#[must_use]
pub const fn is_ident_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// Returns `true` if `character` can start an identifier.
///
/// UAX #31's `XID_Start`, plus `_`: a name may begin with any letter of any
/// script, and with the characters that continue a word -- `é`, `中`, `δ`. The
/// classes are the standard's, not an approximation, because they decide what
/// the language accepts.
#[must_use]
pub fn is_ident_start_char(character: char) -> bool {
    character == '_' || unicode_ident::is_xid_start(character)
}

/// Returns `true` if `character` can continue an identifier.
///
/// UAX #31's `XID_Continue`, plus `_`: digits, combining marks, and the
/// connector punctuation a name like `snake_case` is written with.
#[must_use]
pub fn is_ident_continue_char(character: char) -> bool {
    character == '_' || unicode_ident::is_xid_continue(character)
}

/// Returns `true` if `byte` is horizontal or vertical whitespace.
#[must_use]
pub const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Returns `true` if `byte` is an ASCII decimal digit.
#[must_use]
pub const fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(text: &str) -> Cursor<'_> {
        Cursor::new(FileId::from_raw(0), text)
    }

    #[test]
    fn bump_and_peek_track_offset() {
        let mut c = cursor("ab");
        assert_eq!(c.peek(), Some(b'a'));
        assert_eq!(c.bump(), Some(b'a'));
        assert_eq!(c.offset(), 1);
        assert_eq!(c.bump(), Some(b'b'));
        assert!(c.is_at_end());
        assert_eq!(c.bump(), None);
    }

    #[test]
    fn eat_while_returns_consumed_slice() {
        let mut c = cursor("abc123");
        assert_eq!(c.eat_while(is_ident_start), "abc");
        assert_eq!(c.eat_while(is_ident_continue), "123");
        assert_eq!(c.rest(), "");
    }

    #[test]
    fn bump_char_respects_utf8_width() {
        let mut c = cursor("é!");
        assert_eq!(c.bump_char(), Some('é'));
        assert_eq!(c.offset(), 2);
        assert_eq!(c.span_of_last(), Span::new(FileId::from_raw(0), 1, 2));
        assert_eq!(c.bump_char(), Some('!'));
    }

    #[test]
    fn eat_str_is_lookahead_safe() {
        let mut c = cursor("==>");
        assert!(c.eat_str("=="));
        assert_eq!(c.rest(), ">");
    }

    #[test]
    fn eat_str_declines_without_consuming() {
        let mut c = cursor("=>");
        assert!(!c.eat_str("=="));
        assert_eq!(c.offset(), 0);
        assert_eq!(c.rest(), "=>");
    }
}
