//! Operator precedence and associativity.
//!
//! Expressions are parsed with a binding-power (Pratt) loop. Each infix or
//! postfix operator has a [`BindingPower`]: the loop consumes an operator only
//! if its left binding power exceeds the caller's minimum, which encodes
//! precedence and associativity in one number.
//!
//! Writing `x - y - z` parses left-associatively precisely because `-`'s
//! `(left, right)` pair satisfies `left > right`; a right-associative operator
//! sets `left < right`.
//!
//! [`INFIX_OPERATORS`] is C's table plus `**`, which binds tighter than `*` and
//! is right-associative. Assignment and the conditional operator are handled by
//! the parser rather than by that table, because each has a shape a binding
//! power cannot express; their levels are named here so the two agree.

use fallc_lexer::{Punctuation, TokenKind};

/// Whether an operator is prefix, infix, postfix, or enclosing.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Fixity {
    /// Written before its operand, e.g. `-x`.
    Prefix,
    /// Written between two operands, e.g. `a + b`.
    Infix,
    /// Written after its operand, e.g. `a?`.
    Postfix,
    /// Wraps its operand, e.g. `(a)` or `[a]`.
    Enclosed,
}

/// Associativity of an infix operator.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Assoc {
    /// `a + b + c` parses as `(a + b) + c`.
    Left,
    /// `a = b = c` parses as `a = (b = c)`.
    Right,
    /// Chaining is a syntax or type error, e.g. comparison chains.
    None,
}

/// The pair of binding powers attached to an infix operator.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BindingPower {
    /// Power required to accept this operator on the left.
    pub left: u8,
    /// Power the right-hand operand must exceed.
    pub right: u8,
}

impl BindingPower {
    /// Creates a binding power pair.
    #[must_use]
    pub const fn new(left: u8, right: u8) -> Self {
        BindingPower { left, right }
    }

    /// The `(left, right)` pair implementing left associativity at `level`.
    #[must_use]
    pub const fn left_assoc(level: u8) -> Self {
        BindingPower::new(level, level + 1)
    }

    /// The `(left, right)` pair implementing right associativity at `level`.
    #[must_use]
    pub const fn right_assoc(level: u8) -> Self {
        BindingPower::new(level, level - 1)
    }

    /// The `(left, right)` pair implementing a non-associative operator.
    #[must_use]
    pub const fn non_assoc(level: u8) -> Self {
        BindingPower::new(level, level)
    }

    /// The binding power used to parse a whole expression.
    #[must_use]
    pub const fn min() -> Self {
        BindingPower::new(0, 0)
    }
}

/// Describes one operator's parsing behaviour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpInfo {
    /// Prefix, infix, postfix, or enclosing.
    pub fixity: Fixity,
    /// Associativity; meaningful for infix operators.
    pub assoc: Assoc,
    /// Binding powers; meaningful for infix and postfix operators.
    pub power: BindingPower,
}

impl OpInfo {
    /// Describes a left-associative infix operator at `level`.
    #[must_use]
    pub const fn infix_left(level: u8) -> Self {
        OpInfo {
            fixity: Fixity::Infix,
            assoc: Assoc::Left,
            power: BindingPower::left_assoc(level),
        }
    }

    /// Describes a right-associative infix operator at `level`.
    #[must_use]
    pub const fn infix_right(level: u8) -> Self {
        OpInfo {
            fixity: Fixity::Infix,
            assoc: Assoc::Right,
            power: BindingPower::right_assoc(level),
        }
    }

    /// Describes a prefix operator binding at `level`.
    #[must_use]
    pub const fn prefix(level: u8) -> Self {
        OpInfo {
            fixity: Fixity::Prefix,
            assoc: Assoc::Right,
            power: BindingPower::new(level, level),
        }
    }

    /// Describes a postfix operator binding at `level`.
    #[must_use]
    pub const fn postfix(level: u8) -> Self {
        OpInfo {
            fixity: Fixity::Postfix,
            assoc: Assoc::Left,
            power: BindingPower::new(level, level + 1),
        }
    }
}

