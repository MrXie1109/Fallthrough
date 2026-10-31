//! The special method names, and what each one means.
//!
//! A special method is a function whose name the language itself looks for: a
//! `__add__` is what `a + b` calls, a `__str__` is what a conversion to text
//! calls. The name is the whole interface — there is no `interface` declaration
//! to satisfy — so the set of names the compiler knows is part of the language
//! definition, and it lives here so that no two passes can disagree about it.
//!
//! # Naming
//!
//! Every name is written with a double underscore on each side, so a special
//! method cannot collide with an ordinary one by accident.
//!
//! # The `i` and `r` forms
//!
//! An arithmetic or bitwise operator has up to three methods:
//!
//! * `__add__` — `a + b`, where `a`'s type defines it.
//! * `__iadd__` — `a += b`, the in-place form.
//! * `__radd__` — `a + b` where `a`'s type does *not* define `__add__`, and
//!   `b`'s type defines `__radd__`. The receiver is `b`, and `a` is passed as
//!   the argument, so the operands are swapped.
//!
//! The `r` form is tried only when the left operand has nothing to offer, which
//! is what lets `1 + fraction` work when only `Fraction` defines the operation.
//! Trying it first would let the right operand override the left, which is not
//! what anyone writing `a + b` expects.
//!
//! # The `i` form
//!
//! `__iadd__` is consulted for `+=` before `__add__`. When a type defines both,
//! the in-place form is the one that means "modify this", so it wins; when only
//! `__add__` exists, `+=` is `a = a + b`, which is why an `i` form is never
//! required.
//!
//! A compound assignment's type is always its target's: what the `i` method
//! returns is discarded, because the operation is a modification of the target
//! rather than a new value.

/// A special method the language knows by name.
///
/// The spelling is the enum variant's meaning; [`SpecialMethod::name`] gives the
/// source text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SpecialMethod {
    /// `__init__` — called by `.{ .. }`, and by `new`.
    ///
    /// Explicit only: an initialiser of another type may not call it.
    Init,
    /// `__im_init__` — a constructor that may also be used implicitly.
    ///
    /// Declared like `__init__`; it additionally lets an initialiser,
    /// assignment, argument, return or field of a different type become this
    /// type without writing a conversion.
    ImInit,
    /// `__im_as__` — a conversion that may also be used implicitly.
    ///
    /// Declared like `__as__`, on the source type with a selector pointing at
    /// the target, and additionally usable wherever the target type is
    /// required.
    ImAs,
    /// `__drop__` — called when a value's lifetime ends.
    Drop,
    /// `__assign__` — assignment: `b = a;` where `b` already exists.
    ///
    /// There is one assignment method, not four. Construction is `__init__`
    /// and assignment is `__assign__`, and in both cases the argument's type
    /// says whether it is a copy or a move -- a move being an `__init__` or
    /// `__assign__` whose parameter is a dying value. Four names would say the
    /// same thing twice.
    Assign,
    /// `__str__` — the human-readable form, for `str(x)`.
    Str,
    /// `__repr__` — the unambiguous form, for debugging.
    Repr,
    /// `__format__` — how a value is written into a formatted string.
    Format,
    /// `__eq__` — `a == b`.
    Eq,
    /// `__ne__` — `a != b`.
    Ne,
    /// `__lt__` — `a < b`.
    Lt,
    /// `__le__` — `a <= b`.
    Le,
    /// `__gt__` — `a > b`.
    Gt,
    /// `__ge__` — `a >= b`.
    Ge,
    /// `__cmp__` — the three-way comparison, for `<=>`.
    Cmp,
    /// `__hash__` — the hash of a value.
    Hash,
    /// `__add__` — `a + b`.
    Add,
    /// `__sub__` — `a - b`.
    Sub,
    /// `__mul__` — `a * b`.
    Mul,
    /// `__div__` — `a / b`.
    Div,
    /// `__mod__` — `a % b`.
    Mod,
    /// `__pow__` — `a ** b`.
    Pow,
    /// `__and__` — `a & b`.
    And,
    /// `__or__` — `a | b`.
    Or,
    /// `__xor__` — `a ^ b`.
    Xor,
    /// `__not__` — `~a`.
    Not,
    /// `__shl__` — `a << b`.
    Shl,
    /// `__shr__` — `a >> b`.
    Shr,
    /// `__neg__` — `-a`.
    Neg,
    /// `__pos__` — `+a`.
    Pos,
    /// `__inc__` — `++a`, and the modification `a++` makes.
    ///
    /// One method for both forms, because the two differ only in what the
    /// *expression* yields, and that is the compiler's business: `++a` yields
    /// the modified value, `a++` yields a copy taken before the call. The
    /// method itself only has to modify the receiver.
    Inc,
    /// `__dec__` — `--a`, and the modification `a--` makes.
    Dec,
    /// `__deref__` — `*a`.
    ///
    /// Returns a reference to what the value stands for, which is what makes a
    /// smart pointer or an iterator dereference the way a pointer does.
    Deref,
    /// `__pointer__` — `a->field`.
    ///
    /// Returns whatever `->` should be applied to next. That need not be a
    /// pointer: any type that itself supports `->` will do, so `a->b->c` can be
    /// several steps, each produced by this method.
    Pointer,
    /// `__getitem__` — `a[i]`.
    ///
    /// Returning a reference is what makes `a[i] = v` writable: the subscript
    /// names a place, and assignment writes through it. There is no separate
    /// writing method, because a place is enough.
    GetItem,
    /// `__call__` — `a(..)`.
    Call,
    /// `__bool__` — `a` in a condition.
    Bool,
    /// `__as__` — any other explicit conversion.
    As,
}

