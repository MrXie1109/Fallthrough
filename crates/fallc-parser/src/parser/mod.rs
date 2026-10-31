//! The recursive-descent parser.
//!
//! # Structure
//!
//! The eventual grammar is expected to have three layers, so the parser is
//! organised into three groups of methods:
//!
//! ```text
//! parse_file
//!   └── parse_item            (top-level declarations)
//!         └── parse_stmt      (statements and blocks)
//!               └── parse_expr (Pratt binding-power loop)
//! ```
//!
//! Only the entry points and the recovery machinery are implemented. Every
//! grammar-specific method returns a placeholder and is documented with what
//! it is waiting for.
//!
//! # Speculative parsing
//!
//! [`Parser::speculate`] runs a closure and rolls back both the token position
//! *and* any diagnostics it produced. Without the diagnostic rollback,
//! trying-and-failing would emit phantom errors; with it, the parser can use
//! backtracking freely where lookahead is awkward.

use fallc_ast::{
    ArrayType, AssignExpr, Ast, Attr, BinaryExpr, BinaryOp, BindStmt, BindingType, BlockId,
    CallExpr, CastExpr, CommaExpr, ConditionalExpr, ConstItem, ConstantMember, DesignatedInit,
    ElseBranch, EnumDecl, EnumMember, ExprId, ExprKind, ExprStmt, FieldDecl, FieldExpr, FoldExpr,
    FoldKind, ForCStmt, FunctionItem, IfStmt, ImportItem, ImportedMembers, ImportedName,
    IncDecExpr, IncDecOp, IndexExpr, Initializer, ItemId, ItemKind, LinkItem, LinkKind,
    LinkLocation, LoopStmt, NamedArgument, NamedType, PackRef, Param, RaiseStmt, ReturnStmt,
    StaticAssert, StmtId, StmtKind, SwitchArm, SwitchStmt, TemplateArgumentSyntax, TemplateParam,
    TemplateParamKind, TryStmt, TypeDecl, TypeDeclKind, TypeOfType, TypeSyntax, TypedInitExpr,
    UnaryExpr, UnaryOp,
};
use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_lexer::{Keyword, Punctuation};
use fallc_lexer::{Token, TokenKind};
use fallc_span::Span;

use crate::precedence::BindingPower;
use crate::stream::{Checkpoint, TokenStream};

/// Knobs that change how the parser behaves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParserConfig {
    /// Maximum nesting depth before the parser gives up.
    ///
    /// Recursive descent uses the native stack, so unbounded nesting in the
    /// input must be bounded here or a deeply nested file becomes a stack
    /// overflow — a crash, not a diagnostic. Hitting the limit produces a
    /// diagnostic and recovery rather than a crash.
    pub max_recursion_depth: u32,
}

impl Default for ParserConfig {
    fn default() -> Self {
        ParserConfig {
            max_recursion_depth: 256,
        }
    }
}

/// The result of parsing one file.
#[derive(Clone, Debug)]
pub struct ParseResult {
    /// The tree. Well-formed even when `diagnostics` is non-empty, because the
    /// parser recovers rather than aborting.
    pub ast: Ast,
    /// What went wrong.
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseResult {
    /// Returns `true` if no fatal diagnostic was reported.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_fatal)
    }
}