/// The infix operator table, following C's precedence and associativity.
///
/// # Levels
///
/// Higher binds tighter. The gaps leave room to insert operators, and the
/// ordering matches C exactly so that mixed expressions such as
/// `a + b * c`, `a & b == c`, and `a = b = c` group the way a C programmer
/// expects. In particular bitwise operators bind *looser* than comparisons,
/// which is C's rule (and the opposite of what many people assume).
///
/// | Level | Operators | Associativity |
/// | ----- | --------- | ------------- |
/// | 10 | `*` `/` `%` | left |
/// | 9 | `+` `-` | left |
/// | 8 | `<<` `>>` | left |
/// | 7 | `<` `<=` `>` `>=` | left |
/// | 6 | `==` `!=` | left |
/// | 5 | `&` | left |
/// | 4 | `^` | left |
/// | 3 | `|` | left |
/// | 2 | `&&` | left |
/// | 1 | `||` | left |
/// | 0 | `?:` | right |
///
/// The conditional operator is handled by the parser rather than by this table,
/// because its middle operand is a full expression rather than one at its own
/// precedence: in `a ? b : c` the `b` runs to the `:` whatever it contains.
/// The level is named here so that the two agree about where it sits.
///
/// Assignment is right-associative and is handled separately from this table,
/// because it needs an lvalue on the left rather than a value.
///
/// # Ordering
///
/// The table is sorted by [`TokenKind`]'s own ordering, **not** by precedence,
/// because lookup binary-searches it. Sorting it by precedence reads better but
/// silently makes every lookup fail, so a test asserts the ordering.
pub static INFIX_OPERATORS_SORTED_BY_KIND: () = ();
pub static INFIX_OPERATORS: &[(TokenKind, OpInfo)] = &[
    (TokenKind::Punct(Punctuation::Plus), OpInfo::infix_left(9)),
    (TokenKind::Punct(Punctuation::Minus), OpInfo::infix_left(9)),
    (TokenKind::Punct(Punctuation::Star), OpInfo::infix_left(10)),
    // `**` binds tighter than `*` and is right-associative, so `2 ** 3 ** 2`
    // is `2 ** (3 ** 2)`. Grouping it the other way would give `(2 ** 3) ** 2`,
    // which is a different number.
    (
        TokenKind::Punct(Punctuation::StarStar),
        OpInfo::infix_right(11),
    ),
    (TokenKind::Punct(Punctuation::Slash), OpInfo::infix_left(10)),
    (
        TokenKind::Punct(Punctuation::Percent),
        OpInfo::infix_left(10),
    ),
    (TokenKind::Punct(Punctuation::Amp), OpInfo::infix_left(5)),
    (TokenKind::Punct(Punctuation::Pipe), OpInfo::infix_left(3)),
    (TokenKind::Punct(Punctuation::Caret), OpInfo::infix_left(4)),
    (TokenKind::Punct(Punctuation::Shl), OpInfo::infix_left(8)),
    (TokenKind::Punct(Punctuation::Shr), OpInfo::infix_left(8)),
    (TokenKind::Punct(Punctuation::AmpAmp), OpInfo::infix_left(2)),
    (
        TokenKind::Punct(Punctuation::PipePipe),
        OpInfo::infix_left(1),
    ),
    (TokenKind::Punct(Punctuation::EqEq), OpInfo::infix_left(6)),
    (TokenKind::Punct(Punctuation::BangEq), OpInfo::infix_left(6)),
    (TokenKind::Punct(Punctuation::Lt), OpInfo::infix_left(7)),
    (TokenKind::Punct(Punctuation::Le), OpInfo::infix_left(7)),
    (TokenKind::Punct(Punctuation::Cmp), OpInfo::infix_left(7)),
    (TokenKind::Punct(Punctuation::Gt), OpInfo::infix_left(7)),
    (TokenKind::Punct(Punctuation::Ge), OpInfo::infix_left(7)),
];

/// The prefix operator table.
///
/// `+`, `-`, `~`, and `!`. Dereference (`*`) and address-of (`&`) are handled in
/// the expression parser rather than here, because their operand is parsed to
/// the postfix level instead of the usual prefix one.
pub static PREFIX_OPERATORS: &[(TokenKind, OpInfo)] = &[
    // Sorted by `TokenKind`, which is the order the enum declares them, not by
    // anything about the operators: `lookup` binary-searches this table, so
    // `-a` and `+a` worked while `~a` and `!a` silently did not parse at all.
    (TokenKind::Punct(Punctuation::Plus), OpInfo::prefix(11)),
    (TokenKind::Punct(Punctuation::Minus), OpInfo::prefix(11)),
    (TokenKind::Punct(Punctuation::Tilde), OpInfo::prefix(11)),
    (TokenKind::Punct(Punctuation::Bang), OpInfo::prefix(11)),
];

/// The postfix operator table.
///
/// Empty: `++`, `--`, calls, indexing, and field access are postfix forms that
/// the expression parser handles directly, so there is no postfix operator to
/// look up.
pub static POSTFIX_OPERATORS: &[(TokenKind, OpInfo)] = &[];

