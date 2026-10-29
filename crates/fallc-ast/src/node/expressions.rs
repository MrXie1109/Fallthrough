//! Expression nodes: what an expression is made of.
//!
//! Every expression is a small structure in the arena, and [`ExprKind`] names
//! which one a node is. The operators' spellings live here as well, because an
//! operator's meaning is part of the expression it appears in.

use super::*;

/// A character literal.
///
/// The source text is kept rather than the decoded code point, so that escape
/// decoding lives in one place. A width prefix on the literal, as in `u16'x'`,
/// selects the character type; without one it is a `c8`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CharLiteral {
    /// The literal exactly as written, including the quotes and any escapes.
    pub text: String,
    /// The width written before the literal, as a bit count, if any.
    pub width: Option<u32>,
}

/// A byte literal.
/// An integer literal.
///
/// The digits are kept as text rather than a parsed value because the numeric
/// grammar's value range is not settled; the checker parses them when it needs
/// the value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntLiteral {
    /// The digits, without any suffix.
    pub digits: String,
    /// The type the suffix selected, or `None` for the default.
    ///
    /// `None` means `int`. `Some("u8")` means the literal is a `u8`, and that
    /// is its type — the suffix is **not** a conversion, so `x u8 = 5u32;` is a
    /// type mismatch rather than a silent truncation.
    pub suffix: Option<String>,
}

/// A floating-point literal.
///
/// The text is kept rather than a parsed value, for the same reason an integer
/// literal's is: the numeric grammar is the lexer's business and the type
/// checker only needs to know which type the suffix selected.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FloatLiteral {
    /// The literal exactly as written, including any suffix.
    pub text: String,
    /// The type the suffix selected, or `None` for the default.
    ///
    /// `None` means `double`; `Some("float")` means the `f` suffix was written.
    pub suffix: Option<String>,
}

/// The comma operator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommaExpr {
    /// Evaluated for its effects and discarded.
    pub lhs: ExprId,
    /// The value of the whole expression.
    pub rhs: ExprId,
}

/// `++` or `--`, before or after its operand.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IncDecExpr {
    /// Whether this is an increment or a decrement.
    pub op: IncDecOp,
    /// Whether the operator was written after the operand.
    pub postfix: bool,
    /// The operand, which must be an lvalue.
    pub operand: ExprId,
}

/// Which direction an [`IncDecExpr`] moves.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum IncDecOp {
    /// `++`
    Increment,
    /// `--`
    Decrement,
}

impl IncDecOp {
    /// Returns the source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            IncDecOp::Increment => "++",
            IncDecOp::Decrement => "--",
        }
    }
}

/// A binary operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BinaryExpr {
    /// The operator, as written.
    pub op: BinaryOp,
    /// The left operand.
    pub lhs: ExprId,
    /// The right operand.
    pub rhs: ExprId,
}

/// An array literal: `{1, 2, 3}`.
///
/// The length is the number of elements, so it is not written: `{1, 2, 3}` is a
/// `[3]int` once the element type is known. It is written with braces rather
/// than `.{ .. }`, which is the aggregate initialiser and takes its type from
/// where it is written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArrayLiteral {
    /// The elements, in source order.
    pub elements: Vec<ExprId>,
}

/// A range: `low .. high` or `low ..= high`.
///
/// It is sugar for the standard library's `Range` construction: `1 .. 10` is
/// `Range(1, 10, 1)` and `1 ..= 10` is `Range(1, 11, 1)`, so the high end is
/// excluded in the first form and included in the second. Written here as its
/// own expression rather than a call so that the two ends are checked as
/// integers and the range can be recognised in the positions where it means a
/// walk -- a `for` header, or an argument that expects a `Range`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RangeExpr {
    /// The low end.
    pub low: ExprId,
    /// The high end, as written.
    pub high: ExprId,
    /// Whether the high end is included.
    pub inclusive: bool,
}

/// A prefix operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnaryExpr {
    /// The operator, as written.
    pub op: UnaryOp,
    /// The operand.
    pub operand: ExprId,
}

