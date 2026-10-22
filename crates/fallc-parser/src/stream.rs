//! The parser's token buffer.
//!
//! The parser needs more than one token of lookahead (to disambiguate
//! `<` starting a type argument from a less-than operator, for instance) and
//! it needs to backtrack on occasion. Rather than re-lexing, the parser buffers
//! tokens and remembers how far it has committed:
//!
//! * [`TokenStream::advance`] consumes a token permanently.
//! * [`TokenStream::checkpoint`] plus [`TokenStream::rewind`] implement
//!   speculative parsing.
//! * [`TokenStream::peek`] inspects the buffer without consuming.
//!
//! Because the whole file is lexed up front by the driver, the buffer never
//! needs to ask the lexer for more input, which keeps this type simple. If
//! lazy lexing is ever wanted, only this type changes.

use fallc_lexer::{Punctuation, Token, TokenKind};
use fallc_span::{FileId, Span};

/// A buffered view over a lexed token stream.
#[derive(Clone, Debug)]
pub struct TokenStream {
    tokens: Vec<Token>,
    pos: usize,
}

impl TokenStream {
    /// Creates a stream over `tokens`.
    ///
    /// If `tokens` is empty or does not end in EOF, a synthetic EOF is
    /// appended, so that [`TokenStream::peek`] is never `None`.
    #[must_use]
    pub fn new(mut tokens: Vec<Token>) -> Self {
        if !tokens.last().is_some_and(Token::is_eof) {
            tokens.push(Token::new(TokenKind::Eof, Span::SYNTHETIC));
        }
        TokenStream { tokens, pos: 0 }
    }

    /// Returns the token `n` positions ahead of the cursor.
    ///
    /// Lookahead beyond the end of input yields the EOF token, so this never
    /// returns `None`.
    #[must_use]
    pub fn peek(&self, n: usize) -> &Token {
        let idx = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    /// Returns the kind of the token `n` positions ahead.
    #[must_use]
    pub fn peek_kind(&self, n: usize) -> TokenKind {
        self.peek(n).kind
    }

    /// Returns the current token's kind.
    #[must_use]
    pub fn current_kind(&self) -> TokenKind {
        self.peek(0).kind
    }

    /// Returns the current token's span.
    #[must_use]
    pub fn current_span(&self) -> Span {
        self.peek(0).span
    }

    /// Returns `true` if the current token has kind `kind`.
    #[must_use]
    pub fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == kind
    }

    /// Returns `true` if the current token is EOF.
    #[must_use]
    /// Returns `true` if the current token is this punctuation.
    ///
    /// The common case by a wide margin, since most of the grammar's decisions
    /// are about punctuation.
    pub fn at_punct(&self, punct: Punctuation) -> bool {
        self.at(TokenKind::Punct(punct))
    }

    pub fn at_eof(&self) -> bool {
        self.peek(0).is_eof()
    }

