//! Type checking.
//!
//! Operates on the AST plus the [`Resolutions`] produced by
//! [`crate::resolve`], and produces a [`TypeTable`]. It never mutates the AST.
//!
//! # Inference
//!
//! Type checking is written as a bidirectional walk (`check` against an
//! expected type, `infer` for a synthesised type) because that is the shape
//! that handles both annotated and unannotated constructs without needing
//! unification variables everywhere. The language's only inference is `auto`,
//! in a binding's initialiser and in a function's return type; everything else
//! is written.
//!
//! Every construct is checked against the type rule the language gives it, and
//! the only implicit conversions are the ones in `is_compatible`: adding
//! `const` to an indirection, an array decaying to a pointer, a function to a
//! pointer, the byte-pointer conversion, and an unsuffixed literal adopting the
//! type its context requires.

use fallc_ast::{
    AssignExpr, Ast, BinaryExpr, BinaryOp, CallExpr, ExprId, ExprKind, FieldExpr, UnaryOp,
};
use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_span::Span;

use crate::resolve::{FunctionSignature, Resolutions};
use crate::types::{BindingTypes, ConversionMethod, Type, TypeTable, is_byte_pointer_conversion};

/// Returns `true` if `found` is `expected` with strictly more `const`
/// qualifiers on its indirections.
///
/// This is the only implicit conversion the language has besides arithmetic
/// literals taking their context's type: a pointer or reference to a mutable
/// target may be used where one to a constant target is required, because that
/// only takes away the right to write.
///
/// Recursion means nesting works: `**T` may become `*const *const T`, and each
/// level may add `const` independently.
///
/// Dropping `const` is not allowed here. It is available as an explicit `as`,
/// which is how a program states that it knows what it is doing.
#[must_use]
pub fn is_const_widening(found: &Type, expected: &Type) -> bool {
    match (found, expected) {
        // A pointer may gain `const` on its pointee, and may not lose the
        // pointee's existing `const`.
        (
            Type::Pointer {
                pointee: found_pointee,
                pointee_is_const: found_const,
                pointer_is_const: found_pointer_const,
            },
            Type::Pointer {
                pointee: expected_pointee,
                pointee_is_const: expected_const,
                pointer_is_const: expected_pointer_const,
            },
        ) => {
            // Constness only accumulates: a mutable source may become constant,
            // a constant one may not become mutable.
            let pointee_ok = found_const == expected_const || (*expected_const && !*found_const);
            let pointer_ok = found_pointer_const == expected_pointer_const
                || (*expected_pointer_const && !*found_pointer_const);
            pointee_ok
                && pointer_ok
                && (found_pointee == expected_pointee
                    || is_const_widening(found_pointee, expected_pointee))
        }
        // A reference behaves the same way.
        (
            Type::Reference {
                referent: found_referent,
                referent_is_const: found_const,
                is_rvalue: found_rvalue,
            },
            Type::Reference {
                referent: expected_referent,
                referent_is_const: expected_const,
                is_rvalue: expected_rvalue,
            },
        ) => {
            let referent_ok = found_const == expected_const || (*expected_const && !*found_const);
            // An rvalue reference and an lvalue reference are different types,
            // and neither widens to the other: `T&&` takes a temporary and
            // `T&` takes something that outlives the call, so treating one as
            // the other would let a temporary be stored where it cannot live.
            referent_ok
                && found_rvalue == expected_rvalue
                && (found_referent == expected_referent
                    || is_const_widening(found_referent, expected_referent))
        }
        // An array's element may widen, since `[N]*T` to `[N]*const T` is the
        // same idea one level down.
        (
            Type::Array {
                element: found_element,
                len: found_len,
            },
            Type::Array {
                element: expected_element,
                len: expected_len,
            },
        ) => {
            found_len == expected_len
                && (found_element == expected_element
                    || is_const_widening(found_element, expected_element))
        }
        _ => false,
    }
}

/// Returns `true` if a written type has an `auto` in it.
///
/// `auto` may stand alone or under a modifier: `&auto`, `*auto`, `const auto`,
/// `[4]auto`. Any of them is a hole in the type to be filled from the
/// initialiser.
/// Returns `true` when a statement always leaves the block.
///
/// Only the forms that leave *whatever* the program holds are counted: a
/// `return`, a jump, or a `raise`. An `if` whose branches both leave does too,
/// but deciding that needs the branches to be analysed rather than read, and a
/// warning that is sometimes wrong is worse than one that is sometimes
/// missing -- the point is to catch the statement behind a `return`, which is
/// never intended.
fn leaves(kind: &fallc_ast::StmtKind) -> bool {
    match kind {
        fallc_ast::StmtKind::Return(_)
        | fallc_ast::StmtKind::Break
        | fallc_ast::StmtKind::Continue
        | fallc_ast::StmtKind::Fallthrough
        | fallc_ast::StmtKind::Raise(_) => true,
        // A block leaves when its own last statement is one of those, which is
        // what `{ return; }` written as a statement means.
        fallc_ast::StmtKind::Block(_) | fallc_ast::StmtKind::Defer(_) => false,
        _ => false,
    }
}

/// Returns the value of a condition written as a literal.
///
/// Only a written `true`, `false`, or an integer literal counts: a `const` used
/// as a condition is a deliberate configuration switch, and warning about it
/// would be warning about the pattern rather than about a mistake.
/// Returns `true` when two expressions name the same place.
///
/// Structural rather than name-based: `a.b == a.b` is one place written twice,
/// and so is `*p == *p`. Two *different* names that resolve to the same binding
/// (`a` and a reference to it) are not caught, which is the right trade -- a
/// warning that fires on two names cannot tell which of them the author meant.
/// Returns `true` if `callee` names the library's explicit move.
///
/// `std.move(v)`, `std.util.move(v)` and a bare `move(v)` all do; a *method*
/// called `move` (`player.move(dx)`) does not, because its qualifier is a value
/// rather than the standard library.
fn is_move_call(ast: &Ast, callee: fallc_ast::ExprId) -> bool {
    use fallc_ast::ExprKind;
    match &ast.expr(callee).kind {
        ExprKind::Name(name) => name == "move",
        ExprKind::Field(field) if field.field == "move" => match &ast.expr(field.object).kind {
            ExprKind::Name(object) => object == "std" || object == "util",
            ExprKind::Field(inner) => {
                inner.field == "util"
                    && matches!(&ast.expr(inner.object).kind, ExprKind::Name(name) if name == "std")
            }
            _ => false,
        },
        _ => false,
    }
}

/// Returns the name a `Name` expression writes, if it is one.
fn name_expression(ast: &Ast, id: fallc_ast::ExprId) -> Option<String> {
    match &ast.expr(id).kind {
        fallc_ast::ExprKind::Name(name) => Some(name.clone()),
        _ => None,
    }
}

/// Collects the reads, the written targets, and the moves of one subtree.
///
/// A read is every name occurrence; a target is a name written by a plain
/// assignment, which is not a read; a move is a name `std.move(..)` was applied
/// to, recognised by the span the checker recorded.
struct MoveScan {
    reads: Vec<fallc_ast::ExprId>,
    targets: Vec<fallc_ast::ExprId>,
    moves: Vec<String>,
}

impl MoveScan {
    fn new() -> Self {
        MoveScan {
            reads: Vec::new(),
            targets: Vec::new(),
            moves: Vec::new(),
        }
    }

    fn finish(self) -> (Vec<fallc_ast::ExprId>, Vec<fallc_ast::ExprId>, Vec<String>) {
        (self.reads, self.targets, self.moves)
    }
}

impl fallc_ast::visit::Visitor for MoveScan {
    fn visit_expr(&mut self, ast: &Ast, id: fallc_ast::ExprId) {
        match &ast.expr(id).kind {
            fallc_ast::ExprKind::Name(_) => self.reads.push(id),
            // `std.move(v)` -- and `move(v)`, for a program that imports it
            // directly. The callee is recognised by its name: a method call
            // `v.move(x)` is qualified by a *value*, which is not what this is.
            fallc_ast::ExprKind::Call(call) if is_move_call(ast, call.callee) => {
                if let Some(&argument) = call.args.first()
                    && let Some(name) = name_expression(ast, argument)
                {
                    self.moves.push(name);
                }
            }
            fallc_ast::ExprKind::Assign(assign)
                if assign.op.is_none() && name_expression(ast, assign.target).is_some() =>
            {
                self.targets.push(assign.target);
            }
            _ => {}
        }
    }
}

fn same_place(ast: &Ast, left: fallc_ast::ExprId, right: fallc_ast::ExprId) -> bool {
    use fallc_ast::ExprKind;
    match (&ast.expr(left).kind, &ast.expr(right).kind) {
        (ExprKind::Name(one), ExprKind::Name(other)) => one == other,
        (ExprKind::Field(one), ExprKind::Field(other)) => {
            one.field == other.field
                && one.through_pointer == other.through_pointer
                && same_place(ast, one.object, other.object)
        }
        (ExprKind::Unary(one), ExprKind::Unary(other)) => {
            one.op == other.op && same_place(ast, one.operand, other.operand)
        }
        (ExprKind::Index(one), ExprKind::Index(other)) => {
            same_place(ast, one.object, other.object) && same_place(ast, one.index, other.index)
        }
        (ExprKind::IntLiteral(one), ExprKind::IntLiteral(other)) => one.digits == other.digits,
        _ => false,
    }
}

fn literal_condition(ast: &Ast, condition: fallc_ast::ExprId) -> Option<bool> {
    match &ast.expr(condition).kind {
        fallc_ast::ExprKind::BoolLiteral(value) => Some(*value),
        fallc_ast::ExprKind::IntLiteral(literal) => {
            let text = literal.digits.replace('_', "");
            let (radix, digits) = if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                (16, rest)
            } else if let Some(rest) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
                (8, rest)
            } else if let Some(rest) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
                (2, rest)
            } else {
                (10, text.as_str())
            };
            // A literal too large for `u64` is not something to guess about.
            u64::from_str_radix(digits, radix).ok().map(|value| value != 0)
        }
        _ => None,
    }
}

fn mentions_auto(ty: &fallc_ast::TypeSyntax) -> bool {
    use fallc_ast::TypeSyntax;
    match ty {
        TypeSyntax::Auto(_) | TypeSyntax::DecltypeAuto(_) => true,
        TypeSyntax::Array(array) => mentions_auto(&array.element),
        TypeSyntax::Member(member) => mentions_auto(&member.outer),
        // A function type mentions `auto` when one of its types does.
        TypeSyntax::Function(function) => {
            function.parameters.iter().any(mentions_auto) || mentions_auto(&function.returns)
        }
        TypeSyntax::UnsizedArray(inner)
        | TypeSyntax::Pointer(inner)
        | TypeSyntax::PointerToConst(inner)
        | TypeSyntax::ConstPointer(inner)
        | TypeSyntax::Reference(inner)
        | TypeSyntax::ConstReference(inner)
        | TypeSyntax::RvalueReference(inner)
        | TypeSyntax::ConstRvalueReference(inner)
        | TypeSyntax::Volatile(inner)
        | TypeSyntax::Const(inner) => mentions_auto(inner),
        // The operand is an expression, and a name is a type.
        TypeSyntax::Named(_) | TypeSyntax::TypeOf(_) | TypeSyntax::Unresolved(_) => false,
    }
}