impl SpecialMethod {
    /// Returns the method's source spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            SpecialMethod::Init => "__init__",
            SpecialMethod::ImInit => "__im_init__",
            SpecialMethod::ImAs => "__im_as__",
            SpecialMethod::Drop => "__drop__",
            SpecialMethod::Assign => "__assign__",
            SpecialMethod::Str => "__str__",
            SpecialMethod::Repr => "__repr__",
            SpecialMethod::Format => "__format__",
            SpecialMethod::Eq => "__eq__",
            SpecialMethod::Ne => "__ne__",
            SpecialMethod::Lt => "__lt__",
            SpecialMethod::Le => "__le__",
            SpecialMethod::Gt => "__gt__",
            SpecialMethod::Ge => "__ge__",
            SpecialMethod::Cmp => "__cmp__",
            SpecialMethod::Hash => "__hash__",
            SpecialMethod::Add => "__add__",
            SpecialMethod::Sub => "__sub__",
            SpecialMethod::Mul => "__mul__",
            SpecialMethod::Div => "__div__",
            SpecialMethod::Mod => "__mod__",
            SpecialMethod::Pow => "__pow__",
            SpecialMethod::And => "__and__",
            SpecialMethod::Or => "__or__",
            SpecialMethod::Xor => "__xor__",
            SpecialMethod::Not => "__not__",
            SpecialMethod::Shl => "__shl__",
            SpecialMethod::Shr => "__shr__",
            SpecialMethod::Neg => "__neg__",
            SpecialMethod::Pos => "__pos__",
            SpecialMethod::Inc => "__inc__",
            SpecialMethod::Dec => "__dec__",
            SpecialMethod::Deref => "__deref__",
            SpecialMethod::Pointer => "__pointer__",
            SpecialMethod::GetItem => "__getitem__",
            SpecialMethod::Call => "__call__",
            SpecialMethod::Bool => "__bool__",
            SpecialMethod::As => "__as__",
        }
    }

    /// Returns the method for a binary operator, and whether it is the
    /// reflected form.
    ///
    /// The `i` form is not reached from here: `a += b` chooses between `__iadd__`
    /// and `__add__` itself.
    #[must_use]
    pub const fn for_binary(op: crate::BinaryOp, reflected: bool) -> Option<SpecialMethod> {
        Some(match (op, reflected) {
            (crate::BinaryOp::Add, false) => SpecialMethod::Add,
            (crate::BinaryOp::Sub, false) => SpecialMethod::Sub,
            (crate::BinaryOp::Mul, false) => SpecialMethod::Mul,
            (crate::BinaryOp::Div, false) => SpecialMethod::Div,
            (crate::BinaryOp::Rem, false) => SpecialMethod::Mod,
            (crate::BinaryOp::Pow, false) => SpecialMethod::Pow,
            (crate::BinaryOp::BitAnd, false) => SpecialMethod::And,
            (crate::BinaryOp::BitOr, false) => SpecialMethod::Or,
            (crate::BinaryOp::BitXor, false) => SpecialMethod::Xor,
            (crate::BinaryOp::Shl, false) => SpecialMethod::Shl,
            (crate::BinaryOp::Shr, false) => SpecialMethod::Shr,
            (crate::BinaryOp::Eq, false) => SpecialMethod::Eq,
            (crate::BinaryOp::Ne, false) => SpecialMethod::Ne,
            (crate::BinaryOp::Lt, false) => SpecialMethod::Lt,
            (crate::BinaryOp::Le, false) => SpecialMethod::Le,
            (crate::BinaryOp::Gt, false) => SpecialMethod::Gt,
            (crate::BinaryOp::Ge, false) => SpecialMethod::Ge,
            // The three-way comparison has one form: it already answers for
            // both orders, so there is nothing for a reflected version to add.
            (crate::BinaryOp::Cmp, false) => SpecialMethod::Cmp,
            // The reflected forms exist for the symmetric operators. A
            // comparison has a reflected form too -- `1 < fraction` needs
            // `fraction.__gt__` -- but it is not the same operator reversed by
            // name, so it is spelled out.
            (crate::BinaryOp::Add, true) => SpecialMethod::Add,
            (crate::BinaryOp::Sub, true) => SpecialMethod::Sub,
            (crate::BinaryOp::Mul, true) => SpecialMethod::Mul,
            (crate::BinaryOp::Div, true) => SpecialMethod::Div,
            (crate::BinaryOp::Rem, true) => SpecialMethod::Mod,
            (crate::BinaryOp::Pow, true) => SpecialMethod::Pow,
            (crate::BinaryOp::BitAnd, true) => SpecialMethod::And,
            (crate::BinaryOp::BitOr, true) => SpecialMethod::Or,
            (crate::BinaryOp::BitXor, true) => SpecialMethod::Xor,
            (crate::BinaryOp::Shl, true) => SpecialMethod::Shl,
            (crate::BinaryOp::Shr, true) => SpecialMethod::Shr,
            (crate::BinaryOp::Eq, true) => SpecialMethod::Eq,
            (crate::BinaryOp::Ne, true) => SpecialMethod::Ne,
            (crate::BinaryOp::Lt, true) => SpecialMethod::Gt,
            (crate::BinaryOp::Le, true) => SpecialMethod::Ge,
            (crate::BinaryOp::Gt, true) => SpecialMethod::Lt,
            (crate::BinaryOp::Ge, true) => SpecialMethod::Le,
            _ => return None,
        })
    }

    /// Returns the method for a unary operator.
    #[must_use]
    pub const fn for_unary(op: crate::UnaryOp) -> Option<SpecialMethod> {
        Some(match op {
            crate::UnaryOp::Neg => SpecialMethod::Neg,
            crate::UnaryOp::Plus => SpecialMethod::Pos,
            crate::UnaryOp::Not => SpecialMethod::Not,
            // `~a` is the bitwise complement. `!a` is logical negation, which
            // is not `__not__`: a truth test asks for `__bool__` and then
            // negates the answer, and no user method can change that.
            crate::UnaryOp::BitNot => SpecialMethod::Not,
            // A type may redefine dereference, which is what makes a smart
            // pointer dereference like a pointer does. Taking an address may
            // not be redefined: it is how the language reaches memory.
            crate::UnaryOp::Deref => SpecialMethod::Deref,
            crate::UnaryOp::AddressOf => return None,
        })
    }

    /// Returns the method for `op=`, if the operator has an in-place form.
    ///
    /// A `+` has `__iadd__`; a comparison has none, because `a == b` has no
    /// in-place meaning to give.
    #[must_use]
    pub fn in_place(self) -> Option<SpecialMethod> {
        let base = match self {
            SpecialMethod::Add => "iadd",
            SpecialMethod::Sub => "isub",
            SpecialMethod::Mul => "imul",
            SpecialMethod::Div => "idiv",
            SpecialMethod::Mod => "imod",
            SpecialMethod::Pow => "ipow",
            SpecialMethod::And => "iand",
            SpecialMethod::Or => "ior",
            SpecialMethod::Xor => "ixor",
            SpecialMethod::Shl => "ishl",
            SpecialMethod::Shr => "ishr",
            _ => return None,
        };
        // Written directly rather than through a lookup table, because the set
        // is closed and a table would need its own test to stay complete.
        Some(match base {
            "iadd" => SpecialMethod::Add,
            "isub" => SpecialMethod::Sub,
            "imul" => SpecialMethod::Mul,
            "idiv" => SpecialMethod::Div,
            "imod" => SpecialMethod::Mod,
            "ipow" => SpecialMethod::Pow,
            "iand" => SpecialMethod::And,
            "ior" => SpecialMethod::Or,
            "ixor" => SpecialMethod::Xor,
            "ishl" => SpecialMethod::Shl,
            _ => SpecialMethod::Shr,
        })
    }

    /// Returns the `i` form's source spelling for a base operator.
    ///
    /// `__add__` has `__iadd__`; a method with no in-place form has none.
    #[must_use]
    pub fn in_place_name(self) -> Option<&'static str> {
        Some(match self {
            SpecialMethod::Add => "__iadd__",
            SpecialMethod::Sub => "__isub__",
            SpecialMethod::Mul => "__imul__",
            SpecialMethod::Div => "__idiv__",
            SpecialMethod::Mod => "__imod__",
            SpecialMethod::Pow => "__ipow__",
            SpecialMethod::And => "__iand__",
            SpecialMethod::Or => "__ior__",
            SpecialMethod::Xor => "__ixor__",
            SpecialMethod::Shl => "__ishl__",
            SpecialMethod::Shr => "__ishr__",
            _ => return None,
        })
    }

    /// Returns the `r` form's source spelling for a base operator.
    ///
    /// The reflected name is the base name with an `r` in front of the operator
    /// word: `__radd__`, `__rsub__`. A comparison swaps to its opposite instead,
    /// which [`SpecialMethod::for_binary`] handles.
    #[must_use]
    pub fn reflected_name(self) -> Option<&'static str> {
        Some(match self {
            SpecialMethod::Add => "__radd__",
            SpecialMethod::Sub => "__rsub__",
            SpecialMethod::Mul => "__rmul__",
            SpecialMethod::Div => "__rdiv__",
            SpecialMethod::Mod => "__rmod__",
            SpecialMethod::Pow => "__rpow__",
            SpecialMethod::And => "__rand__",
            SpecialMethod::Or => "__ror__",
            SpecialMethod::Xor => "__rxor__",
            SpecialMethod::Shl => "__rshl__",
            SpecialMethod::Shr => "__rshr__",
            SpecialMethod::Eq => "__req__",
            SpecialMethod::Ne => "__rne__",
            SpecialMethod::Lt => "__rlt__",
            SpecialMethod::Le => "__rle__",
            SpecialMethod::Gt => "__rgt__",
            SpecialMethod::Ge => "__rge__",
            _ => return None,
        })
    }
}