/// The parser state for one file.
#[derive(Debug)]
pub struct Parser {
    config: ParserConfig,
    diagnostics: DiagnosticBag,
    /// The file's text, used to recover identifier and literal spelling.
    ///
    /// Tokens carry spans, not text, so the parser needs the source to name
    /// things. It is optional so that the parser can still be driven without
    /// it (the token kinds remain correct; only spelling is lost).
    source: Option<String>,
    depth: u32,
    /// Set once the recursion limit has been hit, to avoid a diagnostic per
    /// frame while unwinding.
    poisoned: bool,
    /// Whether a top-level `->` ends the expression rather than opening a
    /// member access.
    ///
    /// Set while a trait requirement is parsed, because there `->` says what
    /// the operation must yield: `*it -> !void`. The two readings cannot be
    /// told apart by what follows the arrow -- `bool` is both a type and a
    /// possible field name -- so the reading is decided by where the expression
    /// is written. Parentheses put the ordinary reading back, which is how a
    /// requirement that really does reach through a pointer is written:
    /// `(*it)->field`.
    result_arrow_next: bool,
    /// Whether the expression being parsed is a requirement.
    ///
    /// A requirement states a method by calling it, and what a call's arguments
    /// *are* there is the parameter types -- `it.add(*int) -> int`. Reading
    /// them with the type grammar is what makes a pointer, an array or a
    /// function type writable; a value that is not a type falls back to the
    /// ordinary reading.
    in_requirement: bool,
    /// How many template parameter headers the expression parser is inside.
    ///
    /// A `>` closes the header, so it must not be read as a comparison: the
    /// default in `template <n int = 4>` ends at the `>`, and `4 >` is not an
    /// expression.
    in_template_header: u32,
}

mod expressions;
mod functions;
mod items;
mod recovery;
mod statements;

/// Returns the bit width a character literal's prefix names, if any.
///
/// `u16'x'` is a `c16`; a bare `'x'` is a `c8`. The prefix is part of the
/// token's text because it is not a type name that could stand alone.
fn char_literal_width(text: &str) -> Option<u32> {
    match text.find('\'')? {
        0 => None,
        width => match &text[..width] {
            "u8" => Some(8),
            "u16" => Some(16),
            "u32" => Some(32),
            _ => None,
        },
    }
}

/// Reads the value of an integer literal's digits.
///
/// Only a requirement's `-> 7` needs this: there the literal is a value to
/// compare against, not an expression to lower.
fn integer_literal_value(digits: &str) -> Option<i64> {
    let digits = digits.replace('_', "");
    let (radix, body) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest)
    } else {
        (10, digits.as_str())
    };
    i64::from_str_radix(body, radix).ok()
}

/// Splits a numeric literal's digits from its type suffix.
///
/// `5u8` splits into `5` and `u8`; `1.5f` into `1.5` and `f`. A literal with no
/// suffix returns `None`, which means the default type for its kind.
fn split_numeric_suffix(text: &str) -> (&str, Option<&str>) {
    // Where the digits end depends on the base. In `0xFF` the letters are
    // digits, so splitting at the first letter would leave `x` and `FF` as a
    // suffix. In a decimal or float literal any letter begins the suffix.
    let digits_end = if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
    {
        2 + rest
            .find(|c: char| !c.is_ascii_hexdigit() && c != '_')
            .unwrap_or(rest.len())
    } else if let Some(rest) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
        2 + rest
            .find(|c: char| !('0'..='7').contains(&c) && c != '_')
            .unwrap_or(rest.len())
    } else if let Some(rest) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        2 + rest
            .find(|c: char| c != '0' && c != '1' && c != '_')
            .unwrap_or(rest.len())
    } else {
        text.find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(text.len())
    };

    let (digits, suffix) = text.split_at(digits_end);
    if suffix.is_empty() {
        (digits, None)
    } else {
        (digits, Some(suffix))
    }
}

/// Maps a token to the binary operator it denotes.
///
/// Returns `None` for tokens that are not operators, which is how the Pratt
/// loop knows to stop.
#[must_use]
pub fn binary_op_of(kind: TokenKind) -> Option<BinaryOp> {
    let punct = kind.punct()?;
    binary_op_of_punct(punct)
}