/// An assignment.
///
/// `target op= value` is represented as an assignment whose `op` is `Some`,
/// which keeps one node for both forms. The target must be an lvalue, which the
/// checker enforces.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AssignExpr {
    /// The assigned-to expression, which must denote storage.
    pub target: ExprId,
    /// The operator of a compound assignment, or `None` for plain `=`.
    pub op: Option<BinaryOp>,
    /// The value assigned.
    pub value: ExprId,
}

/// A template parameter pack named inside a body that is being instantiated.
///
/// A pack is not a value but a list of them, so every use of one is answered
/// while the instantiation is known: `(values + ...)` combines the elements,
/// `sizeof...(values)` counts them, and `values[1]` picks one out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PackRef {
    /// The pack's name.
    pub pack: String,
    /// The span of the name.
    pub pack_span: Span,
}

/// A fold over a template parameter pack.
///
/// Four forms, as in C++: the two unary ones take the elements alone and the two
/// binary ones take a starting value as well, and each pair differs in which end
/// the combination starts from.
///
/// ```text
/// (pack op ...)            unary right:  e1 op (e2 op (.. op eN))
/// (... op pack)            unary left:   ((e1 op e2) op ..) op eN
/// (pack op ... op init)    binary right: e1 op (e2 op (.. op (eN op init)))
/// (init op ... op pack)    binary left:  (((init op e1) op e2) op ..) op eN
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FoldExpr {
    /// The pack being folded.
    pub pack: PackRef,
    /// The operator that combines the elements.
    pub op: BinaryOp,
    /// Which of the four forms this is.
    pub kind: FoldKind,
    /// The starting value of a binary fold.
    pub init: Option<ExprId>,
}

/// Which fold a [`FoldExpr`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoldKind {
    /// `(pack op ...)` — the rightmost element is combined first.
    UnaryRight,
    /// `(... op pack)` — the leftmost element is combined first.
    UnaryLeft,
    /// `(pack op ... op init)` — a binary fold starting from `init`, rightmost
    /// element first.
    BinaryRight,
    /// `(init op ... op pack)` — a binary fold starting from `init`, leftmost
    /// element first.
    BinaryLeft,
}

/// Member access through a value or a pointer.
///
/// `pointer->field` is sugar for `(*pointer).field`, so the arrow form records
/// that a dereference must happen as well.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FieldExpr {
    /// The value or pointer being accessed.
    pub object: ExprId,
    /// The field's name.
    pub field: String,
    /// Whether the arrow form was written, which implies a dereference.
    pub through_pointer: bool,
}

/// Indexing, as in `a[i]`.
///
/// The object may be an array, a pointer, or a type that defines
/// `__getitem__`. Which one it is decided by the checker, because the same
/// syntax has to serve all three.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexExpr {
    /// The array, pointer, or indexable value.
    pub object: ExprId,
    /// The subscript.
    pub index: ExprId,
}

/// A call.
///
/// The arguments are positional: the language has no named arguments. The
/// callee is an expression, so a name or a member access both serve, and a
/// template argument list is written between the callee and the parenthesis.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CallExpr {
    /// What is being called.
    pub callee: ExprId,
    /// The arguments, in order.
    pub args: Vec<ExprId>,
    /// The arguments written by name, `.name = value`.
    ///
    /// They come after every positional one, so the positional list stays a
    /// prefix of the parameter list and only the tail is ever named.
    pub named: Vec<NamedArgument>,
    /// The explicit template arguments, `f<int>(..)`.
    ///
    /// Empty for an ordinary call, and also for a template that is inferred:
    /// a template argument list is only written when deduction cannot supply
    /// one, or when it would be ambiguous. Which is which is decided by the
    /// checker, not here.
    pub template_args: Vec<TemplateArgumentSyntax>,
    /// Whether a template argument list was written at all.
    ///
    /// `f<>()` and `f()` have the same (empty) argument list, and they mean
    /// different things: the first supplies no arguments to the parameters,
    /// and the second asks deduction for them. Only a pack can be given
    /// nothing, so only a pack call can tell the two apart.
    pub explicit_template_arguments: bool,
}

/// One named argument, `.name = value`.
///
/// Written after every positional argument of a call or an initialiser, so that
/// the positional ones still read as the parameters they are: `f(1, .b = 2)`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NamedArgument {
    /// The parameter or field the value is for.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The value written after `=`.
    pub value: ExprId,
    /// The whole `.name = value`.
    pub span: Span,
}