/// Returns the symbol a special method is emitted and called under.
///
/// `written_name` is the special method's spelling and `params` are the mangled
/// parameter types in order -- the receiver first, then the rest. The receiver
/// is a parameter like any other, which is what keeps `*Box` and `*const Box`
/// distinct: naming the symbol after the bare type name made those two collide.
///
/// The mangling of a type itself lives in `fallc-sema`, because that is where
/// `Type` is defined and the encoding has to know the type's structure rather
/// than its printed form.
#[must_use]
pub fn symbol_for(written_name: &str, params: &[String]) -> String {
    let mut symbol = written_name.to_owned();
    for param in params {
        symbol.push('_');
        symbol.push_str(param);
    }
    symbol
}

/// Every special method, sorted by name.
///
/// Must stay sorted, because [`lookup`] binary-searches it. A test below checks
/// both the order and that every variant is present, so adding a variant
/// without a table entry fails loudly instead of silently never being found.
pub static SPECIAL_METHODS: &[SpecialMethod] = &[
    SpecialMethod::Add,
    SpecialMethod::And,
    SpecialMethod::As,
    SpecialMethod::Assign,
    SpecialMethod::Bool,
    SpecialMethod::Call,
    SpecialMethod::Cmp,
    SpecialMethod::Dec,
    SpecialMethod::Deref,
    SpecialMethod::Div,
    SpecialMethod::Drop,
    SpecialMethod::Eq,
    SpecialMethod::Format,
    SpecialMethod::Ge,
    SpecialMethod::GetItem,
    SpecialMethod::Gt,
    SpecialMethod::Hash,
    SpecialMethod::ImAs,
    SpecialMethod::ImInit,
    SpecialMethod::Inc,
    SpecialMethod::Init,
    SpecialMethod::Le,
    SpecialMethod::Lt,
    SpecialMethod::Mod,
    SpecialMethod::Mul,
    SpecialMethod::Ne,
    SpecialMethod::Neg,
    SpecialMethod::Not,
    SpecialMethod::Or,
    SpecialMethod::Pointer,
    SpecialMethod::Pos,
    SpecialMethod::Pow,
    SpecialMethod::Repr,
    SpecialMethod::Shl,
    SpecialMethod::Shr,
    SpecialMethod::Str,
    SpecialMethod::Sub,
    SpecialMethod::Xor,
];

