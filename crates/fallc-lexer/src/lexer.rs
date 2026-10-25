//! The scanner: source text in, tokens out.
//!
//! # Driving the lexer
//!
//! ```text
//! let mut lexer = Lexer::new(LexerConfig::default());
//! let result = lexer.tokenize(source, file_id);
//! ```
//!
//! [`Lexer::next_token`] is the single-token entry point; the parser needs it
//! because the parser is not token-at-a-time — it looks ahead, and sometimes
//! wants to re-lex a raw token sequence under a different mode (a raw string,
//! for instance).
//!
//! # Recovery
//!
//! Every scanning routine either consumes at least one byte or returns `None`.
//! When a character cannot start any token, the lexer emits a
//! [`TokenKind::Unknown`] token covering exactly that character and
//! reports a diagnostic, so the token stream is always total and progress is
//! always made.

use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_span::{FileId, Span};

use crate::cursor::{
    Cursor, is_ident_continue, is_ident_continue_char, is_ident_start, is_ident_start_char,
    is_whitespace,
};

/// The longest delimiter a raw string may carry, in bytes.
///
/// A bound is what makes the opening delimiter findable without looking at the
/// whole file: `r#tag"` is a raw string only if the tag is short, and anything
/// longer is an identifier called `r` followed by a `#`.
const MAX_RAW_DELIMITER: usize = 64;
use crate::token::{Token, TokenKind};

/// Knobs that change what the lexer produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LexerConfig {
    /// If `true`, whitespace and comments are collected into
    /// [`Token::leading`] instead of being dropped.
    pub preserve_trivia: bool,
    /// If `true`, lexical errors are reported as warnings.
    ///
    /// Useful for tools that want a best-effort token stream without failing.
    pub errors_are_warnings: bool,
}

/// The token stream for one file, plus whatever went wrong producing it.
#[derive(Clone, Debug)]
pub struct LexResult {
    /// The tokens, always terminated by exactly one [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// Diagnostics produced while scanning.
    pub diagnostics: Vec<Diagnostic>,
}

impl LexResult {
    /// Returns `true` if scanning produced no fatal diagnostic.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_fatal)
    }

    /// Returns every token except the trailing [`TokenKind::Eof`].
    pub fn tokens_without_eof(&self) -> &[Token] {
        match self.tokens.split_last() {
            Some((last, rest)) if last.is_eof() => rest,
            _ => &self.tokens,
        }
    }
}

/// The scanner state for one file.
#[derive(Debug)]
pub struct Lexer {
    config: LexerConfig,
    diagnostics: DiagnosticBag,
    /// How many bytes the last character literal's width prefix took.
    ///
    /// Needed because a prefix is only a prefix when a quote follows, so one
    /// that is not must be un-consumed.
    last_width_prefix_len: u32,
}

impl Lexer {
    /// Creates a lexer with the given configuration.
    #[must_use]
    pub fn new(config: LexerConfig) -> Self {
        Lexer {
            config,
            diagnostics: DiagnosticBag::new(),
            last_width_prefix_len: 0,
        }
    }

    /// Returns this lexer's configuration.
    #[must_use]
    pub fn config(&self) -> LexerConfig {
        self.config
    }