/// One written template argument, as it appears in `f<int, 3>(..)`.
///
/// Kept as syntax because what it *means* depends on the parameter it fills: a
/// bare `3` is a value of whatever type the parameter declares, and `int` is a
/// type. Which is which is the checker's question. A character sequence --
/// `f<"text">()` -- is written as a string literal and read as one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateArgumentSyntax {
    /// A type, written as a type is.
    Type(TypeSyntax),
    /// A value, written as an expression: an integer, or a string literal.
    Value(ExprId),
    /// A dotted name, which is a type's or a constant's, and only the parameter
    /// says which.
    ///
    /// `Box<std.String>` names a type and `Device<OpenMode.READ>` names the
    /// constant `READ`; the two are written alike -- a name, a dot, a name --
    /// and the grammar has no way to choose. The path is kept as written, and
    /// the parameter's kind is what reads it.
    Named {
        /// The path, with its dots.
        path: String,
        /// Where it was written.
        span: fallc_span::Span,
    },
}

impl TemplateArgumentSyntax {
    /// Whether this argument is a pack expansion, written `Args...`.
    ///
    /// Only a type can carry the marker, since `...` is written after a type's
    /// name; a value argument expands by naming the pack it came from, which
    /// the parser cannot tell from an ordinary expression.
    #[must_use]
    pub fn is_expansion(&self) -> bool {
        matches!(self, TemplateArgumentSyntax::Type(TypeSyntax::Named(named)) if named.is_expansion)
    }
}