/// Looks a special method up by its source spelling.
#[must_use]
pub fn lookup(name: &str) -> Option<SpecialMethod> {
    SPECIAL_METHODS
        .binary_search_by(|method| method.name().cmp(name))
        .ok()
        .map(|index| SPECIAL_METHODS[index])
}

/// Returns `true` if `name` is a special method's name.
///
/// The `i` and `r` forms count. They are not separate table entries, because
/// each is derived from a base method rather than standing on its own, but
/// `__iadd__` and `__radd__` are special methods all the same: writing one has
/// to change what `+=` or a mixed-type `+` does. Treating them as ordinary
/// functions made them fail in a way that looked like the operator being
/// unimplemented.
#[must_use]
pub fn is_special(name: &str) -> bool {
    if lookup(name).is_some() {
        return true;
    }
    // `__i<op>__` and `__r<op>__`, for an operator whose base form is known.
    let Some(inner) = name
        .strip_prefix("__")
        .and_then(|rest| rest.strip_suffix("__"))
    else {
        return false;
    };
    let Some(word) = inner.strip_prefix('i').or_else(|| inner.strip_prefix('r')) else {
        return false;
    };
    SPECIAL_METHODS
        .iter()
        .any(|method| method.in_place_name() == Some(name) || method.reflected_name() == Some(name))
        && !word.is_empty()
}