/// Returns where the `auto` in a written type is, for a diagnostic.
///
/// The innermost one is taken: `&auto` answers the `auto`'s own span, which is
/// what a reader wants to see pointed at.
fn auto_span_in(ty: &fallc_ast::TypeSyntax) -> fallc_span::Span {
    use fallc_ast::TypeSyntax;
    match ty {
        TypeSyntax::Auto(span) | TypeSyntax::DecltypeAuto(span) => *span,
        TypeSyntax::Array(array) => auto_span_in(&array.element),
        TypeSyntax::Member(member) => auto_span_in(&member.outer),
        TypeSyntax::UnsizedArray(inner)
        | TypeSyntax::Pointer(inner)
        | TypeSyntax::PointerToConst(inner)
        | TypeSyntax::ConstPointer(inner)
        | TypeSyntax::Reference(inner)
        | TypeSyntax::ConstReference(inner)
        | TypeSyntax::RvalueReference(inner)
        | TypeSyntax::ConstRvalueReference(inner)
        | TypeSyntax::Volatile(inner)
        | TypeSyntax::Const(inner) => auto_span_in(inner),
        _ => ty.span(),
    }
}

/// Collects the array-length expressions among the written types of a statement.
///
/// An array length is a constant position, and it is resolved before the
/// statement's own expressions are checked -- so a `constexpr` template call in
/// one had not been instantiated yet. Checking the lengths first is what gives
/// them a value before the type they belong to is resolved.
fn collect_constant_positions(kind: &fallc_ast::StmtKind, out: &mut Vec<ExprId>) {
    // Only a *binding's* written types have constant positions. The condition
    // of an `if constexpr` is deliberately **not** collected here: this pass
    // runs over the whole block before any of its bindings are recorded, so a
    // condition naming one -- `if constexpr (Input(type(it)))` after
    // `it auto = ..;` -- folded against a name with no type yet, and the wrong
    // answer was cached under the condition's span. The condition is typed by
    // the statement itself, where it belongs, and a `constexpr` call in it is
    // instantiated by that same typing.
    if let fallc_ast::StmtKind::Bind(bind) = kind
        && let fallc_ast::BindingType::Written(ty) = &bind.ty
    {
        collect_type_lengths(ty, out);
    }
}

/// What a subscript resolved to.
enum IndexResolution {
    /// The element type the selected `__getitem__` returns.
    Found(Type),
    /// The type has no `__getitem__` at all.
    NoMethod,
    /// It has one, and the subscript does not fit its parameter: the type is
    /// the parameter's, which is what the reader needs to be told.
    IndexMismatch(Type),
}

/// Rounds `value` up to the next multiple of `alignment`.
fn round_up(value: i64, alignment: i64) -> i64 {
    if alignment <= 1 {
        return value;
    }
    // `i64::div_ceil` is still unstable, so the rounding is written out. A
    // value already on a boundary stays where it is.
    let steps = (value + alignment - 1) / alignment;
    steps.saturating_mul(alignment)
}

/// Whether a written type is an rvalue reference, `&&T` or `&&const T`.
///
/// That is the parameter a dying argument may be moved from, so it is the shape
/// overload resolution looks for.
fn is_rvalue_reference_syntax(ty: &fallc_ast::TypeSyntax) -> bool {
    matches!(
        ty,
        fallc_ast::TypeSyntax::RvalueReference(_)
            | fallc_ast::TypeSyntax::ConstRvalueReference(_)
    )
}

/// Collects the lengths of the array types inside one written type.
fn collect_type_lengths(ty: &fallc_ast::TypeSyntax, out: &mut Vec<ExprId>) {
    use fallc_ast::TypeSyntax;
    match ty {
        TypeSyntax::Array(array) => {
            out.push(array.len);
            collect_type_lengths(&array.element, out);
        }
        // A function type's parts are types too, and an array length may live
        // in one of them: `fn([N]int) void`.
        TypeSyntax::Function(function) => {
            for parameter in &function.parameters {
                collect_type_lengths(parameter, out);
            }
            collect_type_lengths(&function.returns, out);
        }
        TypeSyntax::UnsizedArray(inner)
        | TypeSyntax::Pointer(inner)
        | TypeSyntax::PointerToConst(inner)
        | TypeSyntax::ConstPointer(inner)
        | TypeSyntax::Reference(inner)
        | TypeSyntax::ConstReference(inner)
        | TypeSyntax::RvalueReference(inner)
        | TypeSyntax::ConstRvalueReference(inner)
        | TypeSyntax::Volatile(inner)
        | TypeSyntax::Const(inner) => collect_type_lengths(inner, out),
        TypeSyntax::Named(named) => {
            for argument in &named.arguments {
                if let fallc_ast::TemplateArgumentSyntax::Type(inner) = argument {
                    collect_type_lengths(inner, out);
                }
            }
        }
        // `type(expr)` and `type(auto)` name an expression, and an expression
        // has no array
        // length written in it.
        TypeSyntax::TypeOf(_) | TypeSyntax::DecltypeAuto(_) | TypeSyntax::Unresolved(_) => {}
        TypeSyntax::Auto(_) => {}
        TypeSyntax::Member(member) => collect_type_lengths(&member.outer, out),
    }
}

/// A constant expression, folded far enough to compute an array length.
///
/// This is a deliberately small language: enough integer arithmetic to write
/// `[4]int` and `[2 * 3]int`, and nothing that would require a value to exist
/// at runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
enum ConstExpr {
    /// An integer literal.
    Int(i64),
    /// `-x`
    Neg(ExprId),
    /// `~x`
    Not(ExprId),
    /// `!x`, folded as 0 or 1.
    LogicalNot(ExprId),
    /// `a ? b : c`, folded by folding the branch the condition selects.
    Conditional {
        /// The condition.
        condition: ExprId,
        /// The value when it holds.
        then_value: ExprId,
        /// The value when it does not.
        else_value: ExprId,
    },
    /// `a && b` / `a || b`, folded without evaluating the operand the answer
    /// does not depend on.
    ///
    /// Short-circuiting is not only about speed: the right operand of
    /// `n != 0 && 10 / n > 1` divides by zero when `n` is zero, and folding it
    /// regardless would perform a division the program never does.
    Logical {
        /// Whether the operator is `&&`.
        is_and: bool,
        /// The left operand.
        lhs: ExprId,
        /// The right operand.
        rhs: ExprId,
    },
    /// A comparison, which is what decides a compile-time `if`.
    Compare {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        lhs: ExprId,
        /// The right operand.
        rhs: ExprId,
    },
    /// A binary operation on constants.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        lhs: ExprId,
        /// The right operand.
        rhs: ExprId,
    },
    /// `sizeof(T)` — the size of a type, known before anything runs.
    Sizeof(fallc_ast::TypeSyntax),
    /// `alignof(T)` — the alignment of a type.
    Alignof(fallc_ast::TypeSyntax),
    /// `sizeof...(values)` — how many elements a pack has.
    SizeofPack(fallc_ast::PackRef),
    /// `(values + ...)` — the pack's elements combined with an operator.
    Fold(fallc_ast::FoldExpr),
    /// A call to a function, folded when the function and its arguments are
    /// all constant.
    Call {
        /// The callee's name.
        callee: String,
        /// The arguments, each folded in turn.
        args: Vec<ExprId>,
    },
    /// A name that stands for a constant: another `const`, or a value template
    /// parameter.
    ///
    /// Resolved through the definition rather than copied here, because a
    /// constant may be declared after the expression that uses it.
    Named {
        /// The expression naming it, so the folder can ask what it resolved to.
        name: String,
        /// Where the name was written.
        span: fallc_span::Span,
    },
    /// Anything else, which is not a constant.
    Unknown,
}

/// Returns `true` if a written type mentions one of the template's parameters.
///
/// Used to decide whether an instantiation's symbol already distinguishes it
/// from the template's other instantiations. If no parameter mentions one, the
/// mangled parameters are the same for every instantiation, and the arguments
/// have to go into the symbol instead.
fn mentions_type_parameter_syntax(
    ty: &fallc_ast::TypeSyntax,
    params: &[fallc_ast::TemplateParam],
) -> bool {
    use fallc_ast::TypeSyntax;
    let names_one = |path: &str| {
        params
            .iter()
            .any(|param| param.name == path && param.kind == fallc_ast::TemplateParamKind::Type)
    };
    match ty {
        // A name mentions a type parameter either by *being* one or by taking
        // one as an argument. Only the first was checked, so `Box<Args...>`
        // looked like a type mentioning nothing -- and the instantiation's key
        // was then appended to a symbol that already carried the arguments,
        // producing a name no definition was ever emitted under.
        TypeSyntax::Named(named) => {
            names_one(&named.path)
                || named.arguments.iter().any(|argument| match argument {
                    fallc_ast::TemplateArgumentSyntax::Type(ty) => {
                        mentions_type_parameter_syntax(ty, params)
                    }
                    fallc_ast::TemplateArgumentSyntax::Value(_) => false,
                    fallc_ast::TemplateArgumentSyntax::Named { .. } => false,
                })
        }
        TypeSyntax::Array(array) => mentions_type_parameter_syntax(&array.element, params),
        TypeSyntax::UnsizedArray(inner)
        | TypeSyntax::Pointer(inner)
        | TypeSyntax::PointerToConst(inner)
        | TypeSyntax::ConstPointer(inner)
        | TypeSyntax::Reference(inner)
        | TypeSyntax::ConstReference(inner)
        | TypeSyntax::RvalueReference(inner)
        | TypeSyntax::ConstRvalueReference(inner)
        | TypeSyntax::Volatile(inner)
        | TypeSyntax::Const(inner) => mentions_type_parameter_syntax(inner, params),
        TypeSyntax::Member(member) => mentions_type_parameter_syntax(&member.outer, params),
        TypeSyntax::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| mentions_type_parameter_syntax(parameter, params))
                || mentions_type_parameter_syntax(&function.returns, params)
        }
        // The operand is an expression, which cannot name a type parameter,
        // and `auto` is deduced from one.
        TypeSyntax::TypeOf(_)
        | TypeSyntax::DecltypeAuto(_)
        | TypeSyntax::Auto(_)
        | TypeSyntax::Unresolved(_) => false,
    }
}

/// Returns the bytes a string literal stands for.
///
/// The escapes are decoded, in `fallc_ast::literal::decode_escapes`: a string
/// literal is the bytes it stands for, so `"a\nb"` is three bytes and not four.
/// Code generation uses the same decoder, so a literal used as a template value
/// and the same literal used as a value cannot differ.
fn decode_string_literal(text: &str) -> Vec<u8> {
    // The prefixes a literal may carry -- `u16".."`, `b".."` -- are dropped, so
    // that the characters are what is left; `""".."""` keeps everything between
    // its three quotes. A prefix that names a width wider than a byte is still
    // read byte-wise; a value parameter is a character *sequence*, and a
    // sequence of wider characters does not fit one.
    //
    // The escapes are decoded, because a string literal is the bytes it stands
    // for: `"\n"` is one byte and not two. Code generation decodes them the same
    // way, so a string used as a template value and the same literal used as a
    // value cannot differ.
    fallc_ast::literal::decode_escapes(fallc_ast::literal::string_body(text))
}

/// Returns the characters a raw string literal holds.
///
/// The source text is `r"..."` or `r#tag"..."tag#`: everything from the first
/// `"` to the last `"` is the content, and nothing in it is an escape.
fn raw_string_body(text: &str) -> &str {
    let Some(open) = text.find('"') else {
        return "";
    };
    let Some(close) = text.rfind('"') else {
        return "";
    };
    if open >= close {
        return "";
    }
    &text[open + 1..close]
}

/// Returns the length of a raw string literal, including its terminator.
fn raw_string_length(text: &str) -> u64 {
    let body = raw_string_body(text);
    u64::try_from(body.len()).unwrap_or(0) + 1
}