/// A binary operator.
///
/// The set and the precedence relationships are C's; see
/// `fallc-parser::precedence` for the table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `**` — exponentiation, which is right-associative.
    Pow,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<=>` — the three-way comparison, yielding a sign.
    Cmp,
    /// `&&` — short-circuiting.
    LogicalAnd,
    /// `||` — short-circuiting.
    LogicalOr,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
}

impl BinaryOp {
    /// Returns `true` if this operator short-circuits.
    ///
    /// A short-circuiting operator must be lowered with control flow, because
    /// the right operand is evaluated conditionally rather than always.
    #[must_use]
    pub const fn is_short_circuit(self) -> bool {
        matches!(self, BinaryOp::LogicalAnd | BinaryOp::LogicalOr)
    }

    /// Returns `true` if this operator yields a boolean.
    #[must_use]
    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        )
    }

    /// Returns `true` if this operator can overflow.
    ///
    /// Used to decide where a debug-mode overflow check is needed.
    #[must_use]
    pub const fn can_overflow(self) -> bool {
        matches!(self, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
    }

    /// Returns `true` if this operator can divide by zero.
    #[must_use]
    pub const fn can_divide_by_zero(self) -> bool {
        matches!(self, BinaryOp::Div | BinaryOp::Rem)
    }

    /// Returns the source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Cmp => "<=>",
            BinaryOp::LogicalAnd => "&&",
            BinaryOp::LogicalOr => "||",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Pow => "**",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
        }
    }
}

/// A prefix operator.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `+`
    Plus,
    /// `!`
    Not,
    /// `~`
    BitNot,
    /// `&` — take the address of an lvalue.
    AddressOf,
    /// `*` — dereference a pointer or reference.
    Deref,
}

impl UnaryOp {
    /// Returns the source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Plus => "+",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::AddressOf => "&",
            UnaryOp::Deref => "*",
        }
    }
}

/// An explicit type conversion.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CastExpr {
    /// The value being converted.
    pub operand: ExprId,
    /// The type being converted to, as written.
    pub ty: TypeSyntax,
}

/// `call()??` -- run the call and let whatever it raises keep going.
///
/// It is the same as `try { call(); } except { raise; }`, and legal only where
/// raising is allowed at all: the propagation has to end somewhere, and a
/// function that cannot raise cannot be the place. It is an *expression* rather
/// than a statement so that the value survives: `v int = inner(n)??;`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PropagateExpr {
    /// The expression whose exception is passed on.
    pub value: ExprId,
}

/// `call()!!` -- the value, or the program stops.
///
/// It is `try { call() } except { std.panic(); }` written as a postfix, for the
/// failure a program has decided is not worth handling: a missing resource at
/// start-up, a case that cannot happen. Unlike `?` it needs no `except` on the
/// function, because nothing leaves it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AbortExpr {
    /// The expression that must not raise.
    pub value: ExprId,
}

/// `raise "type": "what";` or `raise;`
///
/// Both operands are expressions, and the checker requires each to be a string:
/// the exception's type and the message.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RaiseStmt {
    /// The exception type literal, absent for a re-raise.
    pub exception_type: Option<ExprId>,
    /// The message literal, absent for a re-raise.
    pub message: Option<ExprId>,
}

impl RaiseStmt {
    /// Returns `true` if this is a bare `raise;`, which re-raises.
    #[must_use]
    pub fn is_reraise(&self) -> bool {
        self.exception_type.is_none() && self.message.is_none()
    }
}

/// `try { .. } except { .. } else { .. } finally { .. }`
///
/// Every clause except the `try` body is optional.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TryStmt {
    /// The guarded block.
    pub body: BlockId,
    /// The `except` clauses, in the order they are written.
    ///
    /// Each clause may name the exceptions it catches; a clause with no names
    /// catches every exception. The first matching clause runs, so order is
    /// part of the meaning.
    pub except_clauses: Vec<ExceptClause>,
    /// The `else` block, run when the body completes without raising.
    pub else_block: Option<BlockId>,
    /// The `finally` block, run in every case.
    pub finally_block: Option<BlockId>,
}

/// One `except` clause of a `try`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExceptClause {
    /// The exception names this clause catches, as expressions.
    ///
    /// `except ("KeyError") { .. }` compares the exception's own name with each
    /// of these; empty means a bare `except`, which catches every exception.
    /// Names are expressions so that the checker types them like any other.
    pub names: Vec<ExprId>,
    /// The block that runs when this clause matches.
    pub block: BlockId,
    /// Where the clause was written, for diagnostics.
    pub span: Span,
}

/// A designated initialiser with its type written out.
/// A designated initialiser with its type written out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypedInitExpr {
    /// The type's name, as written.
    pub type_name: String,
    /// The template arguments, empty for an ordinary type.
    ///
    /// `Array<int>{}` names one member of a family; without them the name
    /// would be a template, which has no fields to initialise.
    pub type_arguments: Vec<TemplateArgumentSyntax>,
    /// The span of the name alone.
    pub type_span: Span,
    /// The elements, in source order.
    pub init: DesignatedInit,
}

/// The kinds of expression.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExprKind {
    /// An integer literal.
    IntLiteral(IntLiteral),
    /// A character literal, such as `'x'`, whose value is a code point.
    CharLiteral(CharLiteral),
    /// A floating-point literal, such as `1.5` or `2.0f`.
    FloatLiteral(FloatLiteral),
    /// A string literal that does **not** end with a zero, such as `b"Hello"`.
    ///
    /// An ordinary string gains a terminator so it can be handed to C; the `b`
    /// prefix suppresses that, for a string whose length is the point.
    ByteStringLiteral(String),
    /// A string literal, holding its exact source text including any escapes.
    ///
    /// Undecoded for the same reason as [`ExprKind::IntLiteral`].
    StringLiteral(String),
    /// A raw string literal: `r"..."` or `r#tag"..."tag#`.
    ///
    /// The text is the source between the delimiters, and nothing in it is an
    /// escape: `r"C:\dir"` is the six characters `C:\dir`. The delimiter form
    /// exists so that the text can contain a quote, which the undelimited form
    /// cannot.
    RawStringLiteral(String),
    /// `true` or `false`.
    BoolLiteral(bool),
    /// `nullptr` — the null pointer.
    ///
    /// It has no type of its own: it is the null value of whatever pointer type
    /// it is used at, which is why it is a literal rather than a constant of
    /// some particular pointer type.
    NullPointer,
    /// A name, such as `main`.
    Name(String),
    /// The comma operator: evaluate `lhs`, discard it, yield `rhs`.
    Comma(CommaExpr),
    /// A binary operation: `lhs op rhs`.
    Binary(BinaryExpr),
    /// A range: `low .. high` or `low ..= high`.
    Range(RangeExpr),
    /// An array literal: `{1, 2, 3}`.
    ArrayLiteral(ArrayLiteral),
    /// A prefix operation: `op operand`.
    Unary(UnaryExpr),
    /// An assignment: `target = value`, or `target op= value`.
    Assign(AssignExpr),
    /// A call: `callee(args)`.
    Call(CallExpr),
    /// A typed designated initialiser, `Point{42, 37}`.
    ///
    /// The same as `.{ 42, 37 }` but with the type written out, which is what
    /// makes it usable where nothing supplies the type: `p auto = Point{42, 37}`
    /// works, while `p auto = .{42, 37}` cannot be deduced.
    TypedInit(TypedInitExpr),

    /// A nested designated initialiser used as an expression, as an element of
    /// an outer one.
    ///
    /// Its type comes from the field it initialises, so it carries no type of
    /// its own.
    NestedInit(DesignatedInit),
    /// Member access: `object.field`, or `pointer->field` for `(*pointer).field`.
    Field(FieldExpr),
    /// Indexing: `object[index]`.
    ///
    /// An array or pointer is indexed directly; any other type needs
    /// `__getitem__`, which is why this is an expression form rather than a
    /// rewrite into pointer arithmetic.
    Index(IndexExpr),
    /// A pre- or post-increment or decrement.
    ///
    /// Either way the operand must be an lvalue, and either way it is modified;
    /// only the value the expression yields differs.
    IncDec(IncDecExpr),
    /// `expr as Type` — an explicit type conversion.
    ///
    /// Conversion is always explicit: there are no implicit conversions in the
    /// language, so this node is the *only* way a value changes type. The one
    /// apparent exception is a truthiness test in a condition, which accepts
    /// more than a boolean without converting anything.
    Cast(CastExpr),
    /// `cond ? then : otherwise` — the conditional operator.
    ///
    /// Right-associative, so `a ? b : c ? d : e` is `a ? b : (c ? d : e)`. Only
    /// one of the two branches is evaluated, which is why this is a node of its
    /// own rather than a call to some function.
    Conditional(ConditionalExpr),
    /// `call()?` -- the value, unless the call raises, in which case the
    /// exception leaves the enclosing function.
    Propagate(PropagateExpr),
    /// `call()!!` -- the value, unless the call raises, in which case the
    /// program stops: the panic is the whole answer, and there is no exception
    /// left to pass on or to catch.
    Abort(AbortExpr),
    /// `type(expr)` written where an expression is expected -- a *type*, not a
    /// value.
    ///
    /// It exists so that a trait query can name a type it could not otherwise
    /// spell: `InputIterator(type(a.begin()))` asks about the type of a value.
    /// Everywhere else a type is not a value and this form is refused.
    TypeOf(ExprId),
    /// A written *type* where an expression is expected -- again a type, not a
    /// value.
    ///
    /// `type(expr)` answers "what is this value's type", but a trait can also
    /// be asked about a type that no value has *yet*, or that cannot be written
    /// as an expression at all: `Iterable(Array<int>)`. The parser reads the
    /// argument with the type grammar when it looks like a template instance,
    /// and this node carries what it read. Only a query's argument accepts it.
    TypeLiteral(Box<TypeSyntax>),
    /// `except(expr)` -- a compile-time `bool`: whether the expression may raise.
    ///
    /// The expression is *analysed*, never evaluated: it exists to ask a
    /// question about a type, which is what `std.fake` is for.
    ThrowQuery(ExprId),
    /// `(values + ...)` — a fold over a parameter pack.
    ///
    /// The pack's elements are combined left to right with the operator, and
    /// the result is a value of the same type. Written as `(name op ...)`: the
    /// parentheses say where the fold begins and ends, and the `...` stands for
    /// "every remaining element", so the shape cannot be confused with an
    /// ordinary binary expression.
    Fold(FoldExpr),
    /// `values...` — every element of a parameter pack, in an argument list.
    ///
    /// Only a call's argument list accepts it: the elements are the parameters
    /// the callee receives, one per element, and at the call site that is the
    /// only place a list of values is wanted.
    PackExpand(ExprId),
    /// `(pattern, ...)` — repeat `pattern` for every element of a pack.
    ///
    /// The pattern names the pack, and each repetition sees the next element:
    /// `(this.write(values), ...)` writes every value in turn. The name the
    /// pattern uses is the pack parameter of the enclosing function.
    CommaFold {
        /// The expression to repeat.
        pattern: ExprId,
    },
    /// `sizeof...(values)` — how many elements a parameter pack has.
    ///
    /// A count rather than a size in bytes: a pack is a list of values that
    /// exists only while the instantiation is known, so what can be asked of it
    /// is how long it is, and the answer is a `usize` constant.
    SizeofPack(PackRef),
    /// `sizeof(T)` — how many bytes a value of `T` occupies.
    ///
    /// The operand is a *type*, always parenthesised. It is a compile-time
    /// constant, which is what makes it usable as an array length.
    Sizeof(TypeSyntax),
    /// `alignof(T)` — the alignment `T` is required to have.
    Alignof(TypeSyntax),
    /// `lambda(args) -> expr` or `lambda(args) { .. }` — an anonymous function.
    Lambda(LambdaExpr),
    /// `@__builtin_defined(X)` and its kind: a question put to the compiler
    /// while compiling.
    ///
    /// It is a constant, so it decides `if @constexpr` and `@enable_if`. The
    /// argument is written as a name and read by the builtin, which is why it
    /// is kept as text rather than resolved like a value.
    Builtin(BuiltinQuery),
    /// `if (cond) { .. } else { .. }` standing where a value is wanted.
    ///
    /// Both branches are blocks, and the value is whichever branch ran. The
    /// two must have the same type; an `if` with no `else` has no value to
    /// give when the condition is false, so it produces nothing.
    IfValue(IfValueExpr),
    /// `{ .. expr }` — a block standing where a value is wanted.
    ///
    /// Its value is its last expression when that one leaves the `;` out, and
    /// it produces nothing otherwise. The block runs its statements in order,
    /// so it is also the way to compute a value through a few temporary steps:
    /// `v int = { t int = 3; t * 2 };`.
    Block(BlockId),
    /// An expression the parser could not recognise.
    Unresolved(Unresolved),
}