/// The precedence of assignment, for reference.
///
/// Lower than every binary operator, and right-associative, so `a = b = c`
/// assigns `c` to `b` first.
pub const ASSIGNMENT_PRECEDENCE: u8 = 0;

/// The precedence of the conditional operator.
///
/// Below `||` and above assignment. Right-associative, so
/// `a ? b : c ? d : e` is `a ? b : (c ? d : e)`.
pub const CONDITIONAL_PRECEDENCE: u8 = 0;

/// Binding power of `..` and `..=`.
///
/// A range is not an operator in the arithmetic sense: its two ends are
/// integers and its result is a `Range`. It is parsed at the lowest level, below
/// `?:`, so `1 .. n > 0 ? a : b` reads the range's high end as the whole
/// conditional -- but above nothing, since a range is written where a value is
/// expected rather than around one.
pub const RANGE_PRECEDENCE: u8 = 0;

/// Looks up the parsing behaviour of an infix operator.
#[must_use]
pub fn infix(kind: TokenKind) -> Option<OpInfo> {
    lookup(INFIX_OPERATORS, kind)
}

/// Looks up the parsing behaviour of a prefix operator.
#[must_use]
pub fn prefix(kind: TokenKind) -> Option<OpInfo> {
    lookup(PREFIX_OPERATORS, kind)
}

/// Looks up the parsing behaviour of a postfix operator.
#[must_use]
pub fn postfix(kind: TokenKind) -> Option<OpInfo> {
    lookup(POSTFIX_OPERATORS, kind)
}

fn lookup(table: &[(TokenKind, OpInfo)], kind: TokenKind) -> Option<OpInfo> {
    table
        .binary_search_by(|(k, _)| k.cmp(&kind))
        .ok()
        .map(|idx| table[idx].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_infix_table_is_sorted_by_token_kind() {
        // `lookup` binary-searches by `TokenKind`, not by precedence, so an
        // entry inserted next to its precedence group rather than in enum order
        // is simply never found: `<=>` lexed correctly and then failed to
        // parse, because `Cmp` was written after `Ge` instead of between `Le`
        // and `Gt`. Nothing else caught it.
        for pair in INFIX_OPERATORS.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "INFIX_OPERATORS is not sorted by TokenKind: {:?} then {:?}",
                pair[0].0,
                pair[1].0
            );
        }
    }

    #[test]
    fn every_infix_entry_is_reachable() {
        // The sortedness test above proves order; this proves the other half,
        // that a `TokenKind` in the table can actually be looked up.
        for (kind, expected) in INFIX_OPERATORS {
            let found = infix(*kind).unwrap_or_else(|| {
                panic!("{:?} is in the table but `infix` did not find it", kind)
            });
            assert_eq!(found, *expected, "{kind:?} resolved to the wrong entry");
        }
    }

    #[test]
    fn the_prefix_table_is_sorted_and_reachable() {
        for pair in PREFIX_OPERATORS.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "PREFIX_OPERATORS is not sorted: {:?} then {:?}",
                pair[0].0,
                pair[1].0
            );
        }
        for (kind, _) in PREFIX_OPERATORS {
            assert!(prefix(*kind).is_some(), "{kind:?} is unreachable");
        }
    }

    #[test]
    fn exponentiation_binds_tighter_than_multiplication() {
        // `2 * 3 ** 2` must be `2 * 9`, so `**` needs the higher power.
        let star = infix(TokenKind::Punct(Punctuation::Star)).expect("`*` is an operator");
        let starstar = infix(TokenKind::Punct(Punctuation::StarStar)).expect("`**` is an operator");
        assert!(
            starstar.power.left > star.power.left,
            "`**` must bind tighter than `*`"
        );
    }

    #[test]
    fn exponentiation_is_right_associative() {
        // `2 ** 3 ** 2` is `2 ** 9`, not `8 ** 2`.
        let starstar = infix(TokenKind::Punct(Punctuation::StarStar)).expect("`**` is an operator");
        assert_eq!(starstar.assoc, Assoc::Right);
        assert!(starstar.power.right <= starstar.power.left);
    }

    #[test]
    fn empty_tables_report_no_operators() {
        assert!(infix(TokenKind::Unknown).is_none());
        assert!(prefix(TokenKind::Eof).is_none());
        assert!(postfix(TokenKind::Eof).is_none());
    }

    #[test]
    fn left_assoc_binds_tighter_on_the_left() {
        let bp = BindingPower::left_assoc(5);
        assert!(bp.left < bp.right);
    }

    #[test]
    fn right_assoc_binds_tighter_on_the_right() {
        let bp = BindingPower::right_assoc(5);
        assert!(bp.left > bp.right);
    }
}