/// Returns the length of a byte string literal, including its terminator.
///
/// The literal's source text includes the `b` prefix and the quotes, which are
/// not part of the value. The escapes are decoded first: `b"\n"` is one byte,
/// not two, and code generation emits the decoded bytes.
fn byte_string_length(text: &str) -> u64 {
    // A byte string carries no terminating zero: its length is its bytes.
    decode_string_literal(text).len() as u64
}

/// Returns the element type of a string literal, from its width prefix.
///
/// `"ab"` holds `c8`; `u16"ab"` holds `c16`.
fn string_element_type(text: &str) -> Type {
    match text.find('"') {
        Some(0) | None => Type::C8,
        Some(open) => match &text[..open] {
            "u16" => Type::C16,
            "u32" => Type::C32,
            _ => Type::C8,
        },
    }
}

/// Returns the number of elements a string literal holds, NUL included.
///
/// The trailing NUL is part of the literal's type: `"ab"` is `[3]c8`. That is
/// what makes an ordinary string usable as a C string, and it is why `b"ab"`,
/// which has no NUL, is `[2]c8`.
///
/// The count is of *decoded* elements, because that is what code generation
/// emits: `"a\nb"` is four elements, and a length taken from the source text
/// would have said five.
fn string_literal_length(text: &str) -> u64 {
    decode_string_literal(text).len() as u64 + 1
}

/// Returns the span of an expression.
fn node_span(ast: &Ast, expr: ExprId) -> fallc_span::Span {
    ast.expr(expr).span
}

/// One function a call brought into being from a template.
#[derive(Clone, Debug)]
pub struct Instantiation {
    /// The key its arguments make, which is what makes it unique.
    pub key: String,
    /// The symbol it is emitted under.
    pub symbol: String,
    /// The tree the template's body is in.
    ///
    /// A node id is an index into *one* tree, so the same number names
    /// different functions in different translation units. The tree travels
    /// with the function instead of being found by number.
    pub tree: Ast,
    /// The template itself, which is the body to emit.
    pub function: fallc_ast::FunctionItem,
    /// The parameters it was instantiated with, so the body's types resolve the
    /// same way they did while it was checked.
    pub arguments: crate::template::Arguments,
    /// The template's parameter list, in order.
    pub params: Vec<fallc_ast::TemplateParam>,
    /// The types of the template's *own* expressions under this
    /// instantiation.
    ///
    /// A type table is keyed by expression, and every instantiation shares the
    /// template's expressions. One table for all of them means the last
    /// instantiation checked overwrites the others, and lowering the first then
    /// reads the second's types -- which is how a `double` function came to
    /// contain `ret i32`. Each instantiation carries its own.
    pub types: TypeTable,
    /// The declared type of each of the template body's local bindings.
    ///
    /// A local like `fresh *T` has the *substituted* type -- `*i32` for
    /// `Array<i32>` -- and the binding types are keyed by the name's span, which
    /// the template shares with every other instantiation. They are kept per
    /// instantiation for the same reason the types are.
    pub bindings: BindingTypes,
    /// Every per-expression record this instantiation's check made.
    ///
    /// All of them are keyed by span, and every instantiation shares the
    /// template's spans -- so they have to be kept per instantiation or the
    /// last one checked overwrites the rest.
    pub records: InstantiationRecords,
    /// The instantiated type of each of the template's parameters, in order.
    ///
    /// Carried because a parameter's written type may not survive being read
    /// again: `Box<Args...>` names a pack expansion, and expanding it needs the
    /// substitution that only the checker has. Code generation is handed the
    /// answers instead of re-reading the syntax and finding `Box` alone.
    ///
    /// Empty when the parameters are ordinary, which is every template that
    /// does not mention a pack.
    pub resolved_params: Vec<Type>,
    /// The return type, resolved under the instantiation's arguments.
    ///
    /// The written return type is one expression in one tree, so its recorded
    /// resolution is the last instantiation's: a method `fn at(..) T` of a
    /// family had every member lowered as whichever member was checked last.
    pub resolved_returns: Type,
}

/// An explicit special-method call whose meaning is built in rather than a
/// declared function.
///
/// A primitive type owns nothing, so the compiler gives it the three lifetime
/// methods directly: `__init__` and `__assign__` store the value, and
/// `__drop__` does nothing. A declared type that writes none of them keeps the
/// language's implicit rules instead -- a member-wise copy for construction,
/// a member-wise store for assignment, field-wise destruction for a
/// destructor -- which are the same operations. Recording the operation rather
/// than a symbol is what keeps code generation from looking for a function
/// that was never declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InPlaceOperation {
    /// A built-in `__init__`/`__assign__`: storing the value is the whole
    /// operation. For a declared type with no `__init__` this is the same
    /// member-wise copy the language already performs.
    Store,
    /// A built-in `__drop__`: run whatever the value's own destructor and
    /// fields need. A primitive needs nothing, so this emits nothing.
    Drop,
}

/// The records one instantiation's check made, which code generation reads.
///
/// A template's body is one set of expressions shared by every instantiation,
/// and everything a check resolves is filed by span. Two instantiations
/// therefore write to the same keys, which is why each keeps its own set.
#[derive(Clone, Debug, Default)]
pub struct InstantiationRecords {
    /// Where the body reads through a reference.
    pub deref_reads: std::collections::HashMap<Span, Type>,
    /// Which arguments the body passes by address.
    pub by_address_arguments: std::collections::HashSet<Span>,
    /// Which call each call expression reached.
    pub call_symbols: std::collections::HashMap<Span, String>,
    /// The call of a closure value each call site makes.
    ///
    /// A record like any other: a template body may call a closure, and each
    /// instantiation checks the call with its own types.
    pub closure_calls: std::collections::HashMap<Span, ClosureCall>,
    /// The value each interface conversion boxes.
    ///
    /// A record like any other: a template body may use an interface, and each
    /// instantiation checks that same expression with its own types, so the
    /// conversion belongs to the instantiation rather than to the program.
    pub interface_conversions: std::collections::HashMap<Span, InterfaceConversion>,
    /// The interface method each virtual call reaches.
    pub interface_calls: std::collections::HashMap<Span, InterfaceCall>,
    /// The lambda each call calls.
    ///
    /// A record like any other: a template body may contain a lambda, and each
    /// instantiation checks that same lambda with its own types, so the answer
    /// belongs to the instantiation rather than to the program.
    pub lambda_calls: std::collections::HashMap<Span, LambdaInstance>,
    /// The lambda each value-position use denotes.
    pub lambda_values: std::collections::HashMap<Span, LambdaInstance>,
    /// Which method each call reached.
    pub method_calls: std::collections::HashMap<Span, String>,
    /// Which `__call__` each call of a value reached.
    ///
    /// A record like the one above: a body that calls a comparator is checked
    /// once per member, and the comparator differs -- `Heap<int, Less<int>>`
    /// and `Heap<int, Greater<int>>` call two different `__call__`s at the same
    /// span. Kept per instantiation for exactly that reason.
    pub call_methods: std::collections::HashMap<Span, String>,
    /// The calls whose callee may raise, so a caller can check for a pending
    /// exception before going on.
    pub throwing_calls: std::collections::HashSet<Span>,
    /// The throwing calls written with `?!`, which send the exception past this
    /// function's own handlers rather than to them.
    pub propagating_calls: std::collections::HashSet<Span>,
    /// Which operator method each operator resolved to.
    pub operator_methods: std::collections::HashMap<Span, OperatorCall>,
    /// Which constructor each `T(args)` call reached.
    ///
    /// A template body may construct a type that depends on the template's
    /// parameters -- `Wrap<T>(value)` -- and the two instantiations reach two
    /// different `__init__`s at the same span. Kept per instantiation for the
    /// same reason as the method and call symbols above.
    pub constructor_calls: std::collections::HashMap<Span, String>,
    /// The type each such call constructs, for the same reason.
    pub constructor_types: std::collections::HashMap<Span, Type>,
    /// Which constructor each `T x = e;` reached.
    pub init_calls: std::collections::HashMap<Span, String>,
    /// Which symbol each index expression resolved to.
    pub index_methods: std::collections::HashMap<Span, String>,
    /// Which `__bool__` each condition resolved to.
    pub bool_methods: std::collections::HashMap<Span, String>,
    /// Which unary operator each expression resolved to.
    pub unary_methods: std::collections::HashMap<Span, String>,
    /// Which compound-assignment method each statement resolved to.
    pub in_place_methods: std::collections::HashMap<Span, String>,
    /// Whether each compound assignment passes its operand by address.
    pub in_place_by_address: std::collections::HashMap<Span, bool>,
    /// Which `__assign__` each assignment resolved to.
    pub assign_methods: std::collections::HashMap<Span, String>,
    /// Which literal became a declared type, and by which constructor.
    pub literal_constructions: std::collections::HashMap<Span, String>,
    /// The type each such literal has.
    pub literal_construction_types: std::collections::HashMap<Span, Type>,
    /// The type each literal argument adopted.
    pub adopted_types: std::collections::HashMap<Span, Type>,
    /// The folded value of each constant expression.
    pub constant_values: std::collections::HashMap<Span, i64>,
    /// Every `if` this instantiation's check decided at compile time.
    pub compile_time_ifs: std::collections::HashMap<Span, bool>,
    /// What each `str(x)`/`repr(x)` resolved to.
    pub text_calls: std::collections::HashMap<Span, Option<String>>,
    /// Which explicit `__init__`/`__drop__` calls were answered without a
    /// method, by the call's span.
    pub in_place_operations: std::collections::HashMap<Span, InPlaceOperation>,
}

/// A concrete value used where an interface is wanted.
///
/// The value is copied onto the heap and a table of the concrete methods is
/// built beside it, so a call through the interface reaches the type's own
/// method.
#[derive(Clone, Debug)]
pub struct InterfaceConversion {
    /// The concrete type the value has.
    pub concrete: Type,
    /// When the source is itself an interface, the table to rebuild from it.
    ///
    /// The source interface's methods are what the table already holds, so the
    /// new table is built by picking those entries rather than by naming the
    /// concrete methods -- which an erased value no longer says.
    pub source: Option<InterfaceSource>,
    /// The interface type the value is used as.
    pub interface_type: Type,
    /// The interface's base name, `IIterator` for `IIterator<int>`.
    pub interface: String,
    /// The symbols of the concrete methods, in the interface's method order.
    pub methods: Vec<String>,
}

/// An interface value used as a different interface.
#[derive(Clone, Debug)]
pub struct InterfaceSource {
    /// The interface the value came from.
    pub base: String,
    /// For each method of the target interface, its entry in the source table.
    pub entries: Vec<usize>,
}

/// A call of a closure value.
#[derive(Clone, Debug)]
pub struct ClosureCall {
    /// The expression that produces the closure, whose fields are the
    /// captured addresses.
    pub value: ExprId,
    /// The type the value has, which names the fields by capture.
    pub ty: Type,
    /// What the value is: the body the call reaches.
    pub instance: LambdaInstance,
    /// The tree the lambda was written in, which is the body's to lower.
    pub tree: Ast,
}

/// One name a lambda captures, and the type it had where the lambda was written.
#[derive(Clone, Debug)]
pub struct Capture {
    /// The name as written.
    pub name: String,
    /// The span of the declaration it names.
    ///
    /// A body is checked once where it is written and again for every call that
    /// decides its parameters -- including calls in another function, where the
    /// captured declaration is no longer in scope. The body's uses resolve to
    /// that declaration by span, so the span travels with the capture and the
    /// check binds it.
    pub name_span: Span,
    /// The type of the captured variable.
    pub ty: Type,
}