/// Returns `true` if the name looks like a special method, whether or not the
/// compiler knows it.
///
/// Used to warn about a near miss: a function called `__add` or `_add_` is
/// almost certainly meant to be `__add__`, and silently treating it as an
/// ordinary function would leave the operator it was written for broken.
#[must_use]
pub fn looks_special(name: &str) -> bool {
    // A leading `__` is the tell. Requiring the trailing one too would miss
    // `__add`, which is the likeliest typo of all, and a missing trailing pair
    // is exactly what makes the name stop being special.
    name.len() > 3 && name.starts_with("__")
}

#[cfg(test)]
mod tests {
    use super::{SPECIAL_METHODS, SpecialMethod, is_special, looks_special, lookup};

    #[test]
    fn the_table_is_sorted_by_name() {
        // `lookup` binary-searches, so an out-of-order entry silently stops
        // being found.
        for pair in SPECIAL_METHODS.windows(2) {
            assert!(
                pair[0].name() < pair[1].name(),
                "SPECIAL_METHODS is not sorted: {:?} then {:?}",
                pair[0].name(),
                pair[1].name()
            );
        }
    }

    #[test]
    fn every_variant_is_in_the_table() {
        // A variant missing from the table is a method the compiler would never
        // find, which is invisible until someone writes it.
        let every = [
            SpecialMethod::Init,
            SpecialMethod::Drop,
            SpecialMethod::Assign,
            SpecialMethod::Str,
            SpecialMethod::Repr,
            SpecialMethod::Format,
            SpecialMethod::Eq,
            SpecialMethod::Ne,
            SpecialMethod::Lt,
            SpecialMethod::Le,
            SpecialMethod::Gt,
            SpecialMethod::Ge,
            SpecialMethod::Cmp,
            SpecialMethod::Hash,
            SpecialMethod::Add,
            SpecialMethod::Sub,
            SpecialMethod::Mul,
            SpecialMethod::Div,
            SpecialMethod::Mod,
            SpecialMethod::Pow,
            SpecialMethod::And,
            SpecialMethod::Or,
            SpecialMethod::Xor,
            SpecialMethod::Inc,
            SpecialMethod::Dec,
            SpecialMethod::Not,
            SpecialMethod::Shl,
            SpecialMethod::Shr,
            SpecialMethod::Neg,
            SpecialMethod::Pos,
            SpecialMethod::Deref,
            SpecialMethod::Pointer,
            SpecialMethod::GetItem,
            SpecialMethod::Call,
            SpecialMethod::Bool,
            SpecialMethod::As,
            SpecialMethod::ImInit,
            SpecialMethod::ImAs,
        ];
        for method in every {
            assert_eq!(
                lookup(method.name()),
                Some(method),
                "`{}` is missing from SPECIAL_METHODS",
                method.name()
            );
        }
        assert_eq!(SPECIAL_METHODS.len(), every.len(), "the table has extras");
    }