/// Maps a punctuation mark to the binary operator it denotes.
#[must_use]
pub fn binary_op_of_punct(punct: Punctuation) -> Option<BinaryOp> {
    Some(match punct {
        Punctuation::Plus => BinaryOp::Add,
        Punctuation::Minus => BinaryOp::Sub,
        Punctuation::Star => BinaryOp::Mul,
        Punctuation::Slash => BinaryOp::Div,
        Punctuation::Percent => BinaryOp::Rem,
        Punctuation::StarStar => BinaryOp::Pow,
        Punctuation::EqEq => BinaryOp::Eq,
        Punctuation::BangEq => BinaryOp::Ne,
        Punctuation::Lt => BinaryOp::Lt,
        Punctuation::Le => BinaryOp::Le,
        Punctuation::Gt => BinaryOp::Gt,
        Punctuation::Ge => BinaryOp::Ge,
        Punctuation::Cmp => BinaryOp::Cmp,
        Punctuation::AmpAmp => BinaryOp::LogicalAnd,
        Punctuation::PipePipe => BinaryOp::LogicalOr,
        Punctuation::Amp => BinaryOp::BitAnd,
        Punctuation::Pipe => BinaryOp::BitOr,
        Punctuation::Caret => BinaryOp::BitXor,
        Punctuation::Shl => BinaryOp::Shl,
        Punctuation::Shr => BinaryOp::Shr,
        _ => return None,
    })
}

/// Maps a token to the prefix operator it denotes.
#[must_use]
pub fn unary_op_of(kind: TokenKind) -> Option<UnaryOp> {
    let punct = kind.punct()?;
    Some(match punct {
        Punctuation::Minus => UnaryOp::Neg,
        Punctuation::Plus => UnaryOp::Plus,
        Punctuation::Bang => UnaryOp::Not,
        Punctuation::Tilde => UnaryOp::BitNot,
        // Address-of and dereference. Both are also binary operators, so the
        // parser decides between the two readings by position: a prefix
        // position uses these, an infix position uses the binary forms.
        Punctuation::Amp => UnaryOp::AddressOf,
        Punctuation::Star => UnaryOp::Deref,
        _ => return None,
    })
}

/// Maps a token to the increment or decrement it denotes.
#[must_use]
pub fn incdec_op_of(kind: TokenKind) -> Option<IncDecOp> {
    match kind.punct()? {
        Punctuation::PlusPlus => Some(IncDecOp::Increment),
        Punctuation::MinusMinus => Some(IncDecOp::Decrement),
        _ => None,
    }
}

impl Default for Parser {
    fn default() -> Self {
        Parser::new(ParserConfig::default())
    }
}