/// What a capturing lambda is: the body, its captures, and its value's type.
#[derive(Clone, Debug)]
pub struct ClosureInfo {
    /// The lambda the value came from.
    pub lambda: fallc_ast::LambdaExpr,
    /// The captured names and their types, in field order.
    pub captures: Vec<Capture>,
    /// The name of the type that holds them.
    pub name: String,
    /// The tree the lambda was written in.
    pub tree: Ast,
}

/// A call of an interface method.
#[derive(Clone, Debug)]
pub struct InterfaceCall {
    /// The interface's base name.
    pub interface: String,
    /// Which method, as an index into the interface's method list.
    pub index: usize,
    /// The parameter types, resolved.
    pub parameters: Vec<Type>,
    /// The result type, resolved.
    pub returns: Type,
    /// Whether a call of this method may raise.
    ///
    /// Part of the method's type: a method that may raise returns its result
    /// through the exception ABI, and the call site reads the tag.
    pub throws: bool,
    /// Whether the result is returned through a hidden pointer.
    pub out_of_line: bool,
}

/// One declared specialisation of a type family.
///
/// `template <> type Box<int> { .. }` is a full one: its pattern has no
/// parameters, and it matches exactly. `template <T type> type Box<*T> { .. }`
/// is partial: `T` stands for whatever the pointer points at. When several
/// match a member, the most specific wins.
#[derive(Clone, Debug)]
pub struct Specialisation {
    /// The parameters the pattern may bind.
    pub params: Vec<fallc_ast::TemplateParam>,
    /// The pattern of the family's arguments, as written.
    pub pattern: Vec<fallc_ast::TypeSyntax>,
    /// The declaration that replaces the member's.
    pub declaration: fallc_ast::TypeDecl,
    /// Where the specialisation was written.
    pub span: Span,
}

/// A specialisation of a family, as another module published it.
///
/// The pattern and the fields are resolved types: an importer has no record of
/// the spans the declaring module wrote them at, so it cannot resolve the
/// syntax itself.
#[derive(Clone, Debug)]
pub struct ImportedSpecialisation {
    /// The parameters the pattern binds.
    pub params: Vec<fallc_ast::TemplateParam>,
    /// The argument pattern, with parameters as unnamed types.
    pub pattern: Vec<Type>,
    /// The fields of the specialised member.
    pub fields: Vec<(String, Type)>,
    /// Whether the fields share one address.
    pub is_union: bool,
    /// Which fields may still be written inside a `const` member function.
    pub mutable_fields: Vec<String>,
    /// The alignment each field asked for, in bytes.
    pub alignments: Vec<(String, u64)>,
    /// The specialised member's constants, folded where they were declared.
    ///
    /// A trait family answers through one of these, and the importer has no
    /// tree to fold the initialiser in.
    pub constants: Vec<(String, i64)>,
}

/// One instantiation of a lambda: the code a call or a use of it needs.
///
/// A lambda's parameters may be left untyped, so the same `lambda` expression
/// can be checked more than once -- once per distinct argument type list. Each
/// check produces records of its own, because the body's expressions were
/// checked against these parameter types and no other.
#[derive(Clone, Debug)]
pub struct LambdaInstance {
    /// The symbol the body is emitted under.
    pub symbol: String,
    /// The parameter types this instantiation was checked with.
    pub parameters: Vec<Type>,
    /// The names captured by reference, with their types.
    ///
    /// A captured name is passed to the emitted body as the address of the
    /// variable it names, which is what makes the lambda see the caller's
    /// variable rather than a copy.
    pub captures: Vec<Capture>,
    /// The lambda the instance came from.
    ///
    /// A closure value is called by name, so the call site has no lambda
    /// expression of its own; the body travels with the instance instead.
    pub lambda: fallc_ast::LambdaExpr,
    /// What the lambda returns.
    pub returns: Type,
    /// Whether the body may raise, which makes the lambda a throwing function.
    ///
    /// A throwing lambda's result is the exception result struct, and a call of
    /// it must be handled with `try` or `?!`, exactly as a written `except`
    /// function's is.
    pub throws: bool,
    /// The type table the body was checked against.
    ///
    /// Every instantiation has one of its own, because the body's expressions
    /// are shared between them and their types differ: lowering a body with
    /// another instantiation's table read a `double` as an `int`.
    pub types: TypeTable,
    /// The declared type of each local the body binds.
    ///
    /// Keyed by the declaration's name span, which the lambda shares with every
    /// other instantiation of itself. Kept per instantiation for the same
    /// reason the types are: an untyped local in the body has a `T` that each
    /// instantiation replaces.
    pub bindings: BindingTypes,
    /// What checking the body recorded, keyed by span.
    pub records: InstantiationRecords,
}

/// The declarations a module's own templates were written against.
///
/// A template body is checked where it is *instantiated*, which may be another
/// file. The names in it belong to the module that wrote it: `Box<T>` in `std`
/// is `std`'s family even when the instantiating file declares a `Box` of its
/// own, and a private type the body uses is not visible anywhere else. So the
/// module's declarations travel with the body and are borrowed where it is
/// checked.
#[derive(Clone, Debug, Default)]
pub struct TypeEnvironment {
    /// The module the declarations belong to, for diagnostics.
    pub module: String,
    /// The nominal types it declared or imported.
    pub nominal_types: std::collections::HashMap<String, crate::types::NominalType>,
    /// The type families it declared.
    pub type_templates: std::collections::HashMap<String, fallc_ast::TypeDecl>,
    /// The aliases it declared.
    pub aliases: std::collections::HashMap<String, crate::types::Type>,
    /// The base type of each enum it declared.
    pub enum_bases: std::collections::HashMap<String, crate::types::Type>,
    /// The specialisations of its families.
    pub type_specialisations: std::collections::HashMap<String, Vec<Specialisation>>,
    /// The interfaces it declared.
    pub interfaces: std::collections::HashMap<String, fallc_ast::InterfaceDecl>,
    /// The modules it could name: itself and the ones it imports.
    ///
    /// A body instantiated elsewhere writes `util.__ft_moves<T>()`, and `util`
    /// is a module only in the module that imported it. The names travel with
    /// the body for the same reason its declarations do.
    pub modules: std::collections::HashSet<String>,
}

/// The constant-folding index of one tree, set aside while another is checked.
///
/// An expression id belongs to one tree, so checking a template body from
/// another module means indexing *that* tree's expressions for as long as the
/// body is being checked -- [`TypeChecker::adopt_const_index`] does it and hands
/// back the previous index, which [`TypeChecker::restore_const_index`] puts back.
pub(super) struct ConstIndex {
    /// The constant expressions, by expression id.
    exprs: Vec<ConstExpr>,
    /// The span of each expression, for diagnostics.
    spans: Vec<fallc_span::Span>,
    /// The written suffix of each literal.
    suffixes: Vec<Option<String>>,
    /// The tree the index was built from.
    tree: Option<usize>,
}

/// Everything needed to check and emit one template's body.
///
/// The parts travel together because they all come from the module that
/// *declared* the template, while the check happens in whichever file
/// instantiates it. A template's body names what its own module declared --
/// `panic.panic`, `INITIAL_CAPACITY`, its own local types -- and the file that
/// asked for `Array<int>` has usually never heard of any of them.
#[derive(Clone, Debug)]
pub struct TemplateBody {
    /// The tree the body is in.
    ///
    /// Shared rather than copied: a module declares hundreds of templated
    /// functions, and they are all written in the one tree. Cloning it per
    /// template made the memory a compilation needs grow with the product of
    /// the two.
    pub tree: std::rc::Rc<Ast>,
    /// The declarations that tree's names resolve to.
    ///
    /// Shared for the same reason as the tree: one module's resolutions answer
    /// for every template it declares.
    pub resolutions: std::rc::Rc<crate::Resolutions>,
    /// Every constant of the module that declared the template, by name,
    /// already folded to its value.
    ///
    /// Folded there, because a constant is identified by an expression *id* and
    /// an id belongs to one tree. The file that instantiates the template has
    /// an index of its own, and reading the module's id against it found
    /// nothing.
    pub constants: std::collections::HashMap<String, i64>,
    /// The declared type of each of those constants, by name.
    ///
    /// Carried for the same reason the values are, and needed because the type
    /// decides the *width* of the store: without it `INITIAL_CAPACITY`, a
    /// `usize`, was stored as an `int` -- four bytes of an eight-byte slot,
    /// leaving the upper half to whatever the previous frame had left there.
    pub constant_types: std::collections::HashMap<String, crate::types::Type>,
    /// The function itself.
    pub function: fallc_ast::FunctionItem,
    /// The declaration's attributes.
    ///
    /// `@enable_if` on a method of a family is checked per member, so the
    /// attribute has to travel with the body.
    pub attrs: Vec<fallc_ast::Attr>,
    /// The declarations the body was written against.
    ///
    /// Absent for a template declared in the file being compiled: that file's
    /// declarations are the checker's own tables.
    pub environment: Option<std::rc::Rc<TypeEnvironment>>,
    /// The declared type of each of the declaring module's module-level
    /// variables, by the span of its name in that module's tree.
    ///
    /// A body instantiated in another module still names them by their bare
    /// names -- `out.write(..)` -- and a variable's type lives in the binding
    /// table of the module that declared it. Without the table the name went
    /// untyped, so the member call it received was never resolved and code
    /// generation had no record for it: the short `print` above `std.out` could
    /// not be emitted.
    pub bindings: crate::types::BindingTypes,
}

/// A trait, with the tree its requirements are written in.
///
/// The tree travels for the same reason a template's does: a requirement is an
/// *expression*, and an expression id belongs to one file. Asking whether a type
/// declared here models a trait declared in the standard library means checking
/// requirements written in a tree this checker never saw, so the tree comes with
/// the declaration.
#[derive(Clone, Debug)]
pub struct TraitBody {
    /// The tree the requirements are written in.
    ///
    /// Shared, because every trait of one module is written in the same tree
    /// and a file may declare several.
    pub tree: std::rc::Rc<Ast>,
    /// The declarations that tree's names resolve to.
    pub resolutions: std::rc::Rc<crate::Resolutions>,
    /// The declaration itself.
    pub declaration: fallc_ast::TraitDecl,
}

/// What asking a trait about a type produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraitOutcome {
    /// Every requirement holds.
    Modelled,
    /// The trait is not declared anywhere this file can see.
    Unknown,
    /// The number of type arguments is wrong.
    Arity {
        /// The trait that was asked about.
        name: String,
        /// How many type variables it declares.
        expected: usize,
        /// How many arguments were given.
        found: usize,
    },
    /// A requirement does not hold.
    NotModelled {
        /// The requirement's source text.
        requirement: String,
        /// Why it failed, when the failure has a message of its own.
        why: Option<String>,
    },
}

/// A special method found on a type, with what its call needs.
#[derive(Clone, Debug)]
struct SpecialMethodCall {
    /// The declaration.
    signature: FunctionSignature,
    /// Every parameter, resolved under the member's own arguments.
    param_types: Vec<Type>,
    /// What the method returns.
    returns: Type,
    /// The symbol the definition is emitted under.
    symbol: String,
}

/// An operator that resolved to a method on one of its operands.
#[derive(Clone, Debug)]
pub struct OperatorCall {
    /// The symbol the method was emitted under.
    pub symbol: String,
    /// Whether the *right* operand is passed as an address.
    ///
    /// True when the parameter is a pointer and the operand is a value: the
    /// operand fits the parameter by having its address taken, so code
    /// generation has to take it.
    pub argument_by_address: bool,
    /// The symbol of the *reflected* form, if that is the one that applied.
    pub reflected: bool,
}