    /// Takes the diagnostics accumulated so far.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics).into_vec()
    }

    /// Scans `text` (which belongs to `file`) to completion.
    ///
    /// The returned stream always ends with exactly one [`TokenKind::Eof`].
    pub fn tokenize(&mut self, text: &str, file: FileId) -> LexResult {
        let mut cursor = Cursor::new(file, text);
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token(&mut cursor);
            let is_eof = token.is_eof();
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        LexResult {
            tokens,
            diagnostics: self.take_diagnostics(),
        }
    }

    /// Scans the next token, or returns [`TokenKind::Eof`] at end of input.
    ///
    /// This never returns `None`: end of input is a token, not an absence.
    pub fn next_token(&mut self, cursor: &mut Cursor<'_>) -> Token {
        let (leading, newline_before) = if self.config.preserve_trivia {
            let (leading, newline_before) = self.scan_trivia(cursor);
            (leading, newline_before)
        } else {
            (Vec::new(), self.skip_trivia(cursor))
        };

        if cursor.is_at_end() {
            return Token::with_trivia(TokenKind::Eof, cursor.span_point(), leading)
                .with_newline_before(newline_before);
        }

        let start = cursor.offset();
        let (kind, span) = match self.scan_token(cursor) {
            Some(kind) => (kind, cursor.span_from(start)),
            None => {
                // Progress guarantee: consume exactly one character.
                cursor.bump_char();
                let span = cursor.span_from(start);
                self.report_unrecognised(span);
                (TokenKind::Unknown, span)
            }
        };
        let token = Token::with_trivia(kind, span, leading).with_newline_before(newline_before);
        // A keyword or punctuation mark is spelled the same way every time, so
        // its text is not stored; everything else -- a name, a literal, an
        // unrecognised character -- needs the source text kept, because the
        // preprocessor sees only tokens and cannot consult the file.
        match kind.spelling() {
            Some(_) => token,
            None => token.with_text(cursor.slice_from(start)),
        }
    }

    /// Consumes trivia, discarding it, and reports whether a line break was
    /// crossed.
    ///
    /// A backslash-newline does not count as a line break: it is a
    /// continuation, so the logical line goes on.
    fn skip_trivia(&mut self, cursor: &mut Cursor<'_>) -> bool {
        let mut newline_before = false;
        loop {
            let before = cursor.offset();
            while let Some(byte) = cursor.peek() {
                if !is_whitespace(byte) {
                    break;
                }
                if byte == b'\n' {
                    newline_before = true;
                }
                cursor.bump();
            }
            self.skip_line_continuation(cursor);
            self.skip_comment(cursor);
            // Stop once neither whitespace, a continuation, nor a comment was
            // consumed, or this loop would never terminate.
            if cursor.offset() == before {
                break;
            }
        }
        newline_before
    }

    /// Consumes a backslash-newline, if that is what is here, reporting whether
    /// anything was consumed.
    ///
    /// In C this is translation phase 2: the two physical lines become one
    /// logical line before anything else looks at them. Doing it in the scanner
    /// keeps the consequence -- that a macro definition can continue onto the
    /// next line -- out of the preprocessor, and it means a continuation is
    /// never reported as an unrecognised character.
    ///
    /// The backslash must be the last character of its line: `\` followed by a
    /// space and then a newline is not a continuation, as in C.
    fn skip_line_continuation(&mut self, cursor: &mut Cursor<'_>) -> bool {
        if cursor.peek() != Some(b'\\') {
            return false;
        }
        match cursor.peek_at(1) {
            Some(b'\n') => {
                cursor.bump();
                cursor.bump();
                true
            }
            Some(b'\r') if cursor.peek_at(2) == Some(b'\n') => {
                cursor.bump();
                cursor.bump();
                cursor.bump();
                true
            }
            _ => false,
        }
    }

    /// Skips one comment, if the cursor is at one.
    ///
    /// Both C forms are recognised: `//` to the end of the line, and `/* */`
    /// which does **not** nest, so the first `*/` closes it.
    ///
    /// Returns `true` if a comment was consumed.
    fn skip_comment(&mut self, cursor: &mut Cursor<'_>) -> bool {
        if cursor.peek() != Some(b'/') {
            return false;
        }
        match cursor.peek_at(1) {
            Some(b'/') => {
                // A line comment runs to the newline, which is left for the
                // whitespace loop so that line positions stay accurate.
                while let Some(byte) = cursor.peek() {
                    if byte == b'\n' {
                        break;
                    }
                    cursor.bump_char();
                }
                true
            }
            Some(b'*') => {
                cursor.bump();
                cursor.bump();
                // Unnested: the first `*/` ends it, as in C.
                while !cursor.is_at_end() {
                    if cursor.peek() == Some(b'*') && cursor.peek_at(1) == Some(b'/') {
                        cursor.bump();
                        cursor.bump();
                        break;
                    }
                    cursor.bump_char();
                }
                // An unterminated block comment runs to end of file, which is
                // what a C compiler does too. Reporting it is part of the
                // diagnostic work; the scanner stays total either way.
                true
            }
            _ => false,
        }
    }

    /// Consumes trivia, returning it for attachment to the following token and
    /// reporting whether a line break was crossed.
    fn scan_trivia(&mut self, cursor: &mut Cursor<'_>) -> (Vec<crate::token::Trivia>, bool) {
        let mut trivia = Vec::new();
        let mut newline_before = false;
        loop {
            let start = cursor.offset();
            while let Some(byte) = cursor.peek() {
                if !is_whitespace(byte) {
                    break;
                }
                if byte == b'\n' {
                    newline_before = true;
                }
                cursor.bump();
            }
            self.skip_line_continuation(cursor);
            self.skip_comment(cursor);
            if cursor.offset() == start {
                break;
            }
            trivia.push(crate::token::Trivia::Unspecified {
                span: cursor.span_from(start),
            });
        }
        (trivia, newline_before)
    }

    /// Attempts to scan one real token starting at the cursor.
    ///
    /// Returns `None` if no token can start here; the caller then consumes one
    /// character and reports an error.
    fn scan_token(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        // A raw string starts with `r`, which is also the start of an ordinary
        // identifier, so it is tried first: `r"C:\dir"` is one literal, and
        // `result` is still one name.
        if let Some(kind) = self.scan_raw_string(cursor) {
            return Some(kind);
        }
        // `f"..."` is an interpolated string. It is tried before identifiers
        // for the same reason `r"..."` is: the `f` is a prefix, not a name.
        if let Some(kind) = self.scan_fstring(cursor) {
            return Some(kind);
        }
        // A byte-string literal must be tried before identifiers, or the `b`
        // is consumed as a name and the quote is left to be scanned as an
        // ordinary string. Longest-match ordering applies to prefixes as well
        // as to operators.
        if let Some(kind) = self.scan_byte_string(cursor) {
            return Some(kind);
        }
        if let Some(kind) = self.scan_char_literal(cursor) {
            return Some(kind);
        }
        // `u16".."` is a string of `c16`, and `u16'x'` a character of `c16`.
        // Tried before identifiers so the width is not read as a name.
        if let Some(kind) = self.scan_prefixed_literal(cursor) {
            return Some(kind);
        }
        if let Some(kind) = self.scan_ident_or_keyword(cursor) {
            return Some(kind);
        }
        if let Some(kind) = self.scan_number(cursor) {
            return Some(kind);
        }
        if let Some(kind) = self.scan_string(cursor) {
            return Some(kind);
        }
        if let Some(kind) = self.scan_punctuation(cursor) {
            return Some(kind);
        }
        None
    }

    /// Scans a numeric literal.
    ///
    /// Recognised: decimal, hex, octal, and binary integers; floats with a
    /// fractional part or an exponent; `_` between two digits; and a type
    /// suffix. A leading `-` is the expression's operator, not part of the
    /// literal.
    fn scan_number(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if !cursor.peek().is_some_and(|b| b.is_ascii_digit()) {
            return None;
        }

        // A `0x`, `0o`, or `0b` prefix selects the base. The digits that follow
        // are validated against it, so `0xZZ` is not a number at all.
        let mut is_float = false;
        if cursor.peek() == Some(b'0') {
            match cursor.peek_at(1) {
                Some(b'x' | b'X') => {
                    cursor.bump();
                    cursor.bump();
                    self.scan_digits(cursor, |b| b.is_ascii_hexdigit());
                    return Some(self.scan_suffix(cursor, false));
                }
                Some(b'o' | b'O') => {
                    cursor.bump();
                    cursor.bump();
                    self.scan_digits(cursor, |b| (b'0'..=b'7').contains(&b));
                    return Some(self.scan_suffix(cursor, false));
                }
                Some(b'b' | b'B') => {
                    // `0b1010`. Only a *zero* followed by `b` is a binary
                    // prefix, so `1b` stays an integer with a suffix attempt.
                    cursor.bump();
                    cursor.bump();
                    self.scan_digits(cursor, |b| b == b'0' || b == b'1');
                    return Some(self.scan_suffix(cursor, false));
                }
                _ => {}
            }
        }

        self.scan_digits(cursor, |b| b.is_ascii_digit());

        // A `.` begins a fractional part. There is no ambiguity with member
        // access, because an integer is not a value that has members: `1.foo`
        // is not a meaningful expression in this language.
        if cursor.peek() == Some(b'.') {
            is_float = true;
            cursor.bump();
            self.scan_digits(cursor, |b| b.is_ascii_digit());
        }

        // An exponent, as in `1e10` or `2.5e-3`. Only consumed when it is well
        // formed, so `1e` followed by something else stays an integer and an
        // identifier rather than becoming a malformed float.
        if matches!(cursor.peek(), Some(b'e' | b'E')) {
            let after_e = cursor.peek_at(1);
            let has_sign = matches!(after_e, Some(b'+' | b'-'));
            let digit_at = if has_sign { cursor.peek_at(2) } else { after_e };
            if digit_at.is_some_and(|b| b.is_ascii_digit()) {
                is_float = true;
                cursor.bump();
                if has_sign {
                    cursor.bump();
                }
                self.scan_digits(cursor, |b| b.is_ascii_digit());
            }
        }

        Some(self.scan_suffix(cursor, is_float))
    }

    /// Consumes digits of the given class, allowing `_` separators between
    /// them.
    ///
    /// A separator is only valid *between* two digits: one at the start, at the
    /// end, or doubled is not part of the number, so the run stops before it.
    /// That is what makes `1_` an integer followed by an identifier rather than
    /// a malformed literal.
    fn scan_digits(&mut self, cursor: &mut Cursor<'_>, is_digit: impl Fn(u8) -> bool) {
        if !cursor.peek().is_some_and(&is_digit) {
            return;
        }
        cursor.bump();

        loop {
            match cursor.peek() {
                Some(b'_')
                    // A separator must be followed by another digit, or it ends
                    // the number instead of belonging to it.
                    if cursor.peek_at(1).is_some_and(&is_digit) =>
                {
                    cursor.bump();
                    cursor.bump();
                }
                Some(b) if is_digit(b) => {
                    cursor.bump();
                }
                _ => break,
            }
        }
    }

    /// Consumes a numeric literal's type suffix, if one is present.
    fn scan_suffix(&mut self, cursor: &mut Cursor<'_>, is_float: bool) -> TokenKind {
        let suffix_start = cursor.offset();
        if cursor.peek().is_some_and(is_ident_start) {
            cursor.eat_while(is_ident_continue);
            let suffix = cursor.slice_from(suffix_start);
            if classify_suffix(suffix, is_float) == Suffix::Unknown {
                // Not a type name, so it was never a suffix. Rewind so the text
                // lexes as its own identifier.
                cursor.rewind_to(suffix_start);
            }
        }
        if is_float {
            TokenKind::Float
        } else {
            TokenKind::Int
        }
    }

    /// Scans an identifier, returning a keyword if the text names one.
    fn scan_ident_or_keyword(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if !cursor.peek_char().is_some_and(is_ident_start_char) {
            return None;
        }
        let start = cursor.offset();
        while cursor.peek_char().is_some_and(is_ident_continue_char) {
            cursor.bump_char();
        }
        // `slice_from` borrows the source text, so keyword lookup needs no
        // allocation. The borrow ends before the next `cursor` mutation.
        let text = cursor.slice_from(start);
        Some(match crate::token::lookup_keyword(text) {
            Some(keyword) => TokenKind::Keyword(keyword),
            None => TokenKind::Ident,
        })
    }

    /// Scans a raw string literal.
    ///
    /// Two forms, and no escape processing in either:
    ///
    /// * `r"..."` -- the content runs to the next `"`.
    /// * `r#tag"..."tag#` -- the content runs to `"tag#`. The tag is what lets
    ///   the text contain a `"`: `r#q"a "quoted" word"q#`. It may be at most 64
    ///   bytes and may hold no whitespace and no `"`, which is what keeps the
    ///   opening delimiter findable.
    ///
    /// A newline inside a raw string is just a character; the literal is not
    /// tied to a line. An unterminated one runs to the end of the file, and the
    /// token is still produced so that the parser reports the mistake once.
    fn scan_raw_string(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if cursor.peek() != Some(b'r') {
            return None;
        }
        // `r"` and `r#tag"` are the two openers. `rest` is the whole remaining
        // source, so the delimiter is found by looking at the text rather than
        // by moving the cursor and moving it back.
        let rest = cursor.rest();
        let (opening, tag) = rest.strip_prefix("r\"").map(|_| (2, None)).or_else(|| {
            // `r#tag"`: a tag is a tag only if it is short and a quote
            // follows it. `r#` before anything else is the name `r` and a
            // `#`, which the caller reads as an identifier.
            let after_hash = rest.strip_prefix("r#")?;
            let length = after_hash
                .find(|character: char| character.is_whitespace() || character == '"')
                .unwrap_or(0);
            if length == 0
                || length > MAX_RAW_DELIMITER
                || after_hash.as_bytes().get(length) != Some(&b'"')
            {
                return None;
            }
            Some((2 + length + 1, Some(&after_hash[..length])))
        })?;
        for _ in 0..opening {
            cursor.bump();
        }
        // The body runs to the closing delimiter, and a newline is just a
        // character: the literal is not tied to a line.
        loop {
            if cursor.is_at_end() {
                break;
            }
            if cursor.peek() == Some(b'"') {
                match tag {
                    None => {
                        cursor.bump();
                        break;
                    }
                    Some(tag) => {
                        let closing = format!("\"{tag}#");
                        if cursor.rest().starts_with(&closing) {
                            for _ in 0..closing.len() {
                                cursor.bump();
                            }
                            break;
                        }
                    }
                }
            }
            cursor.bump_char();
        }
        Some(TokenKind::RawString)
    }

    /// Scans a double-quoted string literal.
    ///
    /// The token keeps the raw text; decoding it is `fallc-ast`'s job, so the
    /// escape rules have one home. `"""` opens the multi-line form, which may
    /// span lines and ends on three quotes; a single-quoted literal ends at the
    /// closing quote or at the end of the line.
    /// Scans an interpolated string, `f"..."`.
    ///
    /// The body is read to the closing quote like an ordinary string -- escapes
    /// still apply, and a `{` inside is found by the parser, which splits the
    /// text and the expressions. Keeping the body a plain string here means the
    /// lexer stays context-free: what a placeholder contains is an expression,
    /// and only the parser knows how to read one.
    fn scan_fstring(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if cursor.peek() != Some(b'f') || cursor.peek_at(1) != Some(b'"') {
            return None;
        }
        let start = cursor.offset();
        cursor.bump();
        // An escaped quote inside the text does not end the literal, so the
        // scan walks escapes rather than looking for the next `"`.
        cursor.bump();
        loop {
            if cursor.is_at_end() {
                self.report_unterminated(self.literal_span(cursor, start), "interpolated string");
                break;
            }
            match cursor.peek() {
                Some(b'"') => {
                    cursor.bump();
                    break;
                }
                Some(b'\\') => {
                    cursor.bump();
                    cursor.bump();
                }
                _ => {
                    cursor.bump();
                }
            }
        }
        Some(TokenKind::FString)
    }

    fn scan_string(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if cursor.peek() != Some(b'"') {
            return None;
        }
        let start = cursor.offset();
        if cursor.peek_at(1) == Some(b'"') && cursor.peek_at(2) == Some(b'"') {
            // The three quotes are the opener; the body runs to the next three.
            cursor.bump();
            cursor.bump();
            cursor.bump();
            if !self.consume_multiline_body(cursor) {
                self.report_unterminated(self.literal_span(cursor, start), "multi-line string");
            }
            return Some(TokenKind::String);
        }
        cursor.bump();
        if !self.consume_string_body(cursor) {
            self.report_unterminated(self.literal_span(cursor, start), "string");
        }
        Some(TokenKind::String)
    }

    /// Consumes the body of a `"""` literal, up to and including the closing
    /// three quotes.
    ///
    /// Newlines are part of the body. A backslash escapes the next character,
    /// so `\"""` does not close the literal; the escape is decoded later, with
    /// the rest of them.
    fn consume_multiline_body(&mut self, cursor: &mut Cursor<'_>) -> bool {
        loop {
            match cursor.peek() {
                None => return false,
                Some(b'\\') => {
                    cursor.bump();
                    if cursor.peek().is_some() {
                        cursor.bump_char();
                    }
                }
                Some(b'"')
                    if cursor.peek_at(1) == Some(b'"') && cursor.peek_at(2) == Some(b'"') =>
                {
                    cursor.bump();
                    cursor.bump();
                    cursor.bump();
                    return true;
                }
                Some(_) => {
                    cursor.bump_char();
                }
            }
        }
    }

    /// Scans a `b"..."` byte-string literal.
    ///
    /// The prefix is only a prefix when a quote follows immediately, so an
    /// identifier beginning with `b` is unaffected: `b` alone and `byte` both
    /// stay names.
    fn scan_byte_string(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if cursor.peek() != Some(b'b') || cursor.peek_at(1) != Some(b'"') {
            return None;
        }
        let start = cursor.offset();
        cursor.bump();
        cursor.bump();
        if !self.consume_string_body(cursor) {
            self.report_unterminated(self.literal_span(cursor, start), "byte string");
        }
        Some(TokenKind::ByteString)
    }

    /// Scans a string or character literal with a width prefix.
    ///
    /// `u16"..."` is a string whose elements are `c16`, and `u16'x'` a single
    /// `c16`. The prefix is only a prefix when a quote follows it.
    fn scan_prefixed_literal(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        self.last_width_prefix_len = 0;
        if !self.consume_char_width_prefix(cursor) {
            return None;
        }
        let start = cursor.offset();
        match cursor.peek() {
            Some(b'"') if cursor.peek_at(1) == Some(b'"') && cursor.peek_at(2) == Some(b'"') => {
                cursor.bump();
                cursor.bump();
                cursor.bump();
                if !self.consume_multiline_body(cursor) {
                    self.report_unterminated(
                        self.literal_span(cursor, start),
                        "multi-line string",
                    );
                }
                Some(TokenKind::String)
            }
            Some(b'"') => {
                cursor.bump();
                if !self.consume_string_body(cursor) {
                    self.report_unterminated(self.literal_span(cursor, start), "string");
                }
                Some(TokenKind::String)
            }
            Some(b'\'') => {
                cursor.bump();
                self.consume_literal_body(cursor);
                if cursor.peek() == Some(b'\'') {
                    cursor.bump();
                } else {
                    self.report_unterminated(self.literal_span(cursor, start), "character literal");
                }
                Some(TokenKind::Char)
            }
            _ => {
                // Not a prefixed literal after all: un-consume the prefix so it
                // lexes as the identifier it was.
                let length = self.last_width_prefix_len;
                cursor.rewind_to(cursor.offset().saturating_sub(length));
                self.last_width_prefix_len = 0;
                None
            }
        }
    }

    /// Scans a character literal.
    ///
    /// `'x'` is a `c8`, and a width prefix selects a wider type: `u16'x'` is a
    /// `c16` and `u32'x'` a `c32`. The prefix belongs to the literal rather
    /// than being a separate token, because it is not a name that could stand
    /// alone.
    ///
    /// The body is not decoded here. `fallc-ast` decodes it, and the type
    /// checker rejects a code point that does not fit the literal's width.
    fn scan_char_literal(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        if cursor.peek() == Some(b'\'') {
            cursor.bump();
        } else if self.consume_char_width_prefix(cursor) {
            if cursor.peek() != Some(b'\'') {
                // The prefix was not followed by a quote, so it was never a
                // prefix: rewind and let it lex as an identifier.
                cursor.rewind_to(cursor.offset().saturating_sub(self.width_prefix_len()));
                return None;
            }
            cursor.bump();
        } else {
            return None;
        }

        self.consume_literal_body(cursor);

        if cursor.peek() == Some(b'\'') {
            cursor.bump();
        }
        Some(TokenKind::Char)
    }

    /// The length of the width prefix just consumed, for rewinding.
    fn width_prefix_len(&self) -> u32 {
        self.last_width_prefix_len
    }

    /// Consumes a `u8`, `u16`, or `u32` prefix, if one is present.
    ///
    /// The width belongs to the literal, not to the type system: `u16'x'`
    /// writes a `c16`, and the characters of a string take the prefix written
    /// on the string.
    fn consume_char_width_prefix(&mut self, cursor: &mut Cursor<'_>) -> bool {
        self.last_width_prefix_len = 0;
        let start = cursor.offset();
        if cursor.peek() != Some(b'u') {
            return false;
        }
        let rest = cursor.rest();
        // Only the widths that name a character type are prefixes: a character
        // is `c8`, `c16` or `c32`, so `u64"x"` is not a literal at all rather
        // than a `c8` one with its prefix quietly dropped.
        for prefix in ["u32", "u16", "u8"] {
            if rest.starts_with(prefix) {
                for _ in 0..prefix.len() {
                    cursor.bump();
                }
                self.last_width_prefix_len = cursor.offset() - start;
                return true;
            }
        }
        false
    }

    /// Consumes the body of a character literal: one character or one escape.
    ///
    /// The body is not decoded here. Decoding belongs with the other literal
    /// decoding, so that the escape rules live in one place; the scanner only
    /// has to know where the body ends.
    fn consume_literal_body(&mut self, cursor: &mut Cursor<'_>) {
        if cursor.peek() == Some(b'\\') {
            cursor.bump();
            // The escape's kind, which may be followed by hex digits.
            let escape = cursor.bump_char();
            if escape == Some('x') {
                let mut digits = 0;
                while digits < 2 && cursor.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
                    cursor.bump();
                    digits += 1;
                }
            } else if escape == Some('u') {
                // `\u{...}` takes a braced code point.
                if cursor.eat(b'{') {
                    while cursor.peek().is_some_and(|b| b != b'}') {
                        cursor.bump_char();
                    }
                    cursor.eat(b'}');
                }
            }
            return;
        }
        // A literal quote means an empty literal, which is not valid but must
        // not consume the closing quote as the body.
        if cursor.peek().is_some_and(|b| b != b'\'') {
            cursor.bump_char();
        }
    }

    /// Consumes the body of a string literal, up to and including the closing
    /// quote or the end of the line.
    ///
    /// Nothing is decoded: the token keeps the raw text and `fallc-ast` decodes
    /// it, so the escape rules have one home. A backslash does not escape the
    /// newline, so a literal string may not span lines.
    fn consume_string_body(&mut self, cursor: &mut Cursor<'_>) -> bool {
        loop {
            match cursor.peek() {
                None | Some(b'\n') => {
                    // Unterminated: stop before the newline so the line ends
                    // cleanly for the parser, which must not swallow the rest
                    // of the file -- but say so, because a literal that ends
                    // where the line ends is a mistake and not a short string.
                    return false;
                }
                Some(b'"') => {
                    cursor.bump();
                    return true;
                }
                // A backslash makes the next character part of the literal even
                // when it is a quote: `"a\"b"` is four characters, and stopping
                // at the escaped quote would end the literal in the middle.
                Some(b'\\') => {
                    cursor.bump();
                    if cursor.peek().is_some_and(|byte| byte != b'\n') {
                        cursor.bump_char();
                    }
                }
                Some(_) => {
                    cursor.bump_char();
                }
            }
        }
    }

    /// Scans one punctuation mark.
    ///
    /// Maximal munch: the longest spelling in the table wins, so `<<=` beats
    /// `<<` beats `<`. Three characters is the longest mark in the table.
    fn scan_punctuation(&mut self, cursor: &mut Cursor<'_>) -> Option<TokenKind> {
        // Maximal munch: try the longest spelling first, so `<<=` beats `<<`
        // beats `<`. Three characters is the longest mark in the table.
        const MAX_LEN: usize = 3;
        let rest = cursor.rest();
        let rest_bytes = rest.as_bytes();

        for len in (1..=MAX_LEN).rev() {
            if rest_bytes.len() < len {
                continue;
            }
            let Some(candidate) = rest.get(..len) else {
                continue;
            };
            // Only consider a prefix that is entirely ASCII: a multi-byte
            // character must not be split, and no mark contains one.
            if !candidate.is_ascii() {
                continue;
            }
            // `.{` is the one mark made of two characters that are not a
            // conventional pairing, and it must be adjacent. Checking the
            // candidate against the table handles that for free.
            if let Some(punct) = crate::token::lookup_punctuation(candidate) {
                for _ in 0..len {
                    cursor.bump();
                }
                return Some(TokenKind::Punct(punct));
            }
        }
        None
    }

    /// The span of the literal that began at `start`.
    ///
    /// The scanner consumes the body whether or not it closes, so the end is
    /// wherever the cursor stopped -- the closing quote when there is one, and
    /// the end of the line otherwise.
    fn literal_span(&self, cursor: &Cursor<'_>, start: u32) -> Span {
        cursor.span_from(start)
    }

    /// Reports a string or character literal that is never closed.
    fn report_unterminated(&mut self, span: Span, what: &str) {
        let mut diag = Diagnostic::error(format!("this {what} is never closed"))
            .with_code(DiagnosticCode::lexer(14))
            .with_span(span)
            .with_note("a literal ends on the line it starts on, so the closing quote is missing");
        if self.config.errors_are_warnings {
            diag.severity = fallc_diag::Severity::Warning;
        }
        self.diagnostics.push(diag);
    }

    /// Reports a character that cannot begin any token.
    fn report_unrecognised(&mut self, span: Span) {
        let mut diag = Diagnostic::error("unrecognised character")
            .with_code(DiagnosticCode::lexer(1))
            .with_span(span);
        if self.config.errors_are_warnings {
            diag.severity = fallc_diag::Severity::Warning;
        }
        self.diagnostics.push(diag);
    }
}