/// A `lambda` expression: an anonymous function written where a value is
/// expected.
///
/// The parameter types may be left out, in which case each call decides them
/// from its own arguments, exactly as a template's parameters are decided. A
/// name used in the body that belongs to the enclosing function is captured by
/// reference: the lambda sees the variable, not a copy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LambdaExpr {
    /// The parameters, in order.
    pub parameters: Vec<LambdaParam>,
    /// The body.
    pub body: LambdaBody,
    /// The source range of the whole lambda, `lambda` included.
    pub span: Span,
}

/// One parameter of a [`LambdaExpr`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LambdaParam {
    /// The parameter's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The declared type, when one is written.
    ///
    /// `None` means the type is decided by each call, so a lambda written once
    /// with `lambda(x) -> x` may be called with an `int` and with a `double`.
    pub ty: Option<TypeSyntax>,
    /// The whole parameter's source range.
    pub span: Span,
}

/// The body of a [`LambdaExpr`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LambdaBody {
    /// `-> expr`: the expression's value is the lambda's result.
    Expression(ExprId),
    /// `{ .. }`: a body like a function's, which returns with `return`.
    Block(BlockId),
}

/// A compile-time question: `@__builtin_defined(X)`, `@meta.defined(X)`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BuiltinQuery {
    /// The builtin's name, without the `@`.
    pub name: String,
    /// What it asks about, as written.
    pub argument: String,
    /// The whole expression's source range.
    pub span: Span,
}

/// `if (cond) { .. } else { .. }` used as an expression.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IfValueExpr {
    /// The condition, tested for truth.
    pub condition: ExprId,
    /// The branch taken when the condition holds.
    pub then_block: BlockId,
    /// The branch taken otherwise, when there is one.
    pub else_block: Option<BlockId>,
}

/// `cond ? then : otherwise`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConditionalExpr {
    /// The condition, tested for truth.
    pub condition: ExprId,
    /// The value when the condition holds.
    pub then_value: ExprId,
    /// The value when it does not.
    pub else_value: ExprId,
}