/// One default a call leaves to an expression.
///
/// The parameter's index, the expression itself, the module that wrote it, and
/// the parameters before it with their types. The expression names the declaring
/// module's tree, and the parameters are bound to the call's arguments while it
/// is lowered.
pub type ExpressionDefault = (usize, ExprId, String, Vec<(String, crate::types::Type)>);

/// Every such default, by the span of the call that left it.
pub type ExpressionDefaults = std::collections::HashMap<Span, Vec<ExpressionDefault>>;

/// The output of running the type checker.
#[derive(Clone, Debug)]
pub struct TypeCheckResult {
    /// The calls whose callee may raise, so that code generation can check for
    /// a pending exception after each one.
    pub throwing_calls: std::collections::HashSet<Span>,
    /// The throwing calls written with `?!`.
    pub propagating_calls: std::collections::HashSet<Span>,
    /// What each `type Name = Other;` alias stands for, by name.
    ///
    /// Published because code generation resolves a few written types on its
    /// own, and `Name` in `fn f(n Name)` has to mean what the alias says.
    pub aliases: std::collections::HashMap<String, crate::types::Type>,
    /// The type computed for each expression that has one.
    pub types: TypeTable,
    /// The type each `type(expr)` names, by the span of the written form.
    ///
    /// Published because code generation resolves a few written types itself,
    /// and `type(a)` in `fn f(a int, b type(a))` has to mean what the question
    /// answered, not the syntax it was asked in.
    pub type_of_types: std::collections::HashMap<Span, crate::types::Type>,
    /// Every constant of this module, by name, folded to its value.
    ///
    /// A template's body names its own module's constants, and the file that
    /// instantiates the template is a different file: `INITIAL_CAPACITY`
    /// belongs to `std.array`, and a program that writes `Array<int>` has never
    /// heard of it. Keyed by name rather than by expression id, because an id
    /// belongs to one tree.
    pub constants_by_name: std::collections::HashMap<String, i64>,
    /// The value of each parameter default the compiler could work out, by the
    /// default expression's span.
    ///
    /// Folded in the module that declared it, because that is where the tree
    /// the expression belongs to is in hand. An exported signature carries the
    /// values so that a caller in another module can leave such a parameter
    /// out -- `getline(line)` for `getline(line, until = '\n')`.
    pub default_values: std::collections::HashMap<Span, i64>,
    /// The constant arguments a call fills its omitted parameters with, by the
    /// call's span: the parameter index, the value, and the type to store it
    /// as.
    ///
    /// The type travels because the call may reach an instantiation, whose
    /// parameters are not in this unit's tables; a `char` default must be
    /// stored as a `char`, not as an `int`.
    pub default_arguments: std::collections::HashMap<Span, Vec<(usize, i64, crate::types::Type)>>,
    /// The defaults a call leaves to an *expression* rather than a value, by
    /// the call's span: the parameter's index, the expression, the module that
    /// wrote it, and the parameters before it with their types.
    ///
    /// The expression names the declaring module's tree, so it is lowered with
    /// that module's tree and tables at the call site -- which is what makes a
    /// default like `b int = a + 1` work from another module.
    pub default_expressions: ExpressionDefaults,
    /// Every `if` whose condition was known at compile time, by the condition's
    /// span, holding whether the *then* branch is the live one.
    ///
    /// Code generation emits only the live branch. The other was never checked,
    /// so its expressions have no types and lowering it would find none.
    pub compile_time_ifs: std::collections::HashMap<Span, bool>,
    /// The declared type of each of those constants, by name.
    ///
    /// The per-expression type table does not survive a template
    /// instantiation: the body is checked again against a table of its own, and
    /// the entry for a constant's *use* is not carried across. Code generation
    /// then had no type for `INITIAL_CAPACITY` and fell back to `int`, so a
    /// `usize` binding was initialised by a 4-byte store into an 8-byte slot --
    /// leaving the upper half to whatever the previous frame left there.
    pub constant_types_by_name: std::collections::HashMap<String, crate::types::Type>,
    /// The declared types, so a later stage can resolve a written type name.
    pub types_declarations: Vec<crate::types::NominalType>,
    /// Everything this module declared, for the template bodies it owns.
    ///
    /// A body is checked where it is instantiated, so the declarations it names
    /// have to travel with it -- including the ones this module kept private.
    pub environment: TypeEnvironment,
    /// The declared type of each local binding.
    pub binding_types: BindingTypes,
    /// The alignment each binding asked for with `alignas`, by the binding's
    /// name span.
    ///
    /// A type's layout carries its fields' alignments; a variable's alignment
    /// is a property of its own storage, so it travels beside the binding. The
    /// value is the one the checker validated -- a power of two, no weaker than
    /// the type's own -- which is what code generation sets on the storage.
    pub binding_alignments: std::collections::HashMap<Span, u64>,
    /// The conversion each cast resolved to, by the cast's span.
    ///
    /// Recorded rather than looked up again in code generation, because the
    /// selector's meaning -- which target type a `__as__` is for -- is decided
    /// by the type system, and code generation has no business repeating that
    /// decision.
    pub conversions: std::collections::HashMap<Span, ConversionMethod>,
    /// The symbol each index expression resolved to, by the expression's span.
    ///
    /// Empty for an array or pointer, which is indexed directly; present when
    /// the object's type defined `__getitem__`.
    pub index_methods: std::collections::HashMap<Span, String>,
    /// The symbol each `__call__` resolved to, by the call's span.
    pub call_methods: std::collections::HashMap<Span, String>,
    /// The symbol each `__bool__` resolved to, by the condition's span.
    ///
    /// A condition on a type that defines `__bool__` is that call rather than
    /// a comparison against zero.
    pub bool_methods: std::collections::HashMap<Span, String>,
    /// The symbol each unary operator resolved to, by the expression's span.
    pub unary_methods: std::collections::HashMap<Span, String>,
    /// The symbol each compound assignment resolved to, by the statement's span.
    pub in_place_methods: std::collections::HashMap<Span, String>,
    /// Whether each compound assignment hands its operand over by address, by
    /// the statement's span.
    pub in_place_by_address: std::collections::HashMap<Span, bool>,
    /// The constructor each `T x = expr;` resolved to, by the initialiser's span.
    pub init_calls: std::collections::HashMap<Span, String>,
    /// The symbol each overloaded operator resolved to, by the expression's
    /// span, together with whether its argument is taken by address.
    ///
    /// `a + b` on a type that defines `+` is a call of `__add__`, and which
    /// overload that is -- and whether the other operand is passed as a value
    /// or as the address of one -- is decided by the same parameter matching
    /// every other call uses. Recording it here keeps code generation from
    /// searching for a symbol by shape.
    pub operator_methods: std::collections::HashMap<Span, OperatorCall>,
    /// Where a string literal is used as a value of a declared type, the
    /// constructor that turns it into one, by the literal's span.
    ///
    /// `"text"` is an array of characters, not a `String`. Where a `String` is
    /// wanted -- `s + "text"`, `t = "text"` in a call -- the literal is
    /// constructed into one, which is a call the checker is the only stage able
    /// to choose. Code generation emits it and owns the result.
    pub literal_constructions: std::collections::HashMap<Span, String>,
    /// The type each such literal is constructed into, by the literal's span.
    ///
    /// The symbol names the constructor and the constructor's receiver names the
    /// type, but recovering a type from a mangled symbol is guesswork. The type
    /// is recorded directly instead.
    pub literal_construction_types: std::collections::HashMap<Span, Type>,
    /// The type each argument expression was given by the parameter it fitted,
    /// by the argument's span, where that differs from the expression's own.
    ///
    /// An unsuffixed integer literal has no type until something gives it one.
    /// Passing it to `f(u8)` makes it a `u8`; the expression's recorded type
    /// stays the default `int`, because that is what the literal is on its own.
    /// Code generation lowers what the table says, so without this it passed an
    /// `i32` to a parameter declared `i8`, and LLVM rejected the call.
    pub adopted_types: std::collections::HashMap<Span, Type>,
    /// The arguments that must be passed as their address, by the argument's
    /// span.
    ///
    /// A reference parameter takes the address of what it is given, the way a
    /// method takes its receiver's address. Which arguments those are is
    /// decided by the parameter matching, which only the checker is doing.
    pub by_address_arguments: std::collections::HashSet<Span>,
    /// The value of every constant expression the checker folded, by the
    /// expression's span.
    ///
    /// `sizeof(T)` and `alignof(T)` have their value computed while the types
    /// are known, which is the checker's business; carrying the number across
    /// keeps code generation from recomputing a layout -- and from having to
    /// know the layout rules at all.
    pub constant_values: std::collections::HashMap<Span, i64>,
    /// Every instantiation a call produced, by the key its arguments make.
    ///
    /// An instantiation is a function the program does not contain: it exists
    /// because a call asked for it, and code generation emits it from the
    /// template's own body. The key is the symbol it was emitted under, and the
    /// *node* is the template it came from -- which is what code generation
    /// needs to find the body.
    pub instantiations: std::collections::HashMap<String, Instantiation>,
    /// The constructor each `T(args)` call resolved to, by the call's span.
    ///
    /// Distinct from `init_calls`, which is `T x = ..;`: that one writes into
    /// the binding's storage, and this one has no storage until code generation
    /// makes some.
    pub constructor_calls: std::collections::HashMap<Span, String>,
    /// The type each such call constructs, by the call's span.
    ///
    /// The callee names a type, which is not a value and so has no entry in the
    /// type table. What is being built is recorded here instead, because only
    /// the checker knows which member of a family the call meant.
    pub constructor_types: std::collections::HashMap<Span, Type>,
    /// The symbol each plain function call resolved to, by the call's span.
    ///
    /// Code generation used to rebuild the symbol from the argument types,
    /// which is the overload rule written a second time -- and it then fell
    /// back to matching symbol *prefixes*, which picked whichever overload
    /// happened to be emitted first. A call through a string literal chose the
    /// `c8` overload of `f` because `f_C2_c8` sorts before the pointer one.
    /// Only the checker knows which candidate won, so it records it.
    pub call_symbols: std::collections::HashMap<Span, String>,
    /// The name spans of the methods written against a type family's parameter.
    ///
    /// Such a function is not a function: `fn twice(this &Crate<T>)` names a
    /// member of a family, and each member is emitted from its own
    /// instantiation. Published so that code generation does not also emit the
    /// declaration as it stands, over a `T` that does not exist.
    pub templated_methods: std::collections::HashSet<Span>,
    /// The declarations `@enable_if` removed, by name span.
    ///
    /// Code generation must not emit one: its body was never checked, because
    /// it does not exist as far as this program is concerned.
    pub disabled_functions: std::collections::HashSet<Span>,
    /// The value each interface conversion boxes, by the value's span.
    pub interface_conversions: std::collections::HashMap<Span, InterfaceConversion>,
    /// The call of a closure value each call site makes, by the call's span.
    ///
    /// The value says which lambda it came from; a call of it reaches the
    /// body through that record, and takes the captures from the value.
    pub closure_calls: std::collections::HashMap<Span, ClosureCall>,
    /// The interface method each virtual call reaches, by the call's span.
    pub interface_calls: std::collections::HashMap<Span, InterfaceCall>,
    /// The lambda each call calls, by the call's span.
    pub lambda_calls: std::collections::HashMap<Span, LambdaInstance>,
    /// The lambda each value-position use denotes, by the lambda's span.
    ///
    /// A lambda that captures nothing is a function, so it may be stored in a
    /// function pointer. The symbol is the body the pointer points at.
    pub lambda_values: std::collections::HashMap<Span, LambdaInstance>,
    /// What each `str(x)` or `repr(x)` resolved to, by the call's span.
    ///
    /// `None` means the compiler renders the value itself; `Some(symbol)`
    /// means the argument's type declared `__str__`/`__repr__` and that
    /// function is called instead.
    pub text_calls: std::collections::HashMap<Span, Option<String>>,
    /// The symbol each method call resolved to, by the call's span.
    ///
    /// Recorded because only the checker knows which definition a call
    /// selected. Code generation used to rebuild the symbol from the receiver
    /// and argument types, which is the same rule written twice, and it then
    /// fell back to searching every known symbol for a match -- guesswork
    /// rather than resolution.
    pub method_calls: std::collections::HashMap<Span, String>,
    /// The symbol each plain assignment resolved to, by the statement's span.
    ///
    /// Empty when the type defines no `__assign__`, in which case the
    /// assignment is the implicit member-wise copy.
    pub assign_methods: std::collections::HashMap<Span, String>,
    /// Where a `__deref__` result is read as a value, the referent to read.
    pub deref_reads: std::collections::HashMap<Span, Type>,
    /// The type each `->` chain ends at, by the field access's span.
    pub pointer_targets: std::collections::HashMap<Span, Type>,
    /// Which explicit `__init__`/`__drop__` calls were answered by a plain
    /// store or a field-wise destroy rather than a declared method, by the
    /// call's span.
    ///
    /// The decision depends on the instantiated element type, so it is made
    /// while the body is checked and travels with the rest of what that check
    /// resolved.
    pub in_place_operations: std::collections::HashMap<Span, InPlaceOperation>,
    /// The qualified name of each imported variable the program used, by the
    /// span of that use. Code generation declares the symbol from it.
    pub external_global_refs: std::collections::HashMap<Span, String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl TypeCheckResult {
    /// Returns `true` if no fatal diagnostic was reported.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_fatal)
    }
}