impl Default for Lexer {
    fn default() -> Self {
        Lexer::new(LexerConfig::default())
    }
}

/// The outcome of classifying a numeric literal's suffix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Suffix {
    /// A type name this language knows.
    Known,
    /// Not a type name, so the text was not a suffix at all.
    Unknown,
}

/// Classifies a numeric literal's suffix.
///
/// An integer literal takes any integer type name, plus the two shorthand
/// forms `u` and `i`. A float takes `f`. Anything else is not a suffix, and the
/// caller rewinds so the text lexes as an identifier. A suffix names a
/// built-in type only; no user-defined type may be named by one.
fn classify_suffix(suffix: &str, is_float: bool) -> Suffix {
    if is_float {
        return match suffix {
            "f" => Suffix::Known,
            _ => Suffix::Unknown,
        };
    }
    match suffix {
        // Every integer type name, plus the two shorthands.
        // Deliberately absent: `byte`, which is not a numeric type and takes
        // no part in arithmetic, so it has no literal suffix.
        "u" | "i" | "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64"
        | "u128" | "isize" | "usize" | "int" | "uint" => Suffix::Known,
        _ => Suffix::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(text: &str) -> LexResult {
        Lexer::default().tokenize(text, FileId::from_raw(0))
    }

    #[test]
    fn empty_input_yields_only_eof() {
        let result = lex("");
        assert_eq!(result.tokens.len(), 1);
        assert!(result.tokens[0].is_eof());
        assert!(result.is_ok());
    }

    #[test]
    fn whitespace_alone_yields_only_eof() {
        let result = lex("  \n\t ");
        assert_eq!(result.tokens.len(), 1);
        assert!(result.tokens[0].is_eof());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn unsupported_input_still_terminates_and_reports() {
        // `@`, `#` and `$` are tokens of the language now -- an attribute, a
        // macro body's operator and a macro invocation -- so what is left
        // unrecognised is a character no token starts, such as a backtick.
        let result = lex("```");
        // Three unrecognised tokens plus EOF.
        assert_eq!(result.tokens.len(), 4);
        assert_eq!(result.diagnostics.len(), 3);
        assert!(!result.is_ok());
        assert!(result.tokens.last().is_some_and(Token::is_eof));
    }

    #[test]
    fn unrecognised_tokens_cover_exactly_one_character() {
        let result = lex("é!");
        assert_eq!(result.tokens[0].span, Span::new(FileId::from_raw(0), 0, 2));
        assert_eq!(result.tokens[1].span, Span::new(FileId::from_raw(0), 2, 3));
    }

    #[test]
    fn keywords_lex_as_keywords_not_identifiers() {
        use crate::token::{Keyword, TokenKind};
        let result = lex("if else while for switch case default");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword(Keyword::If),
                TokenKind::Keyword(Keyword::Else),
                TokenKind::Keyword(Keyword::While),
                TokenKind::Keyword(Keyword::For),
                TokenKind::Keyword(Keyword::Switch),
                TokenKind::Keyword(Keyword::Case),
                TokenKind::Keyword(Keyword::Default),
            ]
        );
        assert!(result.is_ok());
    }

    #[test]
    fn every_control_flow_keyword_is_recognised() {
        use crate::token::{KEYWORDS, TokenKind};
        for &(spelling, keyword) in KEYWORDS {
            let result = lex(spelling);
            assert_eq!(
                result.tokens[0].kind,
                TokenKind::Keyword(keyword),
                "`{spelling}` did not lex as a keyword"
            );
            assert!(result.is_ok(), "`{spelling}` produced a diagnostic");
        }
    }

    #[test]
    fn keyword_lookup_requires_an_exact_match() {
        // A prefix of a keyword is an identifier, not a keyword.
        assert!(crate::token::lookup_keyword("iff").is_none());
        assert!(crate::token::lookup_keyword("i").is_none());
        assert!(crate::token::lookup_keyword("Fallthrough").is_none());
    }

    #[test]
    fn identifiers_are_not_keywords() {
        let result = lex("foo bar _baz x1");
        assert!(
            result
                .tokens_without_eof()
                .iter()
                .all(|t| t.kind == TokenKind::Ident)
        );
        assert!(result.is_ok());
    }

    #[test]
    fn a_keyword_run_together_with_an_identifier_stays_one_identifier() {
        // `iffy` must not lex as `if` + `fy`.
        let result = lex("iffy");
        assert_eq!(result.tokens_without_eof().len(), 1);
        assert_eq!(result.tokens[0].kind, TokenKind::Ident);
        assert_eq!(result.tokens[0].span.len(), 4);
    }

    #[test]
    fn strings_and_punctuation_lex() {
        use crate::token::{Punctuation, TokenKind};
        let result = lex(r#"raise "type": "what";"#);
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword(crate::token::Keyword::Raise),
                TokenKind::String,
                TokenKind::Punct(Punctuation::Colon),
                TokenKind::String,
                TokenKind::Punct(Punctuation::Semi),
            ]
        );
    }

    #[test]
    fn a_switch_header_lexes() {
        use crate::token::{Keyword, Punctuation, TokenKind};
        let result = lex("switch (expr) {");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword(Keyword::Switch),
                TokenKind::Punct(Punctuation::OpenParen),
                TokenKind::Ident,
                TokenKind::Punct(Punctuation::CloseParen),
                TokenKind::Punct(Punctuation::OpenBrace),
            ]
        );
    }

    #[test]
    fn an_unterminated_string_stops_at_the_newline() {
        // An unterminated string ends at the newline: the scanner must not
        // swallow the rest of the file.
        let result = lex("\"abc\nif");
        use crate::token::{Keyword, TokenKind};
        assert_eq!(result.tokens[0].kind, TokenKind::String);
        assert_eq!(result.tokens[1].kind, TokenKind::Keyword(Keyword::If));
        assert!(
            result.tokens[1].span.lo >= 5,
            "the newline must be consumed"
        );
    }

    #[test]
    fn decimal_integers_lex() {
        use crate::token::TokenKind;
        let result = lex("0 42 007");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Int, TokenKind::Int]);
        assert!(result.is_ok());
        assert_eq!(result.tokens[0].span.len(), 1);
        assert_eq!(result.tokens[1].span.len(), 2);
    }

    #[test]
    fn a_dot_after_digits_begins_a_fraction() {
        // An integer has no members, so `1.` cannot be member access and the
        // dot is always part of the number.
        use crate::token::TokenKind;
        let result = lex("1.5");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Float]);
        assert_eq!(result.tokens[0].span.len(), 3);
    }

    #[test]
    fn a_trailing_dot_is_still_a_float() {
        use crate::token::TokenKind;
        let result = lex("1.");
        assert_eq!(result.tokens[0].kind, TokenKind::Float);
    }

    #[test]
    fn exponents_are_recognised() {
        use crate::token::TokenKind;
        for text in ["1e10", "1E10", "1e+10", "1e-10", "1.5e3", "1.5e-3"] {
            let result = lex(text);
            assert_eq!(
                result.tokens[0].kind,
                TokenKind::Float,
                "`{text}` should be a float"
            );
        }
    }

    #[test]
    fn a_malformed_exponent_is_not_swallowed() {
        // `1e` with no digits is an integer and an identifier, not a broken
        // float.
        use crate::token::TokenKind;
        let result = lex("1e");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Ident]);
    }

    #[test]
    fn float_suffixes_are_recognised() {
        use crate::token::TokenKind;
        for text in ["1.5f", "1.0f", "2.0e3f"] {
            let result = lex(text);
            assert_eq!(
                result.tokens[0].kind,
                TokenKind::Float,
                "`{text}` should be a float"
            );
        }
    }

    #[test]
    fn a_float_needs_a_dot_or_an_exponent() {
        // `1f` is not a float, as in C: the suffix selects the type of a float,
        // it does not make an integer into one. So `1f` is an integer followed
        // by a name, and `f` is not a suffix on an integer.
        use crate::token::TokenKind;
        let result = lex("1f");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Ident]);
    }

    #[test]
    fn byte_is_not_a_literal_suffix() {
        // `byte` is not numeric, so a byte literal is written with a cast.
        use crate::token::TokenKind;
        let result = lex("5byte");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Ident]);
    }

    #[test]
    fn integer_suffixes_are_recognised() {
        use crate::token::TokenKind;
        for text in [
            "5u", "5i", "5u8", "5i8", "5u16", "5i16", "5u32", "5i32", "5u64", "5i64", "5u128",
            "5i128", "5usize", "5isize", "5int", "5uint",
        ] {
            let result = lex(text);
            let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
            assert_eq!(
                kinds,
                vec![TokenKind::Int],
                "`{text}` should be one integer"
            );
        }
    }

    #[test]
    fn a_non_type_suffix_is_an_identifier_not_a_suffix() {
        // `5x` is an integer followed by a name. Treating `x` as a suffix
        // would silently change the meaning of a valid expression.
        use crate::token::TokenKind;
        let result = lex("5x");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Ident]);
        // And the identifier starts right after the digits.
        assert_eq!(result.tokens[1].span.lo, 1);
    }

    #[test]
    fn a_number_does_not_swallow_a_following_identifier() {
        use crate::token::TokenKind;
        let result = lex("1x");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Int, TokenKind::Ident]);
    }

    #[test]
    fn dot_brace_is_one_token_when_adjacent() {
        use crate::token::{Punctuation, TokenKind};
        let result = lex(".{");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Punct(Punctuation::DotBrace)]);
        assert_eq!(result.tokens[0].span.len(), 2);
    }

    #[test]
    fn a_space_splits_dot_and_brace() {
        // `.{` is a single token, so whitespace must break it apart rather
        // than being tolerated.
        use crate::token::{Punctuation, TokenKind};
        let result = lex(". {");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Punct(Punctuation::Dot),
                TokenKind::Punct(Punctuation::OpenBrace)
            ]
        );
    }

    #[test]
    fn dot_brace_does_not_disturb_member_access() {
        use crate::token::{Punctuation, TokenKind};
        let result = lex("e.name()");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds[1], TokenKind::Punct(Punctuation::Dot));
        assert_eq!(kinds[2], TokenKind::Ident);
    }

    #[test]
    fn a_keyword_cannot_be_used_as_a_member_name() {
        // `type` is reserved, so `e.type` is not `e` followed by a name. That
        // is the cost of making a word a keyword, and it is why the decision to
        // reserve one is worth making deliberately.
        use crate::token::{Keyword, Punctuation, TokenKind};
        let result = lex("e.type");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(kinds[1], TokenKind::Punct(Punctuation::Dot));
        assert_eq!(kinds[2], TokenKind::Keyword(Keyword::Type));
    }

    #[test]
    fn auto_is_a_keyword() {
        use crate::token::{Keyword, TokenKind};
        let result = lex("auto");
        assert_eq!(result.tokens[0].kind, TokenKind::Keyword(Keyword::Auto));
        // But `automatic` is an identifier.
        let result = lex("automatic");
        assert_eq!(result.tokens[0].kind, TokenKind::Ident);
    }

    #[test]
    fn fn_declarations_lex() {
        use crate::token::{Keyword, Punctuation, TokenKind};
        let result = lex("fn main() int { return 0; }");
        let kinds: Vec<_> = result.tokens_without_eof().iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword(Keyword::Fn),
                TokenKind::Ident,
                TokenKind::Punct(Punctuation::OpenParen),
                TokenKind::Punct(Punctuation::CloseParen),
                TokenKind::Ident, // `int`, a built-in type name, not a keyword
                TokenKind::Punct(Punctuation::OpenBrace),
                TokenKind::Keyword(Keyword::Return),
                TokenKind::Int,
                TokenKind::Punct(Punctuation::Semi),
                TokenKind::Punct(Punctuation::CloseBrace),
            ]
        );
        assert!(result.is_ok());
    }

    #[test]
    fn eof_span_is_a_point_at_end_of_input() {
        let result = lex("ab");
        let eof = result.tokens.last().expect("stream ends with eof");
        assert_eq!(eof.span, Span::point(FileId::from_raw(0), 2));
    }

    #[test]
    fn token_text_is_recorded_for_kinds_without_a_fixed_spelling() {
        // The preprocessor sees tokens and never the source, so a name, a
        // number, and a string must carry their spelling on the token.
        let result = lex("x 1 \"s\" ;");
        assert_eq!(result.tokens[0].spelling(), Some("x"));
        assert_eq!(result.tokens[1].spelling(), Some("1"));
        assert_eq!(result.tokens[2].spelling(), Some("\"s\""));
        // Punctuation is implied by the kind, so nothing is stored for it.
        assert_eq!(result.tokens[3].text, None);
        assert_eq!(result.tokens[3].spelling(), Some(";"));
    }

    #[test]
    fn a_newline_before_a_token_is_recorded() {
        // A macro definition's body ends at the end of its logical line, and
        // the scanner is the only stage that ever sees the whitespace.
        let result = lex("a\nb");
        assert!(!result.tokens[0].newline_before);
        assert!(result.tokens[1].newline_before);
    }

    #[test]
    fn a_backslash_newline_is_a_continuation_not_a_token() {
        let result = lex("a\\\nb\nc");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let kinds: Vec<_> = result
            .tokens_without_eof()
            .iter()
            .map(|token| token.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![TokenKind::Ident, TokenKind::Ident, TokenKind::Ident]
        );
        // The continuation keeps the logical line going; the real newline does
        // not.
        assert!(!result.tokens[1].newline_before);
        assert!(result.tokens[2].newline_before);
    }

    #[test]
    fn trivia_is_attached_when_requested() {
        let mut lexer = Lexer::new(LexerConfig {
            preserve_trivia: true,
            errors_are_warnings: true,
        });
        let result = lexer.tokenize("  x", FileId::from_raw(0));
        let first = &result.tokens[0];
        assert_eq!(first.leading.len(), 1);
        assert_eq!(
            first.leading[0].span(),
            Span::new(FileId::from_raw(0), 0, 2)
        );
    }
}

