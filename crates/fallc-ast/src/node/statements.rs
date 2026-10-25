//! Statements: the forms a body is made of.
//!
//! A block is a list of statements, and [`StmtKind`] names each form. The
//! structures a statement owns -- a binding's initialiser, a `switch` arm, a
//! `try`'s handler -- are next to it.

use super::*;

/// A statement inside a block or function body.
///
/// A block, a binding, an `if`, a `while`, a C-style `for`, a `switch`, a
/// `fallthrough`, a `break`, a `continue`, a `return`, a `raise`, a `try`, or an
/// expression statement. A block is a statement rather than an expression: it has
/// a scope of its own and yields no value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StmtKind {
    /// `{ .. }` on its own -- a block in a position where a statement is
    /// expected.
    ///
    /// It gives the statements inside it a scope of their own, which is what
    /// makes `{ String s = ...; }` destroy `s` at the brace, and what lets a
    /// `switch` arm be written as a block.
    Block(BlockId),
    /// `defer { .. }` -- runs the block when its scope exits.
    ///
    /// Declared order is not the order of execution: deferred blocks run in
    /// reverse, like destructors, and they run on every way out of the scope --
    /// falling off its end, `return`, `break`, `continue`, and an exception
    /// leaving it.
    Defer(BlockId),
    /// `name type;`, `name type = init;`, or `name auto = init;`
    ///
    /// The type is written **postfix**, after the name, like every declaration
    /// in the language.
    Bind(BindStmt),
    /// `if (cond) { .. } else if (cond) { .. } else { .. }`
    If(IfStmt),
    /// `while (cond) { .. } else { .. }`
    ///
    /// The `else` clause runs when the loop ends by its condition becoming
    /// false. `break`, `return`, and `raise` skip it; `continue` does not
    /// count as an interruption.
    While(LoopStmt),
    /// `for (init; cond; update) { .. } else { .. }`
    ///
    /// C-style. The `else` clause follows the same rule as `while`.
    ///
    /// The *range* form, `for (x T : range)`, is rewritten into this one while
    /// the file is parsed: it is a pattern of `begin()`, `end()`, `!=`, `++`
    /// and `*`, all of which are ordinary operations, so there is nothing left
    /// for a later stage to know about it.
    ForC(ForCStmt),
    /// `switch (expr) { case a: .. case b, c: .. default: .. }`
    ///
    /// Cases do not fall through unless a case ends with [`StmtKind::Fallthrough`].
    Switch(SwitchStmt),
    /// `fallthrough;`
    ///
    /// Falls through to the next case. Valid only inside a `switch`.
    Fallthrough,
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `return expr;` or `return;`
    Return(ReturnStmt),
    /// `raise "type": "what";` or the bare `raise;` that re-raises.
    Raise(RaiseStmt),
    /// `try { .. } except { .. } else { .. } finally { .. }`
    ///
    /// `except`, `else`, and `finally` are all optional.
    Try(TryStmt),
    /// An expression evaluated for its effect, as in `f();`.
    Expr(ExprStmt),
    /// `static_assert(cond);` or `static_assert(cond, "message");`
    ///
    /// In a function body it is checked when the body is checked, which for a
    /// template function is at instantiation: an assertion inside a template
    /// that is never used never fires.
    StaticAssert(StaticAssert),
    /// A statement the parser could not recognise.
    Unresolved(Unresolved),
}

/// `static_assert(cond);` — a claim the compiler checks.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StaticAssert {
    /// The condition, which must be a compile-time `bool`.
    pub condition: ExprId,
    /// The message to report when it does not hold, if one was written.
    pub message: Option<String>,
    /// Where it was written, including the `;`.
    pub span: Span,
}

/// An expression statement.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExprStmt {
    /// The expression, whose value is discarded.
    pub expr: ExprId,
}

