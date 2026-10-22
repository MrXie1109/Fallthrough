//! Types as they are written.
//!
//! A type in the tree is syntax, not a resolved type: `Array<T>` is a name with
//! arguments until the checker resolves it, and a type the parser could not
//! recognise is kept as [`Unresolved`] so recovery can continue.

use super::*;

/// A type annotation as written in the source.
///
/// The prefix operators follow the language's rule: `*T` is a pointer,
/// `*const T` points to constant data, `const *T` is a constant pointer, `&T`
/// is a reference, `[N]T` is an array, and `auto` asks for deduction. `const`
/// binds to what follows it, which is why `*const T` and `const *T` differ. A
/// type the parser could not recognise is kept as [`TypeSyntax::Unresolved`],
/// with its raw text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TypeSyntax {
    /// A named type, possibly qualified as a path such as `std.Exception`.
    Named(NamedType),
    /// `[N]T` — a fixed-size array, stored inline.
    Array(ArrayType),
    /// `[]T` — a slice-like array of unknown length.
    ///
    /// As a parameter type it is equivalent to `*T`; see the language notes.
    UnsizedArray(Box<TypeSyntax>),
    /// `*T` — a pointer to a mutable `T`.
    Pointer(Box<TypeSyntax>),
    /// `*const T` — a pointer to a constant `T`.
    ///
    /// The pointee cannot be written through this pointer.
    PointerToConst(Box<TypeSyntax>),
    /// `const *T` — a constant pointer to a mutable `T`.
    ///
    /// The pointer itself cannot be reassigned, but the pointee can be
    /// written. Note that `const` binds to what follows it, so this and
    /// [`TypeSyntax::PointerToConst`] mean different things despite both
    /// containing `const` and `*`.
    ConstPointer(Box<TypeSyntax>),
    /// `&T` — a reference, implemented as a pointer.
    Reference(Box<TypeSyntax>),
    /// `&const T` — a reference through which the referent cannot be written.
    ConstReference(Box<TypeSyntax>),
    /// `&&T` — an *rvalue* reference, which binds to a temporary.
    ///
    /// What makes moving possible: `T(T&&)` is the move constructor, and it is
    /// chosen exactly when the argument is a value about to disappear. An
    /// lvalue reference cannot express that, because it binds to things that
    /// will still be there afterwards.
    RvalueReference(Box<TypeSyntax>),
    /// `&&const T` — an rvalue reference whose referent cannot be written.
    ConstRvalueReference(Box<TypeSyntax>),
    /// `auto` — the type is deduced from the initialiser.
    ///
    /// It may be written alone or under a modifier: `&auto` is a reference to
    /// whatever the initialiser names, `*auto` a pointer to it, `const auto` a
    /// constant one. The deduction supplies the *value* type, so `&auto` and a
    /// bare `auto` differ in exactly the reference the author wrote.
    Auto(Span),
    /// `volatile T` — memory every access to which is a real access.
    ///
    /// Written where `const` is, and able to appear with it: `volatile const T`
    /// is memory that is read exactly as written and never written at all.
    Volatile(Box<TypeSyntax>),
    /// `const T` — an immutable binding.
    ///
    /// A different use of the keyword from the pointer qualifiers above: here
    /// it says the *binding* cannot be reassigned, not that anything behind a
    /// pointer is constant.
    Const(Box<TypeSyntax>),
    /// `type(expr)` — the type an expression has.
    ///
    /// The keyword that *declares* a type also *asks* for one. With the
    /// operand written bare, `type(a)` is the declared type of `a`; with it
    /// parenthesised, `type((a))` is the type of the expression `a`, which is
    /// a reference when `a` names storage. That is `decltype`'s rule, and the
    /// extra parentheses are the whole of it.
    TypeOf(TypeOfType),
    /// `Outer<args>.Name` — a type declared inside another one.
    ///
    /// The outer part is a *type*, not a path segment: `Array<int>` and
    /// `Array<u8>` declare different members.
    Member(TypeMember),
    /// `fn(int) int` — a function type. `*fn(int) int` is a pointer to one.
    Function(FunctionTypeSyntax),
    /// `type(auto)` — the type of an expression, as `decltype(auto)` means it.
    ///
    /// The same answer `type(expr)` gives for the initialiser or the `return`
    /// expression: a dereference or a subscript names a place, so the type is a
    /// reference, while a name is the type it was declared with.
    DecltypeAuto(Span),
    /// A type the parser could not recognise, kept as its raw text.
    Unresolved(Unresolved),
}