#[cfg(test)]
mod raw_string_tests {
    use super::*;
    use fallc_span::FileId;

    fn kinds(text: &str) -> Vec<TokenKind> {
        Lexer::default()
            .tokenize(text, FileId::from_raw(0))
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn a_raw_string_ends_at_the_next_quote_and_holds_no_escapes() {
        let tokens = kinds("r\"C:\\dir\\n\"");
        assert_eq!(tokens, vec![TokenKind::RawString, TokenKind::Eof]);
    }

    #[test]
    fn a_delimited_raw_string_may_contain_quotes() {
        let tokens = kinds("r#q\"a \"quoted\" word\"q#");
        assert_eq!(tokens, vec![TokenKind::RawString, TokenKind::Eof]);
    }

    #[test]
    fn a_tag_must_be_followed_by_a_quote_to_be_a_tag() {
        // `r#` with no quote is the name `r` and a `#`: the `#` begins no
        // token here, so the stream says so rather than inventing a string.
        let tokens = kinds("r#tag");
        assert_eq!(tokens.first(), Some(&TokenKind::Ident));
        assert_eq!(tokens.last(), Some(&TokenKind::Eof));
    }

    #[test]
    fn an_identifier_beginning_with_r_is_still_a_name() {
        let tokens = kinds("result");
        assert_eq!(tokens, vec![TokenKind::Ident, TokenKind::Eof]);
    }
}
