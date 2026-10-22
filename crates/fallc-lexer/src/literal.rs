//! Literal text carriage.
//!
//! The lexer recognises the *shape* of a literal — a string, a raw string, an
//! integer, a float — and records the raw source text; decoding that text into
//! a value belongs to `fallc-ast`, which the checker and the code generator
//! both depend on. Nothing in this module interprets the text.
//!
//! [`Literal`] is a placeholder carrier: the token kind already distinguishes
//! the forms the parser needs, so no later stage asks the lexer for a decoded
//! value, and only the raw-text variant exists.

use fallc_span::Span;

/// A literal token's content, kept as raw source text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Literal {
    /// A literal carried as its exact source text, including any quotes or
    /// suffix. The variant name records that the lexer does not decode it.
    Unspecified {
        /// The literal's exact source text, including any quotes or suffix.
        raw: String,
        /// Where the literal came from.
        span: Span,
    },
}

impl Literal {
    /// Returns the raw source text of this literal.
    #[must_use]
    pub fn raw(&self) -> &str {
        match self {
            Literal::Unspecified { raw, .. } => raw,
        }
    }

    /// Returns the span of this literal.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Literal::Unspecified { span, .. } => *span,
        }
    }
}

/// Why a literal could not be carried.
///
/// Only the placeholder exists: decoding happens in `fallc-ast`, which reports
/// its own failures, so the lexer has not needed a recovery taxonomy.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum LiteralError {
    /// Nothing was decoded, because the lexer does not decode literals.
    Unspecified,
}

impl LiteralError {
    /// A short message suitable for a diagnostic.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self {
            LiteralError::Unspecified => "literal forms are not yet specified",
        }
    }
}

/// Carries `raw` into a [`Literal`] unchanged.
///
/// The signature allows a decoding failure, but this module does not decode:
/// the text is kept verbatim for `fallc-ast`.
pub fn decode(raw: &str, span: Span) -> Result<Literal, LiteralError> {
    Ok(Literal::Unspecified {
        raw: raw.to_owned(),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    #[test]
    fn unspecified_literal_round_trips_its_text() {
        let span = Span::new(FileId::from_raw(0), 0, 3);
        let lit = decode("123", span).expect("placeholder decode cannot fail");
        assert_eq!(lit.raw(), "123");
        assert_eq!(lit.span(), span);
    }
}
