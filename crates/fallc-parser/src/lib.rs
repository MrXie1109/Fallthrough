//! The Fallthrough parser: token stream in, AST out.
//!
//! # Hand-written, recursive descent
//!
//! The parser is a hand-written recursive-descent parser with an explicit
//! token buffer for lookahead ([`TokenStream`]) and Pratt-style binding-power
//! tables for expressions ([`precedence`]). This is the shape that survives
//! language evolution best: error messages stay specific, and new syntax is
//! usually a new function rather than a table rewrite.
//!
//! # Error recovery
//!
//! The parser never returns `Result` for syntax errors. It reports a
//! diagnostic and recovers to a synchronisation point, which is what lets one
//! run surface several independent syntax errors. Every recovery routine must
//! guarantee progress: consuming at least one token before returning.
//!
//! # Panic-free
//!
//! Malformed input must not panic. The only permitted panics are on internal
//! invariant violations, which are reported as [`fallc_diag::Severity::Bug`]
//! diagnostics instead wherever it is possible to keep going.
//!
//! `parse_file` parses a whole source file into a [`fallc_ast::Ast`], reporting
//! syntax errors as diagnostics and recovering to a synchronisation point so
//! that one run can surface several.

#![forbid(unsafe_code)]

pub mod parser;
pub mod precedence;
pub mod stream;

pub use parser::{ParseResult, Parser, ParserConfig};
pub use precedence::{Assoc, BindingPower, Fixity};
pub use stream::TokenStream;