    /// Consumes and returns the current token.
    ///
    /// Consuming past EOF repeatedly yields EOF, so callers cannot run off the
    /// end of the buffer.
    pub fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    /// Consumes the current token if it has kind `kind`.
    pub fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.advance())
        } else {
            None
        }
    }

    /// Returns `true` if the current token can close an angle bracket.
    ///
    /// `>>` closes two nested argument lists and the lexer reads it as one
    /// token, so the parser has to be able to take it one `>` at a time.
    #[must_use]
    pub fn at_gt(&self) -> bool {
        matches!(
            self.current_kind(),
            TokenKind::Punct(Punctuation::Gt | Punctuation::Shr)
        )
    }

    /// Consumes one `>`, splitting `>>` if that is what is there.
    ///
    /// `Array<Array<int>>` ends two lists with no space between them, and
    /// demanding one would make the spelling of a type depend on how the lexer
    /// groups characters. What is consumed is one `>`; a `>>` becomes a single
    /// `>` whose span covers the character that is left, so the rest of the
    /// stream still reads `>`.
    pub fn split_gt(&mut self) -> Option<Token> {
        let token = self.tokens[self.pos].clone();
        let TokenKind::Punct(punct) = token.kind else {
            return None;
        };
        match punct {
            Punctuation::Gt => Some(self.advance()),
            Punctuation::Shr => {
                let file = token.span.file;
                let lo = token.span.lo;
                let hi = token.span.hi;
                let one = Token::new(
                    TokenKind::Punct(Punctuation::Gt),
                    Span::new(file, lo, lo + 1),
                );
                self.tokens[self.pos] = Token::new(
                    TokenKind::Punct(Punctuation::Gt),
                    Span::new(file, lo + 1, hi),
                );
                Some(one)
            }
            _ => None,
        }
    }

    /// Returns the index of the current token.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Records the current position for a later [`TokenStream::rewind`].
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { pos: self.pos }
    }

    /// Restores a previously recorded position.
    ///
    /// Rewinding to a checkpoint discards any tokens consumed since. This is
    /// how speculative parsing backs out; diagnostics emitted during the
    /// abandoned attempt must be rolled back separately (see
    /// [`crate::Parser::speculate`]).
    pub fn rewind(&mut self, checkpoint: Checkpoint) {
        self.pos = checkpoint.pos.min(self.tokens.len() - 1);
    }

    /// Returns the span covering everything consumed since `checkpoint`.
    #[must_use]
    pub fn span_since(&self, checkpoint: Checkpoint) -> Span {
        let start = self
            .tokens
            .get(checkpoint.pos)
            .map_or(Span::SYNTHETIC, |t| t.span);
        let end = self.tokens[self.pos.min(self.tokens.len() - 1)].span;
        start.to(end)
    }

    /// Returns every token, including the trailing EOF.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Returns the file these tokens came from.
    #[must_use]
    pub fn file(&self) -> FileId {
        self.tokens[0].span.file
    }

    /// Returns the source text of the token `n` positions ahead, if the caller
    /// supplies the file's text.
    #[must_use]
    pub fn text_at<'src>(&self, n: usize, source: &'src str) -> Option<&'src str> {
        let span = self.peek(n).span;
        source.get(span.lo as usize..span.hi as usize)
    }
}

/// A saved parse position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Checkpoint {
    pos: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    fn stream(kinds: &[TokenKind]) -> TokenStream {
        let tokens = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                let lo = i as u32;
                Token::new(kind, Span::new(FileId::from_raw(0), lo, lo + 1))
            })
            .collect();
        TokenStream::new(tokens)
    }

    #[test]
    fn eof_is_appended_when_missing() {
        let s = stream(&[TokenKind::Unknown]);
        assert_eq!(s.tokens().len(), 2);
        assert!(s.tokens()[1].is_eof());
    }

    #[test]
    fn eof_is_not_duplicated() {
        let s = stream(&[TokenKind::Unknown, TokenKind::Eof]);
        assert_eq!(s.tokens().len(), 2);
    }

    #[test]
    fn peek_past_end_yields_eof() {
        let s = stream(&[TokenKind::Eof]);
        assert!(s.peek(10).is_eof());
    }

    #[test]
    fn advance_past_eof_is_sticky() {
        let mut s = stream(&[TokenKind::Eof]);
        assert!(s.advance().is_eof());
        assert!(s.advance().is_eof());
        assert_eq!(s.position(), 0);
    }

    #[test]
    fn rewind_restores_position() {
        let mut s = stream(&[TokenKind::Unknown, TokenKind::Unknown]);
        let cp = s.checkpoint();
        s.advance();
        assert_eq!(s.position(), 1);
        s.rewind(cp);
        assert_eq!(s.position(), 0);
    }

    #[test]
    fn span_since_covers_consumed_tokens() {
        // Three tokens: two real ones plus the appended EOF.
        let mut s = stream(&[TokenKind::Unknown, TokenKind::Unknown, TokenKind::Eof]);
        let cp = s.checkpoint();
        assert_eq!(s.span_since(cp), Span::new(FileId::from_raw(0), 0, 1));
        s.advance();
        assert_eq!(s.span_since(cp), Span::new(FileId::from_raw(0), 0, 2));
        s.advance();
        assert_eq!(s.span_since(cp), Span::new(FileId::from_raw(0), 0, 3));
    }
}