/// A templated method whose body is waiting to be checked.
///
/// A method's body is checked once its member is known, which is after the call
/// that asked for it -- so the request is parked and taken up when the item is
/// reached. The fields are what the check needs and what the request learned:
/// the body with its module's tables, the symbol and arguments it was
/// instantiated with, and the element types when the method takes a parameter
/// pack (`Some`, one per element, empty for no element).
#[derive(Debug)]
struct PendingMethodInstantiation {
    /// The method's body, with the tables of the module that declares it.
    body: TemplateBody,
    /// The mangled symbol the instantiation was recorded under.
    symbol: String,
    /// What the template parameters stand for.
    substitution: crate::template::Substitution,
    /// The template parameters, as written.
    arguments: crate::template::Arguments,
    /// The parameters themselves, for checking the body's mentions of them.
    params: Vec<fallc_ast::TemplateParam>,
    /// The element types of a parameter pack, if the method has one.
    pack_elements: Option<Vec<crate::types::Type>>,
}

/// Checks types in an AST.
#[derive(Debug)]
pub struct TypeChecker {
    diagnostics: DiagnosticBag,
    /// Whether function bodies are checked at all.
    ///
    /// A module imported by a program contributes its *declarations*: types,
    /// signatures, constants, defaults, traits, macros. Its implementation --
    /// the statements of its functions and its `__setup__` -- is compiled once,
    /// when the module's own object is built, and a program that reuses that
    /// object has no reason to check it again. This is the switch that makes
    /// the declaration-only pass possible; a module whose object is being built
    /// is checked fully.
    declarations_only: bool,
    /// The type the expression currently being checked is expected to have,
    /// if any.
    ///
    /// Literals consult this so that they take their context's type. It is set
    /// by [`TypeChecker::check_expr`] around the expression it checks.
    expected: Option<Type>,
    /// The type of each local binding, filled in as bindings are checked.
    binding_types: BindingTypes,
    /// The alignment each binding asked for with `alignas`, by name span.
    binding_alignments: std::collections::HashMap<Span, u64>,
    /// The functions marked `@deprecated`, by the name they are declared with.
    deprecated_functions: std::collections::HashMap<String, Option<String>>,
    /// The types of `extern` variables other modules exported, by the qualified
    /// name a program writes: `C.stdin`.
    imported_variables: std::collections::HashMap<String, Type>,
    /// The nominal type names this module did not declare, but imported.
    ///
    /// Kept so that a diagnostic about such a type can say what is different
    /// about it: its methods belong to another module, and a method that
    /// satisfies an interface has to be visible where the conversion is
    /// written.
    imported_type_names: std::collections::HashSet<String>,
    /// The measured alignment of each field of an imported family.
    ///
    /// A member of the family is built where it is first named, and its
    /// declaration arrived without `alignas`: the alignment is an expression in
    /// the declaring module's tree, so what travels is the number, keyed by the
    /// family's name and read when the member is built.
    imported_field_alignments:
        std::collections::HashMap<String, Vec<(String, u64)>>,
    /// The declaration tables of the modules whose template bodies are being
    /// checked, innermost last.
    type_environments: Vec<std::rc::Rc<TypeEnvironment>>,
    /// The names two declaration tables disagreed about, so each is reported
    /// once however many instantiations meet it.
    environment_conflicts: std::collections::HashSet<(String, String)>,
    /// The names this file itself declares as a type.
    ///
    /// A file's own declaration is what its names mean, so an imported family
    /// of the same name steps aside -- until now a `type Box` in a program that
    /// imported `std` could not be named at all.
    local_type_names: std::collections::HashSet<String>,
    /// The names this file itself declares as a type *family*.
    local_families: std::collections::HashSet<String>,
    /// The qualified name of each imported variable a program uses, by the
    /// span of that use. Code generation reads it to declare the symbol.
    external_global_refs: std::collections::HashMap<Span, String>,
    /// The names of the `extern` variables this unit declares.
    ///
    /// Their storage belongs to another library, so code generation emits a
    /// declaration for the symbol rather than a definition. Kept by name
    /// because a declaration has no initialiser and no body to identify it by.
    external_variables: std::collections::HashSet<String>,
    /// The `auto` return type each function being checked has, by index.
    ///
    /// `Some(span)` means the function wrote `auto` as its return type and the
    /// type is not known yet; the first `return` decides it. It is a stack
    /// because a function body may contain a nested one -- a template
    /// instantiated while checking -- and each has its own return type.
    /// Functions whose return type is still to be deduced: the span to file the
    /// answer under, and whether the expression's reference is kept.
    ///
    /// `auto` is the *value* the return expression names, so a reference reads
    /// through; `type(auto)` is the expression's own type, reference included.
    pending_auto_returns: Vec<Option<(Span, bool)>>,
    /// The substitutions to put back when a trait body is done being checked.
    ///
    /// One per trait being checked, because a requirement may name another
    /// trait: `InputIterator(it) -> true` asks about a type, and that question
    /// checks the other trait's requirements -- which bind parameters of their
    /// own.
    binding_saved_substitution: Vec<Option<crate::template::Substitution>>,
    /// Whether a trait requirement is being checked.
    ///
    /// A requirement is an expression *about* a type -- `it != it` says two
    /// positions can be compared -- rather than code that runs, so the
    /// warnings that are about expressions nobody would write on purpose do not
    /// apply inside one.
    in_trait_requirement: bool,
    /// The `__as__` method each cast resolved to, by the cast's span.
    conversion_methods: std::collections::HashMap<Span, ConversionMethod>,
    implicit_conversions: std::collections::HashMap<Span, ConversionMethod>,
    /// The type each `type(expr)` names, by the span of the written form.
    ///
    /// `type(expr)` is resolved where the expression is in hand -- a binding's
    /// declaration, a cast's target, `sizeof` -- and recorded here, because a
    /// written type is resolved in places that have no expression tree to ask.
    type_of_types: std::collections::HashMap<Span, Type>,
    /// The symbol each index expression resolved to, by the expression's span.
    ///
    /// Empty for an array or pointer, which is indexed directly; present when
    /// the object's type defined `__getitem__`.
    index_methods: std::collections::HashMap<Span, String>,
    /// The symbol each `__call__` resolved to, by the call's span.
    call_methods: std::collections::HashMap<Span, String>,
    /// The symbol each `__bool__` resolved to, by the condition's span.
    bool_methods: std::collections::HashMap<Span, String>,
    /// The symbol each unary operator resolved to, by the expression's span.
    unary_methods: std::collections::HashMap<Span, String>,
    /// The symbol each compound assignment resolved to, by the statement's span.
    in_place_methods: std::collections::HashMap<Span, String>,
    /// Whether each compound assignment's operand is passed by address; see
    /// `CheckedProgram::in_place_by_address`.
    in_place_by_address: std::collections::HashMap<Span, bool>,
    /// The constructor each `T x = expr;` resolved to, by the initialiser's span.
    init_calls: std::collections::HashMap<Span, String>,
    /// What each `str(x)` or `repr(x)` resolved to, by the call's span; see
    /// `CheckedProgram::text_calls`.
    text_calls: std::collections::HashMap<Span, Option<String>>,
    /// The symbol each plain function call resolved to; see
    /// `CheckedProgram::call_symbols`.
    call_symbols: std::collections::HashMap<Span, String>,
    /// The names of functions declared `@must_use`.
    ///
    /// Recorded by written name, which is what a call site has before the
    /// overload is chosen; the call checker then notes calls whose result must
    /// be used, by the call's span.
    must_use_functions: std::collections::HashSet<String>,
    /// Calls whose result must be used, by the call's span.
    must_use_calls: std::collections::HashSet<Span>,
    /// Whether each lambda body being checked has raised, and the exception
    /// depth at which it began.
    ///
    /// One frame per lambda being checked: a raise marks the innermost body it
    /// belongs to, since a nested lambda's raise escapes at its own call. The
    /// depth is the baseline, so a `try` *inside* the body catches a raise
    /// while one around the whole lambda does not.
    lambda_raise_stack: Vec<(usize, bool)>,
    /// The methods written against a type family's parameter, by name span.
    templated_methods: std::collections::HashSet<Span>,
    /// The interfaces this module declares, by name.
    interfaces: std::collections::HashMap<String, fallc_ast::InterfaceDecl>,
    /// The expected type recorded for each expression, by expression id.
    ///
    /// An interface is satisfied *structurally*, which needs the resolution
    /// table to find the concrete methods -- and the place a value is checked
    /// does not have it. The expectations are therefore recorded while checking
    /// and resolved in one pass afterwards.
    expected_types: std::collections::HashMap<ExprId, (Span, Type)>,
    /// The closures this module has built, by their type name.
    ///
    /// A capturing lambda is a value of a type that holds the addresses of what
    /// it captured. The type is anonymous -- nothing can write it -- but a call
    /// of the value has to reach the lambda's body, so the body travels here,
    /// keyed by the type's name.
    closures: std::collections::HashMap<String, ClosureInfo>,
    /// The specialisations of each type family, by the family's name.
    type_specialisations: std::collections::HashMap<String, Vec<Specialisation>>,
    /// The specialisations that came from another module, already resolved.
    imported_specialisations: std::collections::HashMap<String, Vec<ImportedSpecialisation>>,
    /// The constants an imported family's own body declares, already folded.
    ///
    /// The generic member of an imported family has no expression tree here, so
    /// its answers cannot be evaluated on demand; they arrive folded, keyed by
    /// the family name they were imported under.
    imported_family_constants:
        std::collections::HashMap<String, std::collections::HashMap<String, i64>>,
    /// The declarations `@enable_if` removed from consideration, by name span.
    ///
    /// A disabled declaration is not an error by itself: it is a declaration
    /// that does not exist for this program, so a call finds whatever else is
    /// there -- or says that nothing matches.
    disabled_functions: std::collections::HashSet<Span>,
    /// The `__setup__` block this module has, if it has one.
    setup_span: Option<Span>,
    /// The value each interface conversion boxes, by the value's span.
    interface_conversions: std::collections::HashMap<Span, InterfaceConversion>,
    /// The call of a closure value each call site makes, by the call's span.
    closure_calls: std::collections::HashMap<Span, ClosureCall>,
    /// The interface method each virtual call reaches, by the call's span.
    interface_calls: std::collections::HashMap<Span, InterfaceCall>,
    /// The lambda each call calls, by the call's span.
    lambda_calls: std::collections::HashMap<Span, LambdaInstance>,
    /// The lambda each value-position use denotes, by the lambda's span.
    lambda_values: std::collections::HashMap<Span, LambdaInstance>,
    /// Every lambda instantiation already checked, by its key.
    ///
    /// One body may be called with one type list from several places; checking
    /// it once and sharing the records keeps the two calls one function.
    lambda_instances: std::collections::HashMap<String, LambdaInstance>,
    /// The constructor each `T(args)` call resolved to; see
    /// `CheckedProgram::constructor_calls`.
    constructor_calls: std::collections::HashMap<Span, String>,
    /// The type each construction call builds; see
    /// `CheckedProgram::constructor_types`.
    constructor_types: std::collections::HashMap<Span, Type>,
    /// Every instantiation a call produced; see
    /// `CheckedProgram::instantiations`.
    instantiation_list: Vec<Instantiation>,
    /// The folded value of each constant expression; see
    /// `CheckedProgram::constant_values`.
    constant_values: std::collections::HashMap<Span, i64>,
    /// The arguments passed by address; see
    /// `CheckedProgram::by_address_arguments`.
    by_address_arguments: std::collections::HashSet<Span>,
    /// The operator methods resolved, by the expression's span; see
    /// `CheckedProgram::operator_methods`.
    operator_methods: std::collections::HashMap<Span, OperatorCall>,
    /// The type an argument expression adopted from its parameter; see
    /// `CheckedProgram::adopted_types`.
    adopted_types: std::collections::HashMap<Span, Type>,
    /// The constructor a string literal used as a declared type resolved to;
    /// see `CheckedProgram::literal_constructions`.
    literal_constructions: std::collections::HashMap<Span, String>,
    /// The type each constructed literal has; see
    /// `CheckedProgram::literal_construction_types`.
    literal_construction_types: std::collections::HashMap<Span, Type>,
    /// The symbol each method call resolved to, by the call's span.
    method_calls: std::collections::HashMap<Span, String>,
    /// The calls whose callee may raise.
    throwing_calls: std::collections::HashSet<Span>,
    /// Those of them written with `?!`, which leave the function rather than
    /// going to a handler in it.
    propagating_calls: std::collections::HashSet<Span>,
    /// Whether raising is allowed here: one entry per function whose body is
    /// being checked, true when it is marked `except`.
    throws_allowed: Vec<bool>,
    /// How many `try` bodies with an `except` enclose the statement being
    /// checked. A call that may raise needs one of these, or a `?!`.
    protected_depth: usize,
    /// How many `except` blocks enclose it, which is where `raise;` is legal.
    except_depth: usize,
    /// Non-zero while the value of a `?!` statement is being checked: that call
    /// is propagating, so it is not the unprotected one.
    propagating_depth: usize,
    /// Non-zero while a `?!(expr)` query is being analysed: the expression is
    /// never emitted, so what it *would* do is all that matters.
    probe_depth: usize,
    /// Whether the probe just analysed may raise.
    probe_saw_throw: bool,
    /// The symbol each plain assignment resolved to, by the statement's span.
    assign_methods: std::collections::HashMap<Span, String>,
    /// Where a `__deref__` result is read as a value, the referent to read.
    deref_reads: std::collections::HashMap<Span, Type>,
    /// The type each `->` chain ends at, by the field access's span.
    pointer_targets: std::collections::HashMap<Span, Type>,
    /// Which explicit `__init__`/`__drop__` calls were answered by a plain
    /// store or a field-wise destroy; see
    /// [`CheckedProgram::in_place_operations`].
    in_place_operations: std::collections::HashMap<Span, InPlaceOperation>,
    /// The resolutions of the file being checked, consulted when the table a
    /// template body was declared with has nothing to say about a name.
    ///
    /// A generic body is type-checked against the definitions of the module
    /// that declares it, but its substituted types are the caller's: the
    /// `T` in `Array`'s `push` becomes `String`, and `String`'s `__init__`
    /// belongs to another module. The file that instantiated the template has
    /// those definitions in its own table, so it is the fallback.
    fallback_resolutions: Option<crate::Resolutions>,
    /// The names of imported modules.
    ///
    /// `C.printf(..)` and `p.show()` are syntactically identical, so resolution
    /// needs to know which names are modules rather than values.
    modules: std::collections::HashSet<String>,
    /// The declared nominal types, by name.
    ///
    /// A `Type::Nominal` holds its fields, so this table is only needed while
    /// building one: a type is resolved by name before its `Type` value exists.
    nominal_types: std::collections::HashMap<String, crate::types::NominalType>,
    /// Templated methods whose instantiation is waiting to be checked.
    ///
    /// A method's body is checked once its member is known, which is after the
    /// call that asked for it -- so it cannot be checked *during* that call.
    /// The request is parked here and taken up when the item is reached.
    pending_method_instantiations: Vec<PendingMethodInstantiation>,