    #[test]
    fn every_name_round_trips() {
        for method in SPECIAL_METHODS {
            assert_eq!(lookup(method.name()), Some(*method));
            assert!(is_special(method.name()));
        }
        assert!(!is_special("add"));
        assert!(!is_special("__unknown__"));
    }

    #[test]
    fn a_misspelled_special_name_is_recognised_as_one() {
        // `__add` and `_add_` are not special methods, but they are almost
        // certainly meant to be, so the compiler can say so.
        assert!(looks_special("__add"));
        assert!(looks_special("__add__"));
        assert!(!looks_special("__"));
        assert!(!looks_special("add"));
        assert!(!looks_special("__x"));
    }

    #[test]
    fn the_reflected_comparison_swaps_the_operator() {
        // `1 < fraction` asks the fraction for `__gt__`, because the fraction
        // is being asked "are you greater than 1".
        assert_eq!(
            SpecialMethod::for_binary(crate::BinaryOp::Lt, true),
            Some(SpecialMethod::Gt)
        );
        assert_eq!(
            SpecialMethod::for_binary(crate::BinaryOp::Ge, true),
            Some(SpecialMethod::Le)
        );
        // An equality is its own reflection.
        assert_eq!(
            SpecialMethod::for_binary(crate::BinaryOp::Eq, true),
            Some(SpecialMethod::Eq)
        );
    }

    #[test]
    fn in_place_forms_exist_only_where_they_mean_something() {
        assert_eq!(SpecialMethod::Add.in_place_name(), Some("__iadd__"));
        assert_eq!(SpecialMethod::Shr.in_place_name(), Some("__ishr__"));
        // A comparison has no in-place meaning.
        assert_eq!(SpecialMethod::Eq.in_place_name(), None);
        assert_eq!(SpecialMethod::Lt.in_place_name(), None);
    }

    #[test]
    fn arithmetic_operators_have_all_three_forms() {
        for op in [
            crate::BinaryOp::Add,
            crate::BinaryOp::Sub,
            crate::BinaryOp::Mul,
            crate::BinaryOp::Div,
            crate::BinaryOp::Rem,
            crate::BinaryOp::Pow,
            crate::BinaryOp::BitAnd,
            crate::BinaryOp::BitOr,
            crate::BinaryOp::BitXor,
            crate::BinaryOp::Shl,
            crate::BinaryOp::Shr,
        ] {
            let base = SpecialMethod::for_binary(op, false).expect("a base form exists");
            assert!(base.in_place_name().is_some(), "{op:?} has no i form");
            assert!(base.reflected_name().is_some(), "{op:?} has no r form");
        }
    }
}