/// Convenience: parse source text in one call.
///
/// Lexes and parses, merging both stages' diagnostics in source order.
#[must_use]
pub fn parse_source(text: &str, file: fallc_span::FileId) -> ParseResult {
    let mut lexer = fallc_lexer::Lexer::default();
    let lexed = lexer.tokenize(text, file);
    let mut stream = TokenStream::new(lexed.tokens);
    let mut parser = Parser::default();
    let mut result = parser.parse_file(&mut stream);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.append(&mut result.diagnostics);
    result.diagnostics = diagnostics;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_ast::{ItemKind, StmtKind, Unresolved};
    use fallc_span::FileId;

    fn parse(text: &str) -> ParseResult {
        parse_source(text, FileId::from_raw(0))
    }

    #[test]
    fn empty_input_parses_to_empty_ast() {
        let result = parse("");
        assert!(result.is_ok());
        assert!(result.ast.items.is_empty());
    }

    #[test]
    fn unspecified_grammar_consumes_nothing_yet() {
        // The grammar is unspecified, so every input yields an empty tree and
        // any lexical complaints that the input itself earned.
        let result = parse("anything at all");
        assert!(result.ast.items.is_empty());
    }

    #[test]
    fn lexer_diagnostics_survive_into_parser_result() {
        // A backtick is not a token; `@` is, since it begins an attribute.
        let result = parse("`");
        assert!(!result.is_ok());
    }

    #[test]
    fn expect_reports_and_does_not_consume() {
        let mut stream = TokenStream::new(vec![Token::new(
            TokenKind::Eof,
            Span::point(FileId::from_raw(0), 0),
        )]);
        let mut parser = Parser::default();
        assert!(parser.expect(&mut stream, TokenKind::Unknown).is_none());
        assert_eq!(parser.diagnostics().len(), 1);
        assert!(stream.at_eof());
    }

    #[test]
    fn speculate_rolls_back_tokens_and_diagnostics() {
        let mut stream = TokenStream::new(vec![Token::new(
            TokenKind::Unknown,
            Span::new(FileId::from_raw(0), 0, 1),
        )]);
        let mut parser = Parser::default();
        let outcome = parser.speculate(&mut stream, |p, s| {
            s.advance();
            p.report_unexpected(s, "something");
            None::<()>
        });
        assert!(outcome.is_none());
        assert_eq!(stream.position(), 0);
        assert!(parser.diagnostics().is_empty());
    }

    #[test]
    fn recursion_limit_is_reported_once() {
        let stream = TokenStream::new(vec![Token::new(
            TokenKind::Eof,
            Span::point(FileId::from_raw(0), 0),
        )]);
        let mut parser = Parser::new(ParserConfig {
            max_recursion_depth: 2,
        });
        let _ = parser.enter(&stream);
        let _ = parser.enter(&stream);
        assert!(!parser.enter(&stream));
        assert_eq!(parser.diagnostics().len(), 1);
        parser.leave();
        parser.leave();
        parser.leave();
        // Once fully unwound, the limit can be hit again. The bag itself does
        // not repeat a diagnostic it already holds at the same place, so the
        // limit is checked by its answer rather than by counting sentences.
        let _ = parser.enter(&stream);
        let _ = parser.enter(&stream);
        assert!(!parser.enter(&stream));
        assert_eq!(parser.diagnostics().len(), 1);
    }

    #[test]
    fn recovery_always_makes_progress() {
        let mut stream = TokenStream::new(vec![
            Token::new(TokenKind::Unknown, Span::new(FileId::from_raw(0), 0, 1)),
            Token::new(TokenKind::Unknown, Span::new(FileId::from_raw(0), 1, 2)),
        ]);
        let mut parser = Parser::default();
        parser.recover_to_item(&mut stream);
        assert_eq!(stream.position(), 1);
    }

    #[test]
    fn statements_placeholder_returns_none() {
        let mut ast = Ast::new();
        let mut stream = TokenStream::new(vec![Token::new(
            TokenKind::Eof,
            Span::point(FileId::from_raw(0), 0),
        )]);
        let mut parser = Parser::default();
        assert!(parser.parse_stmt(&mut stream, &mut ast).is_none());
        assert!(parser.parse_item(&mut stream, &mut ast).is_none());
        assert!(parser.parse_block(&mut stream, &mut ast).is_none());
    }

    #[test]
    fn the_statement_and_item_enums_still_carry_a_fallback() {
        // `Unresolved` remains for constructs whose grammar is not yet
        // specified, so the enums are still extensible without breaking every
        // match in the compiler.
        let stmt = StmtKind::Unresolved(Unresolved::new("", Span::SYNTHETIC));
        let item = ItemKind::Unresolved(Unresolved::new("", Span::SYNTHETIC));
        let _ = (stmt, item);
    }
}

/// The prefixes a declaration may carry before its keyword.
///
/// `export` is a keyword written first, and the attributes that used to be
/// keywords -- `@extern_C`, `@constexpr` and the rest -- follow it. Grouped
/// rather than passed separately because they travel together: every
/// declaration form that accepts one accepts all of them.
#[derive(Clone, Debug, Default)]
struct DeclarationPrefix {
    /// `export`
    exported: bool,
    /// `extern "ABI"`, the ABI name alone.
    abi: Option<String>,
    /// `constexpr`
    is_constexpr: bool,
}