    /// Every imported module's tree, so that a template declared in one can be
    /// checked while another is compiled.
    ///
    /// A family lives in a library and is instantiated by its users, so the
    /// body to check is usually not in the tree being checked: `Array<int>` is
    /// asked for by a program, and `Array`'s methods are written in
    /// `std.array`.
    module_trees: Vec<Ast>,
    /// Every templated function's declaration node, by the name it is bound
    /// under, handed over by the driver.
    ///
    /// A method of a family declared in another module is reached from here:
    /// the checker that resolves `Array<int>` never saw that module's tree, so
    /// it cannot have recorded the node itself.
    template_definitions: std::collections::HashMap<String, TemplateBody>,
    /// Every trait this file can ask about, by the name it may write.
    ///
    /// Filed under the names a program can spell -- the bare one and the
    /// module-qualified ones -- because that is how the language makes a name
    /// reachable, and a query is written with whichever of them is in scope.
    trait_definitions: std::collections::HashMap<String, TraitBody>,
    /// What each instantiated type was made from, by the member's name.
    ///
    /// A method of a type family is written against the declaration's
    /// parameters, so a call on `Box<int>` has to substitute `int` for `T`
    /// before the method's own parameters mean anything. This is where that
    /// link is kept.
    type_template_arguments:
        std::collections::HashMap<String, (String, crate::template::Arguments)>,
    /// The templated types, by name, so that `Array<int>` can be built.
    ///
    /// Held as declarations rather than as types: a template declares no type,
    /// and every member of the family is made on demand from this.
    type_templates: std::collections::HashMap<String, fallc_ast::TypeDecl>,
    /// Every constant of this module, by name, folded to its value.
    ///
    /// Distinct from `constants`, which is keyed by expression id: an id
    /// belongs to one tree, so a template checked from another file cannot use
    /// it. This is what travels with a template.
    constants_by_name: std::collections::HashMap<String, i64>,
    /// Every compile-time-decided `if`. See [`CheckedProgram::compile_time_ifs`].
    compile_time_ifs: std::collections::HashMap<Span, bool>,
    /// What each `type Name = Other;` alias stands for, by name.
    ///
    /// An alias is not a type of its own: resolving one yields the type it
    /// names, so `Id` and `usize` are one type under two spellings and a value
    /// of either is accepted where the other is wanted.
    aliases: std::collections::HashMap<String, Type>,
    /// The target's pointer width, in bits, or zero when it is unknown.
    ///
    /// A pointer's layout is the machine's, so `sizeof` of anything containing
    /// one is too. The driver supplies this from the target machine the object
    /// file is written with, which is the only part that knows the target.
    pointer_bits: u32,
    /// The underlying type of each enum, by name.
    ///
    /// An enumerator's *type* is this, which is what makes `Color.RED`
    /// comparable with an `int` and usable as a `switch` scrutinee.
    enum_bases: std::collections::HashMap<String, Type>,
    /// The functions the compile-time evaluator may run, by name.
    ///
    /// Collected before any body is checked, because a constant may call a
    /// function declared later in the file.
    evaluable_functions: std::collections::HashMap<String, crate::interp::Evaluable>,
    /// The declared type of each constant, by name. See
    /// [`CheckedProgram::constant_types_by_name`].
    constant_types_by_name: std::collections::HashMap<String, crate::types::Type>,
    /// Constants already folded, for a template body being checked.
    ///
    /// A constant is identified by an expression *id*, and an id belongs to one
    /// tree -- so a module's constant cannot be looked up by id while another
    /// module's tree is being checked. It is folded once, where its own tree's
    /// index exists, and the *value* travels instead.
    instantiation_constants: std::collections::HashMap<String, i64>,
    /// The declared type of each value template parameter in scope.
    ///
    /// `template <index usize, string []char>` declares two values, and their
    /// types are what a use of them is checked against: `string[index]` indexes
    /// a character array. The *values* differ per instantiation; the types do
    /// not.
    template_value_types: std::collections::HashMap<String, Type>,
    /// The template parameters in scope, if a template body is being checked.
    ///
    /// A name in it stands for a type the instantiation chose, so resolving a
    /// type consults it before anything else: `T` is not a declared type and
    /// never will be, but inside an instantiation it names a real one.
    substitution: Option<crate::template::Substitution>,
    /// The symbol each instantiation was emitted under, by its key.
    instantiations: std::collections::HashMap<String, String>,
    /// What each instantiation returns, by its key.
    instantiated_returns: std::collections::HashMap<String, Type>,
    /// Whether the comparison being made is of an argument against a
    /// *parameter*, where a value may be passed by address.
    ///
    /// A binding's declared type is a different question: `T x = y` copies, and
    /// accepting a reference parameter's shape there would make every
    /// initialisation an address-taking.
    in_parameter_position: bool,
    /// What `auto` stands for while a binding's type is being resolved.
    ///
    /// Set for the duration of one binding: the initialiser is typed first, its
    /// value type becomes the answer, and the written type -- `auto`, `&auto`,
    /// `*auto`, `[4]auto` -- is resolved with this in place.
    auto_base: Option<Type>,
    /// Whether type resolution is a probe rather than a use.
    ///
    /// Set while resolving a builtin's signature, which is written in prose and
    /// may name a type the program has not declared. A probe that fails is not
    /// an error; a *written* type that fails is.
    resolving_probe: bool,
    /// Whether a type is being resolved as part of a larger type.
    ///
    /// A `const` inside an array element or a pointer target is a qualifier on
    /// the value; the same keyword at the top level of a binding is about the
    /// binding. This flag is what tells the two apart.
    resolving_nested_type: bool,
    /// Name spans of bindings declared `const`.
    ///
    /// Keyed by span, matching how the resolver and the binding type table
    /// identify a declaration.
    const_bindings: std::collections::HashSet<fallc_span::Span>,
    /// Constant expressions, indexed by expression id.
    ///
    /// Array lengths must be compile-time constants, and they are resolved
    /// before normal checking reaches them, so they need a representation that
    /// does not depend on having been type checked. This table is filled by
    /// [`TypeChecker::index_constants`] before checking begins.
    const_exprs: Vec<ConstExpr>,
    /// The module-level constants, by name, holding the expression that gives
    /// each its value.
    ///
    /// Kept as expressions rather than as values because a constant may be
    /// declared after the expression that uses it, and because a constant's
    /// value may itself be computed from another.
    constants: std::collections::HashMap<String, ExprId>,
    /// Spans for those expressions, for diagnostics raised during folding.
    const_expr_spans: Vec<fallc_span::Span>,
    /// The value of each parameter default the compiler could work out, by the
    /// default expression's span.
    default_values: std::collections::HashMap<fallc_span::Span, i64>,
    /// The constant arguments each call fills its omitted parameters with, by
    /// the call's span.
    default_arguments: std::collections::HashMap<
        fallc_span::Span,
        Vec<(usize, i64, crate::types::Type)>,
    >,
    /// The defaults left to an expression, by the call's span.
    default_expressions: ExpressionDefaults,
    /// The type suffix of each literal, `None` where none was written.
    ///
    /// Overload resolution prefers the default type for an unsuffixed literal,
    /// which is the one precision the language has.
    literal_suffixes: Vec<Option<String>>,
    /// Which tree the constant index above was built from.
    ///
    /// An expression id is an index into one tree, so a template body that came
    /// from another module needs its own index for as long as it is being
    /// checked. The address is enough to tell the two apart: the same tree is
    /// the same address, and a body that arrived from a module is a different
    /// one however similar its contents.
    const_index_tree: Option<usize>,
    /// The declared return type of the functions currently being checked,
    /// innermost last.
    ///
    /// A stack rather than one entry because checking a call to a template
    /// instantiation checks that instantiation's body inside the caller's, so
    /// two functions are being checked at once. Functions are only ever
    /// declared at the top level.
    return_types: Vec<(String, Type)>,
}