/// A type named through another: `Outer<args>.Name`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeMember {
    /// The type that declares the member.
    pub outer: Box<TypeSyntax>,
    /// The member's own name.
    pub name: String,
    /// The span of the whole qualified name.
    pub span: Span,
}

/// `type(expr)` — the type of an expression, as written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeOfType {
    /// The expression whose type is asked for.
    pub operand: ExprId,
    /// Whether the operand was written in parentheses.
    ///
    /// This is the difference between the two forms: `type(a)` asks what `a`
    /// *is*, and `type((a))` asks what the expression `a` *is* -- an lvalue of
    /// that type, which the type system spells `&T`.
    pub parenthesized: bool,
    /// Where it was written.
    pub span: Span,
}

/// A named type, possibly a dotted path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NamedType {
    /// The path as written, e.g. `int` or `std.Exception`.
    pub path: String,
    /// The template arguments, empty for an ordinary name.
    ///
    /// `Array<int>` names one member of a family, and which member is the
    /// arguments' business. A name with none is what every name was before
    /// there were templates.
    pub arguments: Vec<TemplateArgumentSyntax>,
    /// Whether the name is an *expansion* of a parameter pack, written `Args...`.
    ///
    /// Expanding a pack in a type position yields one type per element, so it
    /// is meaningful only where several types are wanted: the arguments of
    /// another template. `Box<Args...>` is one `Box` per element, and a bare
    /// `Args` is not a type at all.
    pub is_expansion: bool,
    /// Where it was written.
    pub span: Span,
}

/// `fn(int) int` — a function type.
///
/// A function type is written with its parameter types and then its return
/// type, like the declaration it describes. It is a *type*, so a pointer to a
/// function is `*fn(int) int`: the two are as different as `T` and `*T`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FunctionTypeSyntax {
    /// The ABI, if one is written: `fn "C" (int) int`.
    pub abi: Option<String>,
    /// The parameter types, in order.
    pub parameters: Vec<TypeSyntax>,
    /// Whether the list ends with `...`.
    pub variadic: bool,
    /// Whether a call of it may raise: `fn(int) int except`.
    ///
    /// Part of the type, because the two have different calling conventions: a
    /// function that may raise returns its result through the exception
    /// structure, so a pointer to one is not a pointer to the other. A written
    /// function type without the marker is a function that cannot raise, which
    /// is also the type of every C function.
    pub throws: bool,
    /// The type returned.
    pub returns: Box<TypeSyntax>,
    /// Where it was written.
    pub span: Span,
}

/// `[N]T` — a fixed-size array.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArrayType {
    /// The length expression, which must be a compile-time constant.
    pub len: ExprId,
    /// The span of the length expression, which is where a fold is filed.
    pub len_span: Span,
    /// The element type.
    pub element: Box<TypeSyntax>,
}

impl TypeSyntax {
    /// Returns the source span of this type annotation.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            TypeSyntax::Named(named) => named.span,
            TypeSyntax::Auto(span) => *span,
            TypeSyntax::Member(member) => member.span,
            TypeSyntax::Function(function) => function.span,
            TypeSyntax::DecltypeAuto(span) => *span,
            TypeSyntax::TypeOf(node) => node.span,
            TypeSyntax::Array(array) => array.element.span(),
            TypeSyntax::UnsizedArray(inner)
            | TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner)
            | TypeSyntax::Reference(inner)
            | TypeSyntax::ConstReference(inner)
            | TypeSyntax::RvalueReference(inner)
            | TypeSyntax::ConstRvalueReference(inner)
            | TypeSyntax::Volatile(inner)
            | TypeSyntax::Const(inner) => inner.span(),
            TypeSyntax::Unresolved(u) => u.span,
        }
    }

    /// Returns the name of this type if it is a simple unqualified name.
    ///
    /// Used by the type checker to look up primitives without going through a
    /// string round trip.
    #[must_use]
    pub fn as_name(&self) -> Option<&str> {
        match self {
            TypeSyntax::Named(named) if !named.path.contains('.') => Some(&named.path),
            _ => None,
        }
    }

    /// Returns `true` if this type is `const`.
    ///
    /// Only the outermost qualifier is reported, which is what a binding cares
    /// about.
    #[must_use]
    pub fn is_const(&self) -> bool {
        matches!(self, TypeSyntax::Const(_))
    }
}

/// Raw source text plus its span, for a construct the parser could not build.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Unresolved {
    /// The exact source text.
    pub text: String,
    /// Where it came from.
    pub span: Span,
}

impl Unresolved {
    /// Creates an unresolved fragment.
    #[must_use]
    pub fn new(text: impl Into<String>, span: Span) -> Self {
        Unresolved {
            text: text.into(),
            span,
        }
    }
}