/// A variable binding.
///
/// # The four legal forms
///
/// ```text
/// name type;              // zero-initialised
/// name type = expr;       // initialised from an expression
/// name type = .{ .. };    // initialised from a designated initialiser
/// name auto = expr;       // type deduced from the initialiser
/// ```
///
/// Two forms are **errors**, both because the type cannot be deduced:
///
/// ```text
/// name auto;              // no initialiser to deduce from
/// name auto = .{ .. };    // a designated initialiser has no type of its own
/// ```
///
/// Both are represented here and reported during resolution rather than being
/// rejected by the parser, so that the diagnostic can explain *why* rather than
/// just failing to parse.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BindStmt {
    /// The bound name.
    pub name: String,
    /// The span of the name alone, for diagnostics that point at it.
    pub name_span: Span,
    /// The declared type, or `auto`.
    pub ty: BindingType,
    /// The initialiser, if one was written.
    pub init: Option<Initializer>,
    /// Whether `static` was written after the type.
    ///
    /// A static binding has one object for the whole run rather than one per
    /// call, which makes its initialiser part of the program image: it has to
    /// be computable before the program starts, exactly as a module-level
    /// variable's is.
    pub is_static: bool,
    /// Whether the storage is one object per thread (`thread_local`).
    ///
    /// For a local it also means `static`: per-thread storage outlives the call
    /// that made it, exactly as a static local does.
    pub is_thread_local: bool,
    /// The alignment `alignas(..)` asked for, if it was written.
    ///
    /// The request can only raise the alignment the type would get on its own.
    pub align: Option<AlignSpec>,
    /// Whether the declaration is `export`ed.
    ///
    /// Only a module-level variable may be: a local belongs to one call frame,
    /// so there is nothing another module could name. The flag is written on
    /// the item form and is always `false` for a binding inside a body.
    pub exported: bool,
    /// Whether `@uninitialized` was written: the storage is deliberately left
    /// as it is, with no zero fill and no empty constructor.
    ///
    /// `@uninitialized` is only meaningful without an initialiser: the whole
    /// point is that nothing is written before the program does.
    pub is_uninitialized: bool,
    /// Attributes written after the declaration: `count int @static = 0;`.
    ///
    /// The new spelling replaces the old `static`/`thread_local`/`alignas`
    /// modifiers; the parser folds those three into the fields above so that
    /// nothing downstream has to know both spellings. The rest (`@must_use`,
    /// `@unused`, `@deprecated`, ...) stay here for the checker.
    pub attrs: Vec<super::Attr>,
}

/// The type part of a binding.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BindingType {
    /// A written type annotation.
    Written(TypeSyntax),
    /// The `auto` keyword, asking for deduction from the initialiser.
    Auto(Span),
}

impl BindStmt {
    /// Returns `true` if this binding is declared `const`.
    ///
    /// A `const` binding must be initialised and cannot be assigned afterwards.
    #[must_use]
    pub fn is_const(&self) -> bool {
        matches!(&self.ty, BindingType::Written(ty) if ty.is_const())
    }
}

impl BindingType {
    /// Returns the source range of the type part.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            BindingType::Written(ty) => ty.span(),
            BindingType::Auto(span) => *span,
        }
    }

    /// Returns `true` if this is `auto`.
    #[must_use]
    pub fn is_auto(&self) -> bool {
        matches!(self, BindingType::Auto(_))
    }
}

/// How a binding or field is initialised.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Initializer {
    /// `= expr`
    Expr(ExprId),
    /// `= .{ .. }`, a designated initialiser.
    ///
    /// The elements are expressions, in source order, and are assigned by
    /// position. The raw text between the braces is kept as well, for
    /// diagnostics and for a form whose elements are not expressions.
    Designated(DesignatedInit),
}

/// The contents of a `.{ .. }` initialiser.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DesignatedInit {
    /// The expression this initialiser becomes, for a nested one.
    ///
    /// A top-level initialiser is not an expression, so this is
    /// [`ExprId::DUMMY`] for it. A nested one is, so the checker can record the
    /// type it derived from the enclosing field.
    pub expr: ExprId,
    /// The exact source text between the braces, without the braces.
    ///
    /// Kept for diagnostics and for a form whose elements are not expressions.
    pub text: String,
    /// The elements, in source order.
    ///
    /// A positional initialiser assigns these by position, which is why the
    /// order is recorded rather than a map.
    pub elements: Vec<ExprId>,
    /// The elements written by name, `.field = value`.
    ///
    /// They come after every positional element, so a positional list still
    /// assigns by position and only the tail is named.
    pub named: Vec<NamedArgument>,
    /// The source range of the whole initialiser, including `.{` and `}`.
    pub span: Span,
}