mod attributes;

pub use attributes::deprecated_message;
mod calls;
mod interfaces;
mod lambdas;
mod expressions;
mod methods;
mod packs;
mod operators;
mod setup;
mod statements;
mod templates;
mod types;


impl Default for TypeChecker {
    fn default() -> Self {
        TypeChecker::new(64)
    }
}


/// The symbol a templated function is registered under.
///
/// # Why it is not `resolve_type_syntax`
///
/// A template's signature is read in the module that *declares* it, before any
/// of that module's own declarations is in scope, so a name in it that is not
/// primitive is unspecified. That is the rule the driver uses when it registers
/// the body, and this is the same rule -- written once, so the two cannot drift
/// apart. They were two a moment ago, and the keys they produced never met.
#[must_use]
pub fn template_symbol_of(name: &str, params: &[fallc_ast::TypeSyntax]) -> String {
    let mangled: Vec<String> = params
        .iter()
        .map(|written| crate::types::mangle_type(&unspecified_signature_type(written)))
        .collect();
    fallc_ast::special::symbol_for(name, &mangled)
}

/// Resolves a written type the way a *signature* is resolved: primitives by
/// name, everything else unspecified.
///
/// A declared type is not in scope where a signature is read, so it has no
/// fields to resolve; what it is named is all that is known, and that is enough
/// to mangle it.
#[must_use]
pub fn unspecified_signature_type(ty: &fallc_ast::TypeSyntax) -> crate::types::Type {
    use crate::types::Type;
    use fallc_ast::TypeSyntax;
    let pointer = |inner: &TypeSyntax, pointee_is_const: bool, pointer_is_const: bool| {
        Type::Pointer {
            pointee: Box::new(unspecified_signature_type(inner)),
            pointee_is_const,
            pointer_is_const,
        }
    };
    let reference = |inner: &TypeSyntax, referent_is_const: bool, is_rvalue: bool| Type::Reference {
        referent: Box::new(unspecified_signature_type(inner)),
        referent_is_const,
        is_rvalue,
    };
    match ty {
        TypeSyntax::Named(named) => Type::from_name(&named.path)
            .unwrap_or_else(|| Type::unspecified(named.path.clone(), named.span)),
        TypeSyntax::Auto(span) => Type::unspecified("auto", *span),
        // A nested type is named through its outer one, which a module reading
        // a written signature cannot resolve; the checker that did knows.
        TypeSyntax::Member(member) => {
            Type::unspecified(member.name.clone(), member.span)
        }
        TypeSyntax::Pointer(inner) => pointer(inner, false, false),
        TypeSyntax::PointerToConst(inner) => pointer(inner, true, false),
        TypeSyntax::ConstPointer(inner) => pointer(inner, false, true),
        TypeSyntax::Reference(inner) => reference(inner, false, false),
        TypeSyntax::ConstReference(inner) => reference(inner, true, false),
        TypeSyntax::RvalueReference(inner) => reference(inner, false, true),
        TypeSyntax::ConstRvalueReference(inner) => reference(inner, true, true),
        TypeSyntax::Volatile(inner) => {
            Type::Volatile(Box::new(unspecified_signature_type(inner)))
        }
        TypeSyntax::Const(inner) => Type::Const(Box::new(unspecified_signature_type(inner))),
        // The type an expression has is known to the checker that asked, and
        // not to a module reading the written form.
        TypeSyntax::TypeOf(node) => {
            Type::unspecified("type(..)".to_owned(), node.span)
        }
        TypeSyntax::DecltypeAuto(span) => Type::unspecified("type(auto)".to_owned(), *span),
        // `[]T` in a parameter list is `*T`, and `[]const T` is `*const T` --
        // the same rule `resolve_type_syntax` applies. The two must agree:
        // this one names a definition, that one names a call, and a symbol
        // built from two different types is a call to nothing.
        TypeSyntax::UnsizedArray(inner) => match inner.as_ref() {
            TypeSyntax::Const(element) => pointer(element, true, false),
            element => pointer(element, false, false),
        },
        TypeSyntax::Array(array) => Type::Array {
            element: Box::new(unspecified_signature_type(&array.element)),
            len: 0,
        },
        // The parts of a function type are written types too, and they are what
        // makes two of them different. The function itself is what a caller
        // needs: a call through a pointer to one is checked against them.
        TypeSyntax::Function(function) => Type::Function {
            parameters: function
                .parameters
                .iter()
                .map(unspecified_signature_type)
                .collect(),
            returns: Box::new(unspecified_signature_type(&function.returns)),
            abi: function.abi.clone(),
            variadic: function.variadic,
            throws: function.throws,
        },
        TypeSyntax::Unresolved(u) => Type::unspecified(u.text.clone(), u.span),
    }
}
/// Whether a type is a reference through which nothing may be written.
///
/// Written as a free function because it is a fact about the type alone, and
/// the overload rule that needs it has no other use for a `&mut self`.
#[must_use]
fn fallc_sema_type_is_const_reference(ty: &Type) -> bool {
    ty.is_reference() && ty.pointee_is_const()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_ast::{Ast, ExprKind, Unresolved};
    use fallc_span::{FileId, Span};

    fn span(lo: u32, hi: u32) -> Span {
        Span::new(FileId::from_raw(0), lo, hi)
    }

    #[test]
    fn checking_an_empty_ast_is_ok() {
        let mut checker = TypeChecker::new(64);
        let result = checker.check(&Ast::new(), &Resolutions::new());
        assert!(result.is_ok());
        assert!(result.types.is_empty());
    }

    #[test]
    fn placeholder_expression_has_no_type_and_no_noise() {
        let mut ast = Ast::new();
        let e = ast.builder().alloc_expr(
            ExprKind::Unresolved(Unresolved::new("x", span(0, 1))),
            span(0, 1),
        );
        let mut checker = TypeChecker::new(64);
        let mut types = TypeTable::new();
        let result = checker.infer_expr(&ast, &Resolutions::new(), &mut types, e);
        assert!(result.is_none());
        assert!(checker.diagnostics().is_empty());
    }

    #[test]
    fn compatibility_is_exact_until_specified() {
        let checker = TypeChecker::new(64);
        let a = Type::unspecified("i32", Span::SYNTHETIC);
        let b = Type::unspecified("i32", Span::SYNTHETIC);
        let c = Type::unspecified("u32", Span::SYNTHETIC);
        assert!(checker.is_compatible(&a, &b));
        assert!(!checker.is_compatible(&a, &c));
    }

    #[test]
    fn describe_uses_canonical_spelling() {
        let checker = TypeChecker::new(64);
        // An empty spelling is `void`: a function that returns nothing is
        // written with no type, and a diagnostic must say so rather than name
        // the compiler's placeholder.
        assert_eq!(
            checker.describe(&Type::unspecified("", Span::SYNTHETIC)),
            "void"
        );
        // An unrecognised name is echoed as written, since it is probably a
        // user-defined type the compiler simply does not know yet.
        assert_eq!(
            checker.describe(&Type::unspecified("MyType", Span::SYNTHETIC)),
            "MyType"
        );
        // Aliases describe as what they are.
        assert_eq!(checker.describe(&Type::INT), "i32");
        assert_eq!(checker.describe(&Type::UINT), "u32");
        // `byte` describes as itself: it is not an alias for `u8`.
        assert_eq!(checker.describe(&Type::BYTE), "byte");
        assert_eq!(checker.describe(&Type::ISIZE), "i64");
        assert_eq!(checker.describe(&Type::USIZE), "u64");
    }
}

/// One entry of a template's argument list, after expansion.
///
/// A pack expansion produces arguments that are already resolved, because a
/// pack's elements were reduced to their values when it was bound. Everything
/// else is still written syntax, waiting for the parameter it lands on to say
/// what kind it is.
#[derive(Clone, Debug)]
enum ExpandedArgument {
    /// Written in the source, not yet resolved.
    Written(fallc_ast::TemplateArgumentSyntax),
    /// Produced by expanding a pack, already resolved.
    Resolved(crate::template::TemplateArgument),
}

/// Applies a binary operator to two integers, for folding a pack.
///
/// `None` on overflow or division by zero: a fold is a constant, and a value
/// that does not exist is not one.
/// Returns the value an empty fold of `op` has, if the language gives it one.
fn fold_identity(op: BinaryOp) -> Option<i64> {
    match op {
        // The one the language states: the conjunction of nothing is true and
        // the disjunction of nothing is false. There is no comma operator to
        // make a third identity, so a comma fold of nothing has no value.
        BinaryOp::LogicalAnd => Some(1),
        BinaryOp::LogicalOr => Some(0),
        _ => None,
    }
}

fn apply_binary_op(op: BinaryOp, lhs: i64, rhs: i64) -> Option<i64> {
    match op {
        BinaryOp::Add => lhs.checked_add(rhs),
        BinaryOp::Sub => lhs.checked_sub(rhs),
        BinaryOp::Mul => lhs.checked_mul(rhs),
        BinaryOp::Div => lhs.checked_div(rhs),
        BinaryOp::Rem => lhs.checked_rem(rhs),
        BinaryOp::BitAnd => Some(lhs & rhs),
        BinaryOp::BitOr => Some(lhs | rhs),
        BinaryOp::BitXor => Some(lhs ^ rhs),
        BinaryOp::Shl => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shl(s)),
        BinaryOp::Shr => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shr(s)),
        _ => None,
    }
}

/// Why an expression is not usable where the language requires a constant.
///
/// Spelled out rather than reported as one message, because the useful part is
/// *which* rule was broken: a call to a function that is merely not marked, a
/// call with an argument that is not constant, or an expression that is not
/// constant at all.
#[derive(Clone, Debug)]
pub enum ConstantProblem {
    /// A call to a `constexpr` function with an argument that is not constant.
    Argument {
        /// Which argument, counting from zero.
        index: usize,
        /// The function being called.
        callee: String,
        /// Why that argument is not constant.
        inner: Box<ConstantProblem>,
    },
    /// A call to a function that is not declared `constexpr`.
    NotConstexpr(String),
    /// A call to something that is not a function this compiler can run.
    NotAFunction(String),
    /// A name that is not a constant.
    NotAConstantName(String),
    /// An expression that has no value known before the program runs.
    NotFoldable,
}
