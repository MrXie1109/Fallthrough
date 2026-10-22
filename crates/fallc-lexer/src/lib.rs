//! The Fallthrough lexer: turns source text into a stream of [`Token`]s.
//!
//! # Shape of this crate
//!
//! The lexer is deliberately split so that the *vocabulary* of the language
//! (which keywords, which operators, which literal forms exist) lives in
//! [`token`] as data, while the *mechanism* (scanning, literal decoding,
//! error recovery) lives in [`cursor`], [`lexer`], and [`literal`].
//!
//! [`macros`] is the one pass that runs *after* scanning: C-style macros are a
//! rewrite of tokens into tokens, so [`expand_macros`] takes the stream
//! [`Lexer`] produced, removes the definitions, and expands the invocations.
//! It needs no source text because a token carries what the preprocessor has to
//! know: its spelling and whether a line break preceded it.
//!
//! The vocabulary is the language's: [`TokenKind`], the keyword table, the
//! punctuation and operator set, every literal form, and trivia. It lives as
//! data in [`token`], so nothing that scans text has to know a spelling.
//!
//! # Contract
//!
//! * The lexer never panics on malformed input and never loops forever: every
//!   error consumes at least one byte.
//! * Lexing is lossless in the sense that concatenating the source slices of
//!   all tokens (including trivia, if enabled) reproduces the input.
//! * A lexical error produces a diagnostic *and* a token, so the parser always
//!   has a token to work with and can continue.
//!
//! # Example
//!
//! ```no_run
//! use fallc_lexer::{Lexer, LexerConfig};
//!
//! // Empty input yields a stream containing just EOF.
//! let mut lexer = Lexer::new(LexerConfig::default());
//! let output = lexer.tokenize("", fallc_span::FileId::from_raw(0));
//! assert_eq!(output.tokens.len(), 1);
//! assert!(output.tokens[0].is_eof());
//! ```

#![forbid(unsafe_code)]

pub mod cursor;
pub mod lexer;
pub mod literal;
pub mod macros;
pub mod token;

pub use cursor::Cursor;
pub use lexer::{LexResult, Lexer, LexerConfig};
pub use literal::Literal;
pub use macros::{MacroDiagnostic, expand_macros};
pub use token::{Delimiter, Keyword, Punctuation, Token, TokenKind, Trivia};