/// `if (cond) { .. } else if (cond) { .. } else { .. }`
///
/// The `else if` chain is represented as an `else` branch holding a single
/// [`StmtKind::If`], which keeps the node uniform: there is no separate
/// `else if` variant to handle in every pass.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IfStmt {
    /// Whether the condition is marked `constexpr`.
    ///
    /// `if constexpr (..)` requires its condition to be a constant, and the
    /// branch it does not take is **not checked** -- so that branch may name
    /// things that do not exist for this instantiation. A plain `if` checks
    /// both branches and emits only the one taken.
    pub is_constexpr: bool,
    /// The condition. Truthiness is allowed, so this need not be boolean.
    pub condition: ExprId,
    /// Attributes written after the condition: `if (cond) @likely {`.
    ///
    /// They are branch-weight hints, `@likely` and `@unlikely`; the checker
    /// rejects anything else in this position.
    pub condition_attrs: Vec<super::Attr>,
    /// The `then` block.
    pub then_block: BlockId,
    /// The `else` branch, if present.
    pub else_branch: Option<ElseBranch>,
    /// Attributes written after `else`: `else @likely {`.
    pub else_attrs: Vec<super::Attr>,
}

/// What follows an `if` or a loop's `else` keyword.
///
/// The spec allows `else { .. }` and `else if (..) { .. }`. Both are represented
/// here so that passes match once instead of re-deriving the chain shape.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ElseBranch {
    /// `else { .. }`
    Block(BlockId),
    /// `else if (..) { .. }`, held as the nested `if` statement.
    If(StmtId),
}

/// `while (cond) { .. } else { .. }`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoopStmt {
    /// The loop condition.
    pub condition: ExprId,
    /// The loop body.
    pub body: BlockId,
    /// The loop's `else` block, run only on normal termination.
    pub else_block: Option<BlockId>,
}

/// `for (init; cond; update) { .. } else { .. }`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ForCStmt {
    /// The initialiser, if written.
    pub init: Option<StmtId>,
    /// The condition, if written. An omitted condition means "always true".
    pub condition: Option<ExprId>,
    /// The update expression, if written.
    pub update: Option<ExprId>,
    /// The loop body.
    pub body: BlockId,
    /// The loop's `else` block, run only on normal termination.
    pub else_block: Option<BlockId>,
}

/// `switch (expr) { .. }`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchStmt {
    /// The scrutinee.
    pub scrutinee: ExprId,
    /// The arms, in source order. `default` may appear anywhere.
    pub arms: Vec<SwitchArm>,
}

/// One `case` or `default` arm.
///
/// Cases do not fall through to the next arm unless the arm's body ends with a
/// [`StmtKind::Fallthrough`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchArm {
    /// The labels, or `None` for `default`.
    ///
    /// A `case` may list several constants, as in `case c, d:`. A label must be
    /// a compile-time constant; the checker does not yet reject a non-constant
    /// one.
    pub labels: Option<Vec<ExprId>>,
    /// The statements in this arm.
    pub body: Vec<StmtId>,
    /// The arm's source range.
    pub span: Span,
    /// Attributes after the labels: `case 1 @likely:`.
    pub attrs: Vec<super::Attr>,
}

impl SwitchArm {
    /// Returns `true` if this is the `default` arm.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.labels.is_none()
    }
}

/// `return expr;` or `return;`
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReturnStmt {
    /// The returned expression, absent for a bare `return;`.
    pub value: Option<ExprId>,
}

/// `except` after a function's return type: it may raise an exception.
///
/// The same word is the query, `except(expr)`, which is a compile-time `bool`.
///
/// The condition is also a claim -- `??(cond)` says it may raise only when
/// `cond` holds -- and it is evaluated by the checker, which is the only place
/// that can answer whether a particular call needs protecting. Nothing about it
/// reaches the emitted function: the annotation is about what may leave the
/// body, not about the ABI.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Throws {
    /// The condition, absent for a plain `??`.
    pub condition: Option<ExprId>,
    /// The annotation's own source range.
    pub span: Span,
}

