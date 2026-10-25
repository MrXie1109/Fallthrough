//! Types.
//!
//! A [`Type`] is a primitive ([`IntType`], [`FloatType`], `bool`, `byte`,
//! [`CharType`]), a nominal type declared with `type`, or a composition:
//! pointer, reference, array, `const`, `volatile`.
//!
//! The integer names state their width and signedness, so a width is fixed by
//! the name rather than by the platform; `int`, `uint`, `isize`, and `usize`
//! are aliases. `byte` and `c8`/`c16`/`c32` are distinct types over the integer
//! of their width. Slices and tuples are not provided: an array is the only
//! aggregate besides a nominal type. There is no inference beyond `auto`, and no
//! bottom type.
//!
//! [`Type::Unspecified`] is the placeholder for a type that could not be
//! resolved. It is compatible with nothing, so an unknown name is reported once
//! and does not hide the error behind a cascade.

use fallc_ast::ExprId;
use fallc_span::Span;

/// A primitive integer type.
///
/// All integers are two's complement. The name fixes the width and signedness,
/// so `i32` is 32 bits wherever it is compiled. Signed overflow is a panic in a
/// debug build and undefined in a release one; unsigned arithmetic wraps. See
/// `fallc-ir::codegen` for how overflow is lowered.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum IntType {
    /// `i8`
    I8,
    /// `i16`
    I16,
    /// `i32`
    I32,
    /// `i64`
    I64,
    /// `i128`
    I128,
    /// `u8`
    U8,
    /// `u16`
    U16,
    /// `u32`
    U32,
    /// `u64`
    U64,
    /// `u128`
    U128,
}

impl IntType {
    /// Returns the type's width in bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            IntType::I8 | IntType::U8 => 8,
            IntType::I16 | IntType::U16 => 16,
            IntType::I32 | IntType::U32 => 32,
            IntType::I64 | IntType::U64 => 64,
            IntType::I128 | IntType::U128 => 128,
        }
    }

    /// Returns `true` if the type is unsigned.
    #[must_use]
    pub const fn is_unsigned(self) -> bool {
        matches!(
            self,
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::U64 | IntType::U128
        )
    }

    /// Returns the canonical spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            IntType::I8 => "i8",
            IntType::I16 => "i16",
            IntType::I32 => "i32",
            IntType::I64 => "i64",
            IntType::I128 => "i128",
            IntType::U8 => "u8",
            IntType::U16 => "u16",
            IntType::U32 => "u32",
            IntType::U64 => "u64",
            IntType::U128 => "u128",
        }
    }
}

/// A floating-point type.
/// A character type, named for its width in bits.
///
/// The three are distinct types, not one type of varying width: a `c8` is not a
/// `c16`, and converting between them needs `as`, exactly as between `u8` and
/// `u16`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CharType {
    /// `c8`, based on `u8`. What `char` names, and what a string holds.
    C8,
    /// `c16`, based on `u16`.
    C16,
    /// `c32`, based on `u32`. A Unicode code point.
    C32,
}

impl CharType {
    /// Returns the type's width in bits, which is also its integer base's.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            CharType::C8 => 8,
            CharType::C16 => 16,
            CharType::C32 => 32,
        }
    }

    /// Returns the canonical spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            CharType::C8 => "c8",
            CharType::C16 => "c16",
            CharType::C32 => "c32",
        }
    }

    /// Returns the unsigned integer type this character type is based on.
    #[must_use]
    pub const fn base(self) -> IntType {
        match self {
            CharType::C8 => IntType::U8,
            CharType::C16 => IntType::U16,
            CharType::C32 => IntType::U32,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FloatType {
    /// `float`
    Single,
    /// `double`
    Double,
}

impl FloatType {
    /// Returns the type's width in bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            FloatType::Single => 32,
            FloatType::Double => 64,
        }
    }

    /// Returns the canonical spelling.
    ///
    /// The `f`-prefixed name, because that is the type's real name: `float` and
    /// `double` are aliases kept for familiarity, and a diagnostic that used
    /// one would be describing the same type by a different word depending on
    /// where the reader looked.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            FloatType::Single => "f32",
            FloatType::Double => "f64",
        }
    }
}

/// A type in the Fallthrough type system.
///
/// # Aliases
///
/// Some primitive names are aliases rather than distinct types:
///
/// | Alias | Resolves to |
/// | ----- | ----------- |
/// | `int` | `i32` |
/// | `uint` | `u32` |
/// | `isize` | `i64` |
/// | `usize` | `u64` |
///
/// Aliases are resolved at this level rather than kept as separate types, so
/// that `int` and `i32` compare equal and lower to the same LLVM type. Code that
/// wants to print the name the user wrote must keep the written form itself;
/// [`Type::spelling`] gives the canonical one.
///
/// # `byte` is not an alias
///
/// `byte` is *based on* `u8` but is a **distinct type**: [`Type::Byte`] and
/// [`Type::Int`] holding a `u8` are not interchangeable, and converting between
/// them needs an explicit conversion. They share an LLVM representation, which
/// is a lowering detail and not a statement about the type system.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    /// An integer.
    Int(IntType),
    /// The `byte` type.
    ///
    /// Distinct from `u8`; see the type-level docs.
    Byte,
    /// A nominal type declared by `type Name { ... }`.
    ///
    /// Identified by name rather than by its fields, so two declarations with
    /// the same fields are still different types. The fields are reachable
    /// through the type table, because a type is cheap to copy and its
    /// definition is not.
    Nominal(NominalType),
    /// A character type: `c8`, `c16`, or `c32`.
    ///
    /// Based on the unsigned integer of the same width but a distinct type, as
    /// `byte` is based on `u8`. Characters take no part in arithmetic.
    Character(CharType),
    /// The type of `nullptr`, before it is used at a pointer type.
    ///
    /// A distinct type rather than a constant of some pointer type: `nullptr`
    /// is valid wherever any pointer is wanted, and giving it one particular
    /// pointer type would make every other one need a cast. It converts to an
    /// indirection and to nothing else, so `nullptr + 1` is an error.
    NullPointer,
    /// The `bool` type.
    ///
    /// A distinct type like `byte`, and the result type of a comparison.
    Bool,
    /// The built-in `String` type.
    ///
    /// It exists so that a signature the compiler itself writes can say
    /// `String`. A program's string type is the declared `std.String`, which is
    /// a nominal type; nothing has this primitive type as its value's type.
    String,
    /// A pointer to `pointee`.
    ///
    /// `is_const_pointer` records whether the *pointer* may be reassigned, and
    /// `pointee_is_const` whether the pointee may be written through it. The
    /// two are independent, which is why `const *T` and `*const T` differ.
    Pointer {
        /// The pointed-to type.
        pointee: Box<Type>,
        /// Whether the pointee may be written through this pointer.
        pointee_is_const: bool,
        /// Whether the pointer itself may be reassigned.
        pointer_is_const: bool,
    },
    /// A reference, which the language defines in terms of a pointer.
    ///
    /// Modelled as its own variant rather than as sugar for a pointer so that
    /// diagnostics can name it correctly and so that reference-specific rules
    /// (no null, no rebinding) have somewhere to live.
    Reference {
        /// The referred-to type.
        referent: Box<Type>,
        /// Whether the referent may be written through this reference.
        referent_is_const: bool,
        /// Whether this is an *rvalue* reference, which binds to a temporary.
        ///
        /// The distinction is what makes moving expressible: `T(T&&)` is chosen
        /// exactly when the argument is about to disappear, and a plain
        /// reference cannot say that, because it binds to things that will
        /// still be there afterwards.
        is_rvalue: bool,
    },
    /// A fixed-size array of `element` repeated `len` times.
    Array {
        /// The element type.
        element: Box<Type>,
        /// The number of elements.
        len: u64,
    },
    /// A `const`-qualified value type, as in the element type of `[N]const byte`.
    ///
    /// This is distinct from the `const` that qualifies a *binding*, which is
    /// not part of the type at all. Here the qualifier is part of the type
    /// because it describes the value: the bytes of a byte-string literal
    /// cannot be written through, whoever holds them.
    Const(Box<Type>),
    /// A function type, as written `fn(int) int`.
    ///
    /// A *value* of this type is a function, and a function name used as a
    /// value has it. A pointer to a function is `Pointer { pointee: Function }`,
    /// which is what `*fn(int) int` names: the function type and the pointer to
    /// it are different types, exactly as `T` and `*T` are.
    Function {
        /// The parameter types, in order.
        parameters: Vec<Type>,
        /// The type the function returns.
        returns: Box<Type>,
        /// The ABI, if one was written: `fn "C" (int) int`.
        abi: Option<String>,
        /// Whether the parameter list ends with `...`.
        variadic: bool,
        /// Whether a call of it may raise.
        ///
        /// A written function type never carries `except` -- the marker is not
        /// part of the syntax -- so this is `true` only for the type of a
        /// function *declaration* that may raise. That is what makes
        /// `p *fn(int) int = risky;` a mismatch rather than a call through a
        /// pointer to a function compiled with the exception ABI: the result
        /// of the call would be a struct, and the pointer's caller reads an
        /// `int`.
        throws: bool,
    },
    /// An array of unknown length, as written `[]T`.
    ///
    /// Only meaningful as a parameter type, where it means a pointer; the
    /// parser records what was written and the checker decides.
    UnsizedArray(Box<Type>),
    /// A floating-point number.
    Float(FloatType),
    /// `volatile T` — memory every access to which is a real access.
    ///
    /// A property of the *storage* rather than of the value: the same `int` is
    /// volatile in a device register and not in a local. Two `volatile int`s
    /// are the same type, and a `volatile int` is not an `int` -- reading one
    /// is a different operation, so a program that means it has to say it.
    Volatile(Box<Type>),
    /// A type that could not be resolved, kept as its raw source text.
    ///
    /// The name is reported where it is resolved, so nothing downstream has to
    /// diagnose it twice.
    Unspecified(UnspecifiedType),
}

/// Returns `true` if `found` is a pointer to `byte` and `expected` is any
/// other pointer, or the reverse.
///
/// Returns `true` if this type is a pointer to `c8`, which is how a C string
/// is declared.
///
/// The one type a string literal's constructor must accept: `*const c8` is what
/// `String`'s constructor takes, and recognising it is what lets a literal be
/// constructed into a declared type.
#[must_use]
pub fn is_c_byte_pointer(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Pointer { pointee, .. }
            if matches!(pointee.as_ref().unqualified(), Type::Character(CharType::C8))
    )
}

/// A `*byte` is the language's untyped pointer: it is what raw memory and the C
/// library traffic in, so it converts to and from a pointer to anything without
/// a cast. This is the one pointer conversion that does not have to preserve
/// shape.
///
/// The `const` qualifiers still have to be respected. `*T` may become
/// `*const byte`, because adding const is always safe, but a `*const T` may not
/// become a `*byte` implicitly: that would hand out a writable pointer to
/// constant data. Written as a cast it may, which is what
/// [`is_byte_pointer_cast`] is for.
#[must_use]
pub fn is_byte_pointer_conversion(found: &Type, expected: &Type) -> bool {
    let (
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
    ) = (found, expected)
    else {
        return false;
    };
    // The pointer itself may gain const, never lose it.
    if *found_pointer_const && !*expected_pointer_const {
        return false;
    }
    // Exactly one side must be a byte pointer; two byte pointers are the same
    // type, which plain equality already covers.
    if found_pointee.is_byte() == expected_pointee.is_byte() {
        return false;
    }
    // The pointee's constness may only accumulate, whichever side is the byte
    // pointer: `*T` -> `*const byte` is fine, `*const T` -> `*byte` is not.
    !*found_const || *expected_const
}

/// Returns `true` if one side is a byte pointer and the other is not, whatever
/// their `const` qualifiers say.
///
/// This is the byte-pointer rule as an explicit `as` applies it: dropping
/// `const` is what a cast is for, so `p as *byte` written on a `*const T` is
/// allowed. The implicit conversion keeps the stricter
/// [`is_byte_pointer_conversion`], because a value should not lose `const`
/// merely by being passed somewhere.
#[must_use]
pub fn is_byte_pointer_cast(found: &Type, expected: &Type) -> bool {
    let (
        Type::Pointer {
            pointee: found_pointee,
            ..
        },
        Type::Pointer {
            pointee: expected_pointee,
            ..
        },
    ) = (found, expected)
    else {
        return false;
    };
    // Exactly one side must be a byte pointer: two byte pointers are the same
    // type, which plain equality already covers.
    found_pointee.is_byte() != expected_pointee.is_byte()
}

impl Type {
    /// The `byte` type.
    pub const BYTE: Type = Type::Byte;
    /// The `char` type, which is `c8`.
    pub const CHAR: Type = Type::Character(CharType::C8);
    /// The `c8` type.
    pub const C8: Type = Type::Character(CharType::C8);
    /// The `c16` type.
    pub const C16: Type = Type::Character(CharType::C16);
    /// The `c32` type.
    pub const C32: Type = Type::Character(CharType::C32);
    /// The `bool` type.
    pub const BOOL: Type = Type::Bool;
    /// The `int` type, an alias for `i32`.
    pub const INT: Type = Type::Int(IntType::I32);
    /// The `uint` type, an alias for `u32`.
    pub const UINT: Type = Type::Int(IntType::U32);
    /// The `isize` type, an alias for `i64`.
    pub const ISIZE: Type = Type::Int(IntType::I64);
    /// The `usize` type, an alias for `u64`.
    pub const USIZE: Type = Type::Int(IntType::U64);

    /// Creates a placeholder type from source text.
    #[must_use]
    pub fn unspecified(text: impl Into<String>, span: Span) -> Self {
        Type::Unspecified(UnspecifiedType {
            text: text.into(),
            span,
        })
    }

    /// Returns the source span this type was written at, if any.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            Type::Unspecified(u) => Some(u.span),
            _ => None,
        }
    }

    /// Returns `true` if this type is still unspecified.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Type::Unspecified(_))
    }

    /// Resolves a type name as written in source to a type.
    ///
    /// Returns `None` for a name that is not a primitive, which includes
    /// user-defined types and any path such as `std.Exception`. A declared type
    /// is looked up before this table, so a program may name a type after a
    /// built-in one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Type> {
        Some(match name {
            "i8" => Type::Int(IntType::I8),
            "i16" => Type::Int(IntType::I16),
            "i32" => Type::Int(IntType::I32),
            "i64" => Type::Int(IntType::I64),
            "i128" => Type::Int(IntType::I128),
            "u8" => Type::Int(IntType::U8),
            "u16" => Type::Int(IntType::U16),
            "u32" => Type::Int(IntType::U32),
            "u64" => Type::Int(IntType::U64),
            "u128" => Type::Int(IntType::U128),
            // Aliases.
            "int" => Type::INT,
            "uint" => Type::UINT,
            "isize" => Type::ISIZE,
            "usize" => Type::USIZE,
            // `byte` is a distinct type, not an alias for `u8`.
            "byte" => Type::BYTE,
            "bool" => Type::BOOL,
            // `char` names `c8`, the type a string holds.
            "char" => Type::CHAR,
            "c8" => Type::C8,
            "c16" => Type::C16,
            "c32" => Type::C32,
            // The placeholder string type, so that a builtin signature written
            // as `String` resolves.
            "String" => Type::String,
            // Floating point. `float` and `double` are aliases for the names
            // the types actually have.
            "f32" | "float" => Type::Float(FloatType::Single),
            "f64" | "double" => Type::Float(FloatType::Double),
            _ => return None,
        })
    }

    /// Returns `true` if this is an integer type.
    ///
    /// `byte` is not an integer for this purpose: it does not participate in
    /// integer arithmetic or conversions without an explicit cast.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int(_))
    }

    /// Returns `true` if this is the `byte` type.
    #[must_use]
    pub fn is_byte(&self) -> bool {
        matches!(self, Type::Byte)
    }

    /// Returns `true` if this is the `bool` type.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Bool)
    }

    /// Returns `true` if this is the `char` type.
    #[must_use]
    pub fn is_char(&self) -> bool {
        matches!(self, Type::Character(_))
    }

    /// Returns the character type behind this type, if it is one.
    #[must_use]
    pub fn as_char(&self) -> Option<CharType> {
        match self {
            Type::Character(character) => Some(*character),
            _ => None,
        }
    }

    /// Returns `true` if this is a nominal type.
    #[must_use]
    pub fn is_nominal(&self) -> bool {
        matches!(self, Type::Nominal(_))
    }

    /// Returns the nominal type behind this type, if it is one.
    #[must_use]
    pub fn as_nominal(&self) -> Option<&NominalType> {
        match self {
            Type::Nominal(nominal) => Some(nominal),
            _ => None,
        }
    }

    /// Returns the name of the declared type this is, looking through
    /// indirection and `const`.
    ///
    /// A method's receiver is a pointer to the type, so `*Fraction` and
    /// `*const Fraction` both name `Fraction`; a value of the type names it
    /// directly. Anything else is not a declared type and so has no methods.
    #[must_use]
    /// Returns the declared type behind this one, looking through qualifiers.
    ///
    /// `volatile T` and `const T` are `T` with a fact recorded, so a field
    /// lookup belongs to the underlying declaration.
    pub fn nominal(&self) -> Option<&NominalType> {
        match self {
            Type::Nominal(nominal) => Some(nominal),
            Type::Const(inner) | Type::Volatile(inner) => inner.nominal(),
            Type::Reference { referent, .. } => referent.nominal(),
            Type::Pointer { pointee, .. } => pointee.nominal(),
            _ => None,
        }
    }

    /// Returns the declared name of this type, looking through qualifiers.
    #[must_use]
    pub fn nominal_name(&self) -> Option<&str> {
        match self {
            Type::Nominal(nominal) => Some(&nominal.name),
            Type::Pointer { pointee, .. } => pointee.nominal_name(),
            Type::Reference { referent, .. } => referent.nominal_name(),
            Type::Const(inner) => inner.nominal_name(),
            _ => None,
        }
    }

    /// Returns the name of the type that a value of this type *is*, when it is
    /// a named type.
    ///
    /// A reference is transparent, because `*p = v` and `x = v` both assign
    /// the value they name. A pointer is not: `*seen = p` where `seen` is
    /// `**B` assigns a *pointer*, and `B` is not what stands on the left --
    /// reaching through the pointer here made that assignment call
    /// `__assign__B` with the pointer's value as the receiver's address.
    #[must_use]
    pub fn receiver_base_name(&self) -> Option<&str> {
        match self {
            Type::Nominal(nominal) => Some(&nominal.name),
            Type::Reference { referent, .. } => referent.receiver_base_name(),
            Type::Const(inner) => inner.receiver_base_name(),
            _ => None,
        }
    }

    /// Returns `true` if this is `nullptr`'s type.
    #[must_use]
    pub fn is_null_pointer(&self) -> bool {
        matches!(self, Type::NullPointer)
    }

    /// Removes a module qualification from every nominal name inside this type.
    ///
    /// A type reached through an umbrella is `std.String` at the call site and
    /// `String` where it was declared. They are one type, so lowering works on
    /// the declared spelling: the symbol a method was emitted under uses the
    /// name its own module wrote.
    pub fn strip_module_qualification(&mut self) {
        match self {
            Type::Nominal(nominal) => {
                if let Some((_, last)) = nominal.name.rsplit_once('.') {
                    nominal.name = last.to_owned();
                }
            }
            Type::Pointer { pointee, .. } => pointee.strip_module_qualification(),
            Type::Reference { referent, .. } => referent.strip_module_qualification(),
            Type::Array { element, .. } | Type::UnsizedArray(element) => {
                element.strip_module_qualification();
            }
            Type::Const(inner) => inner.strip_module_qualification(),
            _ => {}
        }
    }

    /// Returns `true` if a value of this type is C text.
    ///
    /// What `raise` takes for its type and message, and what `printf("%s")`
    /// takes: a string literal (`[N]const c8`), a `*const c8`, or a `String`.
    /// The last is a nominal type, so it is recognised by name -- the language
    /// has no trait that would say "this can be read as text".
    #[must_use]
    pub fn is_string_like(&self) -> bool {
        match self {
            Type::Array { element, .. } | Type::UnsizedArray(element) => {
                element.unqualified().is_char()
            }
            Type::Pointer { pointee, .. } => pointee.unqualified().is_char(),
            Type::Nominal(nominal) => nominal.name.rsplit('.').next() == Some("String"),
            Type::Const(inner) => inner.is_string_like(),
            _ => false,
        }
    }

    /// Returns `true` if this is a pointer.
    #[must_use]
    pub fn is_pointer(&self) -> bool {
        matches!(self, Type::Pointer { .. })
    }

    /// Returns `true` if this is a reference.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        matches!(self, Type::Reference { .. })
    }

    /// Returns `true` if this is a declared type that has no definition yet.
    ///
    /// An incomplete type has no layout and no members, so only a pointer or a
    /// reference to it may be written while its definition is elsewhere.
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        match self.unqualified() {
            Type::Nominal(nominal) => nominal.is_incomplete,
            _ => false,
        }
    }

    /// Returns `true` if this is a pointer or a reference.
    ///
    /// Both are indirections, so they behave alike wherever indirection is what
    /// matters — truthiness, for instance.
    #[must_use]
    pub fn is_indirection(&self) -> bool {
        self.is_pointer() || self.is_reference()
    }

    /// Returns the type this qualifies, if it is `const`.
    ///
    /// Used to see through a `const` when the qualifier is not the question
    /// being asked.
    #[must_use]
    pub fn unqualified(&self) -> &Type {
        match self {
            Type::Const(inner) => inner,
            other => other,
        }
    }

    /// Returns `true` if this type means "no value".
    ///
    /// A function with no written return type has one of these. It is spelled
    /// as an empty name because `void` is not a type in the language: a
    /// function either returns something or returns nothing.
    #[must_use]
    pub fn is_void(&self) -> bool {
        // Two spellings mean the same thing: no return type at all, which is
        // what the empty name records, and the word `void`. Missing the second
        // made every `fn f(..) void` from another module impossible to declare,
        // because it was treated as a return type that could not be mapped.
        match self {
            Type::Unspecified(unspecified) => {
                unspecified.text.is_empty() || unspecified.text == "void"
            }
            _ => false,
        }
    }

    /// Returns `true` if this is a `const`-qualified value type.
    #[must_use]
    pub fn is_const_value(&self) -> bool {
        matches!(self, Type::Const(_))
    }

    /// Returns `true` if this is an array of known length.
    #[must_use]
    pub fn is_array(&self) -> bool {
        matches!(self, Type::Array { .. })
    }

    /// Returns `true` if values of this type cannot be written through an
    /// indirection.
    ///
    /// A pointer to a const pointee and a reference to a const referent both
    /// report `true`; the pointer's own constness is a separate question
    /// answered by [`Type::pointer_is_const`].
    #[must_use]
    pub fn pointee_is_const(&self) -> bool {
        match self {
            Type::Pointer {
                pointee_is_const, ..
            } => *pointee_is_const,
            Type::Reference {
                referent_is_const, ..
            } => *referent_is_const,
            _ => false,
        }
    }

    /// Returns the referent of a reference, if this is one.
    ///
    /// A reference reads as its referent, so an operation on one is an
    /// operation on the value. What the reference adds is that it names the
    /// storage rather than holding a copy, which is what assignment uses.
    #[must_use]
    pub fn referent(&self) -> Option<&Type> {
        match self {
            Type::Reference { referent, .. } => Some(referent),
            _ => None,
        }
    }

    /// Returns `true` if this pointer may not be reassigned.
    ///
    /// Only a `const *T` reports `true`; a reference is never rebindable, so
    /// for a reference this is also `true`.
    #[must_use]
    pub fn pointer_is_const(&self) -> bool {
        match self {
            Type::Pointer {
                pointer_is_const, ..
            } => *pointer_is_const,
            Type::Reference { .. } => true,
            _ => false,
        }
    }

    /// Returns the element type of an array.
    #[must_use]
    pub fn array_element(&self) -> Option<&Type> {
        match self {
            Type::Array { element, .. } | Type::UnsizedArray(element) => Some(element),
            _ => None,
        }
    }

    /// Returns the array length, if it is known.
    #[must_use]
    pub fn array_len(&self) -> Option<u64> {
        match self {
            Type::Array { len, .. } => Some(*len),
            _ => None,
        }
    }

    /// Returns the type reached by following one level of indirection.
    #[must_use]
    pub fn pointee(&self) -> Option<&Type> {
        match self {
            Type::Pointer { pointee, .. } => Some(pointee),
            Type::Reference { referent, .. } => Some(referent),
            _ => None,
        }
    }

    /// Returns what a *pointer* points at, and `None` for anything else.
    ///
    /// `pointee` answers the same question for a reference too, which is right
    /// for "what does this name" and wrong for `*`: a reference is already the
    /// value it stands for, so there is nothing to reach through it. `*`
    /// follows a pointer, and `&x` is what makes one.
    #[must_use]
    pub fn pointer_pointee(&self) -> Option<&Type> {
        match self {
            Type::Pointer { pointee, .. } => Some(pointee),
            _ => None,
        }
    }

    /// Returns `true` if this may appear on either side of an explicit `as`.
    ///
    /// Numeric types, `byte`, and the character types convert among one
    /// another, wrapping to the target's width. `bool` converts to an integer,
    /// `nullptr` to any indirection, and a pointer to an array to a pointer to
    /// its element. Between indirections the shape must match modulo `const`,
    /// which either direction may change, and `*T`/`*byte` convert in both
    /// directions with const only ever accumulating.
    ///
    /// A pointer and an integer do not convert: there is no `as` between them.
    #[must_use]
    pub fn is_castable_to(&self, target: &Type) -> bool {
        if self == target {
            return true;
        }
        // A `const` on the value describes what may be written, not what may be
        // read, so a cast converts what is inside it -- in either direction.
        if let Type::Const(inner) = self {
            return inner.is_castable_to(target);
        }
        if let Type::Const(inner) = target {
            return self.is_castable_to(inner);
        }
        // `bool` promotes to an integer as 1 or 0, so it may be cast to any
        // integer type. Nothing else about `bool` is convertible.
        if self.is_bool() && target.is_integer() {
            return true;
        }
        // `nullptr` converts to any indirection, which is the whole reason it
        // is a type of its own: it is the null value of a pointer without
        // being the null value of one particular pointer type.
        if self.is_null_pointer() && target.is_indirection() {
            return true;
        }
        if self.is_convertible() && target.is_convertible() {
            return true;
        }
        // Changing an indirection's `const` qualifiers, in either direction:
        // adding const is also legal implicitly, but dropping it needs the
        // explicit `as` that reaches this function. The pointee's structure
        // must still line up, which `same_shape_ignoring_const` checks.
        if self.is_indirection() && target.is_indirection() && same_shape_ignoring_const(self, target) {
            return true;
        }
        // A byte pointer converts to and from a pointer to anything, and `as`
        // is how the const-dropping direction of that is written -- so the
        // check here is the byte-pointer one without its `const` rule.
        if is_byte_pointer_cast(self, target) {
            return true;
        }
        // A pointer to an array is a pointer to its first element, which is
        // what makes `&buf as *const c8` work for `buf [64]c8`: C spells the
        // same decay by writing the array where a pointer is wanted, and an
        // explicit `as` is how this language says it.
        if let (
            Type::Pointer { pointee, .. },
            Type::Pointer {
                pointee: target_pointee,
                ..
            },
        ) = (self, target)
            && let Type::Array { element, .. } = pointee.unqualified()
            && element
                .unqualified()
                .is_castable_to(&target_pointee.unqualified().clone())
        {
            return true;
        }
        false
    }

    /// Returns `true` if this type may be compared with `other`.
    ///
    /// # The rule
    ///
    /// Comparison is the only operation that mixes arithmetic types. Two
    /// arithmetic types may always be compared, at any width and signedness,
    /// and a pointer or reference may be compared with one of the same type.
    ///
    /// Comparing by *value* rather than by representation is what makes the
    /// mixed cases well behaved: `-1` as an `i8` is less than `1` as a `u32`,
    /// even though the same bits reinterpreted would say otherwise. The
    /// lowering is responsible for getting that right.
    #[must_use]
    pub fn is_comparable_with(&self, other: &Type) -> bool {
        // Two arithmetic types, of any widths or signedness, including floats
        // against integers.
        if self.is_arithmetic_for_comparison() && other.is_arithmetic_for_comparison() {
            return true;
        }
        // Otherwise only an exact match, and only for a type the compiler
        // knows how to compare: a number, a character, `bool`, a pointer, or a
        // reference. A declared type compares by defining `__eq__` and
        // `__ne__` -- there is no member-wise comparison -- and accepting an
        // exact match for one made a program type-check that code generation
        // could not emit, which is how a trait could claim a type it could not
        // be used with.
        self == other && self.is_comparable_whole()
    }

    /// Returns `true` if two values of this type compare without a method.
    ///
    /// The scalar kinds, which the compiler has a comparison instruction for.
    /// An aggregate does not: `Point == Point` is what `__eq__` is for.
    #[must_use]
    pub fn is_comparable_whole(&self) -> bool {
        match self {
            Type::Int(_) | Type::Byte | Type::Bool | Type::Character(_) => true,
            Type::Float(_) => true,
            Type::Pointer { .. } | Type::Reference { .. } => true,
            Type::Const(inner) | Type::Volatile(inner) => inner.is_comparable_whole(),
            _ => false,
        }
    }

    /// Returns `true` if this participates in mixed-type comparison.
    ///
    /// `byte` counts, like any other integer-like type.
    #[must_use]
    pub fn is_arithmetic_for_comparison(&self) -> bool {
        matches!(self, Type::Int(_) | Type::Byte | Type::Float(_))
    }

    /// Returns `true` if this value may be tested for truthiness directly.
    ///
    /// Truthiness is one of a short list of implicit *adjustments* -- with
    /// array decay, taking a value's address for a reference parameter, adding
    /// `const`, and a literal taking its context's type. It is not a general
    /// conversion: no value changes type here, and the operand is tested as
    /// what it is. The rule is that a numeric value is true when it is
    /// non-zero, and a `bool` is itself.
    ///
    /// `byte` counts as numeric for this purpose even though it takes no part
    /// in arithmetic, because a zero byte is naturally false.
    #[must_use]
    pub fn is_truth_testable(&self) -> bool {
        self.is_numeric()
            || self.is_byte()
            || self.is_bool()
            || self.is_char()
            || self.is_indirection()
            // An array is always true: it is viewed as a pointer, and the
            // address of an array is never null. Testing an array is a way to
            // write "the container exists" rather than "it has elements".
            //
            // A function is always true for the same reason: it is code, and
            // there is no such thing as an absent function -- a name that
            // denotes one cannot be null, only a *pointer* to one can.
            || matches!(
                self.unqualified(),
                Type::Array { .. } | Type::Function { .. }
            )
    }

    /// Returns the function type a callable value has, if it has one.
    ///
    /// Two shapes are callable: a value of function type, and a pointer to one.
    /// A function *name* used as a value decays to the pointer, so both are the
    /// ordinary way to call code the program only has an address for.
    #[must_use]
    pub fn function_type(&self) -> Option<&Type> {
        match self.unqualified() {
            callable @ Type::Function { .. } => Some(callable),
            Type::Pointer { pointee, .. } => match pointee.unqualified() {
                callable @ Type::Function { .. } => Some(callable),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns `true` if this is a floating-point type.
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float(_))
    }

    /// Returns `true` if this participates in arithmetic.
    ///
    /// `byte` does **not**: it is not an integer type, so it takes no part in
    /// integer arithmetic or implicit conversion. It can still be the operand
    /// or target of an explicit `as`; see `TypeChecker::is_castable`.
    #[must_use]
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Returns `true` if this type is signed, for choosing a conversion
    /// instruction.
    ///
    /// `byte` is treated as unsigned, matching the `u8` it is based on. Floats
    /// and unspecified types report `false`; only integer signedness matters to
    /// the callers.
    #[must_use]
    pub fn is_signed_for_conversion(&self) -> bool {
        match self {
            Type::Int(int) => !int.is_unsigned(),
            _ => false,
        }
    }

    /// Returns `true` if this can appear on either side of an explicit `as`.
    ///
    /// Wider than [`Type::is_numeric`]: `byte` counts here even though it does
    /// not take part in arithmetic.
    #[must_use]
    pub fn is_convertible(&self) -> bool {
        self.is_numeric() || self.is_byte() || self.is_char()
    }

    /// Renders the type canonically, with aliases resolved.
    ///
    /// `int` prints as `i32` because that is what it *is*; a diagnostic that
    /// wants to echo the user's spelling must keep the source text separately.
    /// `byte` prints as `byte`, because it is not an alias for anything.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Type::Int(int) => int.spelling().to_owned(),
            Type::Byte => "byte".to_owned(),
            Type::Nominal(nominal) => nominal.name.clone(),
            Type::Character(character) => character.spelling().to_owned(),
            Type::Bool => "bool".to_owned(),
            Type::String => "String".to_owned(),
            Type::NullPointer => "nullptr".to_owned(),
            Type::Pointer {
                pointee,
                pointee_is_const,
                pointer_is_const,
            } => {
                // Printed the way it is written, so `*const T` and `const *T`
                // do not look alike in a diagnostic.
                let inner = if *pointee_is_const {
                    format!("*const {}", pointee.spelling())
                } else {
                    format!("*{}", pointee.spelling())
                };
                if *pointer_is_const {
                    format!("const {inner}")
                } else {
                    inner
                }
            }
            Type::Reference {
                referent,
                referent_is_const,
                is_rvalue,
            } => {
                let mark = if *is_rvalue { "&&" } else { "&" };
                if *referent_is_const {
                    format!("{mark}const {}", referent.spelling())
                } else {
                    format!("{mark}{}", referent.spelling())
                }
            }
            Type::Const(inner) => format!("const {}", inner.spelling()),
            Type::Volatile(inner) => format!("volatile {}", inner.spelling()),
            Type::Array { element, len } => format!("[{len}]{}", element.spelling()),
            Type::UnsizedArray(element) => format!("[]{}", element.spelling()),
            Type::Function {
                parameters,
                returns,
                abi,
                variadic,
                throws,
            } => {
                let mut written = String::from("fn");
                if let Some(abi) = abi {
                    written.push_str(&format!(" \"{abi}\""));
                }
                let listed: Vec<String> =
                    parameters.iter().map(|ty| ty.spelling()).collect();
                written.push('(');
                written.push_str(&listed.join(", "));
                if *variadic {
                    if !parameters.is_empty() {
                        written.push_str(", ");
                    }
                    written.push_str("...");
                }
                written.push_str(") ");
                written.push_str(&returns.spelling());
                if *throws {
                    written.push_str(" except");
                }
                written
            }
            Type::Float(float) => float.spelling().to_owned(),
            // Nothing written is `void`: a function that returns no value is
            // declared without a type, and a diagnostic about it must say
            // `void` rather than the name of the compiler's placeholder.
            Type::Unspecified(u) if u.text.is_empty() => "void".to_owned(),
            Type::Unspecified(u) => u.text.clone(),
        }
    }

    /// Returns the width in bits of a type with a fixed layout, if it has one.
    ///
    /// `byte` is eight bits, like the `u8` it is based on.
    #[must_use]
    pub const fn bits(&self) -> Option<u32> {
        match self {
            Type::Int(int) => Some(int.bits()),
            Type::Byte => Some(8),
            Type::Nominal(_) => None,
            Type::Character(character) => Some(character.bits()),
            Type::Bool => Some(1),
            // A null pointer has the layout of a pointer, but it is not a
            // value of any particular pointer type, so its "width" is the
            // target's rather than this type's.
            Type::NullPointer => None,
            // A string's representation is the standard library's business.
            Type::String => None,
            Type::Float(float) => Some(float.bits()),
            // `volatile T` has exactly `T`'s layout: it says how the memory is
            // accessed, not how it is arranged.
            Type::Volatile(inner) => inner.bits(),
            // A pointer's width is the target's. The compiler targets the host
            // only, and `usize` is `u64`, so a pointer is 64 bits.
            //
            // A function type has no layout at all: a function is code, and the
            // only thing holding it is a pointer.
            Type::Pointer { .. } | Type::Reference { .. } | Type::Function { .. } => None,
            // An array's width is its element's times its length, which the
            // caller can compute from `array_element` and `array_len`.
            Type::Const(inner) => inner.bits(),
            Type::Array { .. } | Type::UnsizedArray(_) => None,
            Type::Unspecified(_) => None,
        }
    }
}

/// Maps each local binding to the type it was declared with.
///
/// Keyed by the binding's name span, which is unique per declaration and
/// stable across passes: the resolver binds names by that same span.
#[derive(Clone, Debug, Default)]
pub struct BindingTypes {
    entries: Vec<(Span, Type)>,
}

impl BindingTypes {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        BindingTypes::default()
    }

    /// Records the type of the binding declared at `name_span`.
    pub fn insert(&mut self, name_span: Span, ty: Type) {
        if let Some(slot) = self.entries.iter_mut().find(|(span, _)| *span == name_span) {
            slot.1 = ty;
        } else {
            self.entries.push((name_span, ty));
        }
    }

    /// Forgets the binding declared at `name_span`, if one was recorded.
    ///
    /// Used by a *probe*: a trait's parameters stand for the types being asked
    /// about while the requirements are checked, and must go back to having no
    /// type of their own afterwards.
    pub fn remove(&mut self, name_span: Span) {
        self.entries.retain(|(span, _)| *span != name_span);
    }

    /// Returns the type of the binding declared at `name_span`.
    #[must_use]
    pub fn get(&self, name_span: Span) -> Option<&Type> {
        self.entries
            .iter()
            .find(|(span, _)| *span == name_span)
            .map(|(_, ty)| ty)
    }

    /// Returns every recorded binding, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (Span, &Type)> + '_ {
        self.entries.iter().map(|(span, ty)| (*span, ty))
    }

    /// Returns the number of recorded bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Returns `true` if two indirection types match apart from their `const`
/// qualifiers.
///
/// Used by explicit casts, which may change qualifiers but not the shape of the
/// type: `*const *T` casts to `**T`, but not to `*T`.
#[must_use]
pub fn same_shape_ignoring_const(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (
            Type::Pointer {
                pointee: a_pointee, ..
            },
            Type::Pointer {
                pointee: b_pointee, ..
            },
        ) => same_shape_ignoring_const(a_pointee, b_pointee),
        (
            Type::Reference {
                referent: a_referent,
                ..
            },
            Type::Reference {
                referent: b_referent,
                ..
            },
        ) => same_shape_ignoring_const(a_referent, b_referent),
        // A pointer and a reference are different indirections, so a cast
        // between them is not a qualifier change.
        (Type::Pointer { .. }, Type::Reference { .. })
        | (Type::Reference { .. }, Type::Pointer { .. }) => false,
        (a, b) => a == b,
    }
}

/// Renders a type as a symbol-safe string that no other type can produce.
///
/// # Why not `spelling`
///
/// A printed type is ambiguous once punctuation is removed. `*const Box` and
/// `const *Box` are different types built from the same words, and a linker
/// symbol cannot keep the `*` that tells them apart. Two overloads differing
/// only there would share a symbol and one would silently replace the other --
/// which is exactly what happened with `__as__`, where the second definition
/// overwrote the first and LLVM then rejected the module.
///
/// # The encoding
///
/// Structural, so that distinct types give distinct strings. Every piece that
/// can vary in length carries its length, so that no two encodings can be
/// split differently and read as the same string:
///
/// | Type | Encoding |
/// | ---- | -------- |
/// | `i32` | `I` + name length + name |
/// | `c8`, `float` | likewise, `C` and `F` |
/// | `byte`, `bool` | `B` + name length + name |
/// | `Box` | `N` + name length + name |
/// | `*T` | `P` + pointee-const + pointer-const + pointee |
/// | `&T` | `R` + referent-const + referent |
/// | `[N]T` | `A` + length + `_` + element |
/// | `[]T` | `U` + element |
/// | `const T` | `K` + type |
///
/// The two `const` bits of a pointer are encoded separately and in a fixed
/// order, so all four combinations are distinct. A length is always followed by
/// `_`, which is how the length is told from the text it measures.
#[must_use]
pub fn mangle_type(ty: &Type) -> String {
    /// A tag, then a length, then the text it measures.
    fn tagged(tag: char, text: &str) -> String {
        format!("{tag}{}_", text.len()) + text
    }

    match ty {
        Type::Int(int) => tagged('I', int.spelling()),
        Type::Character(character) => tagged('C', character.spelling()),
        Type::Float(float) => tagged('F', float.spelling()),
        Type::Byte => tagged('B', "byte"),
        Type::Bool => tagged('B', "bool"),
        Type::NullPointer => tagged('B', "nullptr"),
        Type::String => tagged('B', "string"),
        Type::Nominal(nominal) => tagged('N', &nominal.name),
        // `volatile T` is a different type from `T`, so it mangles
        // differently: a function taking one is not a function taking the
        // other, and they must not share a symbol.
        Type::Volatile(inner) => format!("V{}", mangle_type(inner)),
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => format!(
            "P{}{}{}",
            u8::from(*pointee_is_const),
            u8::from(*pointer_is_const),
            mangle_type(pointee)
        ),
        // The two reference kinds are different types, so they have to mangle
        // differently: `T(T&)` and `T(T&&)` are different functions, and giving
        // them one symbol would make the second definition silently replace the
        // first.
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => format!(
            "{}{}{}",
            if *is_rvalue { "W" } else { "R" },
            u8::from(*referent_is_const),
            mangle_type(referent)
        ),
        // The length is followed by `_` so that `A12_..` cannot be read as
        // `A1` followed by `2_..`.
        Type::Array { element, len } => format!("A{len}_{}", mangle_type(element)),
        Type::UnsizedArray(element) => format!("U{}", mangle_type(element)),
        Type::Const(inner) => format!("K{}", mangle_type(inner)),
        // A function type in a signature, as in `*fn(int) int`: the parameters
        // and the return type are all part of what makes two of them different,
        // and so is the ABI.
        Type::Function {
            parameters,
            returns,
            abi,
            variadic,
            throws,
        } => {
            let listed: Vec<String> = parameters.iter().map(mangle_type).collect();
            format!(
                "G{}_{}_{}{}_{}",
                abi.as_deref().unwrap_or(""),
                u8::from(*variadic),
                u8::from(*throws),
                listed.join(""),
                mangle_type(returns)
            )
        }
        // Not writable by a program, so it cannot appear in a signature; given
        // an encoding so that this function is total and cannot silently
        // produce an empty string for a type it does not know.
        Type::Unspecified(unspecified) => tagged('X', &unspecified.text),
    }
}

/// A cast the checker resolved to a type's own conversion method.
///
/// Carries the symbol to call as well as the signature, so code generation does
/// not have to reconstruct the mangling. It cannot: the mangling is structural
/// and depends on resolved types, and a second implementation of it would be a
/// second chance to disagree with the first.
///
/// `frac as double` becomes `__as__(&frac, nullptr)` -- the receiver by
/// address, and a null pointer whose *type* is the target. The selector carries
/// no value; it exists so that a conversion is selected by an argument rather
/// than by its return type, which would not distinguish two conversions of one
/// receiver.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConversionMethod {
    /// The symbol the method is emitted under.
    pub symbol: String,
    /// The definition it was resolved from.
    ///
    /// Kept whole because the conversion is a call site: whether it may raise,
    /// and any `??(cond)` claim it makes, travel with the signature.
    pub signature: crate::FunctionSignature,
    /// What the method returns, which is what the cast produces.
    pub returns: Type,
    /// Whether the method takes a selector parameter.
    ///
    /// `__as__` does: the selector names the target, and the call passes a null
    /// pointer of the target's type. `__int__`, `__float__` and `__bytes__` do
    /// not, because their names already say the target, so the call passes the
    /// receiver alone.
    pub takes_selector: bool,
}

/// A nominal type, as declared by `type`.
#[derive(Clone, Debug)]
pub struct NominalType {
    /// The declared name.
    pub name: String,
    /// The fields, in declaration order.
    ///
    /// Order is part of the type: it is what a positional initialiser
    /// `.{42, 37}` assigns by.
    pub fields: Vec<(String, Type)>,
    /// The names of the fields marked `mutable`, in the same order as
    /// `fields` filters them.
    ///
    /// Kept beside the fields rather than inside the pairs so that every
    /// existing reader of `fields` -- layout, initialisers, mangling -- keeps
    /// working unchanged. A field's mutability is a property of the
    /// *declaration*, not of its type, which is why it is a separate fact.
    pub mutable_fields: Vec<String>,
    /// The alignment each field was given with `alignas`, by field name.
    pub field_alignments: Vec<(String, u64)>,
    /// Whether the fields share one address.
    ///
    /// A `union`'s members are all at offset zero, so the type is as large as
    /// its largest member. The fields are still listed in declaration order --
    /// that is what an initialiser assigns by -- but the *offsets* are all the
    /// same, which is the whole difference from a struct.
    pub is_union: bool,
    /// Whether the type has been declared but not defined.
    ///
    /// `type Foo;` says the name exists and nothing else. An incomplete type has
    /// no layout and no fields, so a value of it cannot be stored, copied,
    /// measured, or asked for a member: only a pointer or a reference to it may
    /// be written. The definition completes it.
    pub is_incomplete: bool,
    /// Which field a union's storage is represented by, as an index.
    ///
    /// The largest one, so that every member fits in the space. Decided by the
    /// checker, which knows each field's size; the code generator has only the
    /// LLVM types and no target data to measure them with.
    pub union_storage: Option<usize>,
    /// The family this type is a member of, and the arguments that made it.
    ///
    /// A member of a family is an ordinary nominal type, but its methods are
    /// written against the *family's* parameters: `Device<Mode>` has a `write`
    /// whose receiver is `&Device<Mode>`. Which member a value is decides what
    /// those parameters mean, and the answer has to travel with the type --
    /// across a module boundary too, where the importer never resolved the
    /// written arguments and so has no other record of them.
    pub template_arguments: Option<(String, crate::template::Arguments)>,
}

/// Two nominal types are the same type when they have the same name.
///
/// The *fields* are a property of the name, not part of its identity, and
/// treating them as part of it made a type unable to mention itself: a type's
/// name has to be registered before its fields are resolved, so that `next
/// *Node` inside `Node` can name it -- and the shallow entry that resolves is
/// the same type as the complete one, however few fields it carries yet.
impl PartialEq for NominalType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for NominalType {}

impl NominalType {
    /// Returns the index and type of the field called `name`.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(usize, &Type)> {
        let (index, ty) = self
            .fields
            .iter()
            .enumerate()
            .find(|(_, (field, _))| field == name)
            .map(|(index, (_, ty))| (index, ty))?;
        // Every member of a union lives at offset zero, so every member is
        // reached through the same slot. Returning the declared index instead
        // would name a member that the representation does not have -- the
        // storage is one member wide, and the index for anything but the first
        // was out of range.
        //
        // The *declared* index is still what `fields` is ordered by, which is
        // what an initialiser assigns by; only reaching a field is redirected.
        Some((if self.is_union { 0 } else { index }, ty))
    }

    /// Returns the declared position of a field, for an initialiser.
    ///
    /// A positional initialiser `.{1, 2}` assigns by *declaration* order, which
    /// for a union is the order the members were written rather than the one
    /// slot they share. Writing a union positionally sets the first member,
    /// which is the conservative reading: the others overlap it.
    #[must_use]
    pub fn declared_field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(field, _)| field == name)
    }
}

/// A type annotation preserved as raw text.
///
/// Used for every type name that is not a recognised primitive, including
/// user-defined types and qualified paths such as `std.Exception`.
#[derive(Clone, Debug)]
pub struct UnspecifiedType {
    /// The annotation exactly as written.
    pub text: String,
    /// Where it was written.
    pub span: Span,
}

// Two annotations naming the same thing are the same type, wherever they were
// written. The span is where a diagnostic points, not part of the name: a
// template parameter `Cmp` written in a helper's parameter list and the same
// `Cmp` written in its body are one type, and comparing the spans made them
// two -- so `cmp Cmp = order;` reported a mismatch between `Cmp` and `Cmp`.
impl PartialEq for UnspecifiedType {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for UnspecifiedType {}

/// Maps each expression to the type computed for it.
///
/// Kept as a side table rather than a field on the AST node: the AST is the
/// parser's output and should not be mutated by type checking, and a side
/// table makes it obvious which passes are allowed to read types.
#[derive(Clone, Debug, Default)]
pub struct TypeTable {
    entries: Vec<(ExprId, Type)>,
}

impl TypeTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        TypeTable::default()
    }

    /// Records the type of an expression, replacing any previous entry.
    pub fn insert(&mut self, expr: ExprId, ty: Type) {
        if let Some(slot) = self.entries.iter_mut().find(|(id, _)| *id == expr) {
            slot.1 = ty;
        } else {
            self.entries.push((expr, ty));
        }
    }

    /// Returns the type recorded for `expr`.
    #[must_use]
    pub fn get(&self, expr: ExprId) -> Option<&Type> {
        self.entries
            .iter()
            .find(|(id, _)| *id == expr)
            .map(|(_, ty)| ty)
    }

    /// Returns the number of recorded types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates over every recorded entry, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (ExprId, &Type)> + '_ {
        self.entries.iter().map(|(id, ty)| (*id, ty))
    }

    /// Returns `(span, type)` pairs for display, skipping entries whose type
    /// has no source location.
    ///
    /// Used by the driver's `--emit=types`; kept here rather than in the
    /// driver so that the driver does not need access to the private storage.
    #[must_use]
    pub fn entries_for_display(&self) -> Vec<(Span, String)> {
        self.entries
            .iter()
            .filter_map(|(_, ty)| ty.span().map(|span| (span, format!("{ty:?}"))))
            .collect()
    }
}

#[cfg(test)]
mod mangling_tests {
    use super::{NominalType, Type, mangle_type};

    fn nominal(name: &str) -> Type {
        Type::Nominal(NominalType {
            mutable_fields: Vec::new(),
            field_alignments: Vec::new(),
            is_union: false,
            is_incomplete: false,
            union_storage: None,
            template_arguments: None,
            name: name.to_owned(),
            fields: Vec::new(),
        })
    }

    /// Every type in a set of deliberately confusable types.
    fn confusable() -> Vec<(&'static str, Type)> {
        let box_ty = nominal("Box");
        vec![
            ("int", Type::Int(super::IntType::I32)),
            ("u8", Type::Int(super::IntType::U8)),
            ("bool", Type::Bool),
            ("byte", Type::Byte),
            ("Box", box_ty.clone()),
            ("*Box", Type::Pointer {
                pointee: Box::new(box_ty.clone()),
                pointee_is_const: false,
                pointer_is_const: false,
            }),
            // The four const combinations of one pointer type. These are the
            // ones a printed form cannot tell apart.
            ("*const Box", Type::Pointer {
                pointee: Box::new(box_ty.clone()),
                pointee_is_const: true,
                pointer_is_const: false,
            }),
            ("const *Box", Type::Pointer {
                pointee: Box::new(box_ty.clone()),
                pointee_is_const: false,
                pointer_is_const: true,
            }),
            ("const *const Box", Type::Pointer {
                pointee: Box::new(box_ty.clone()),
                pointee_is_const: true,
                pointer_is_const: true,
            }),
            ("**Box", Type::Pointer {
                pointee: Box::new(Type::Pointer {
                    pointee: Box::new(box_ty.clone()),
                    pointee_is_const: false,
                    pointer_is_const: false,
                }),
                pointee_is_const: false,
                pointer_is_const: false,
            }),
            ("&Box", Type::Reference {
                is_rvalue: false,
                referent: Box::new(box_ty.clone()),
                referent_is_const: false,
            }),
            ("&const Box", Type::Reference {
                is_rvalue: false,
                referent: Box::new(box_ty.clone()),
                referent_is_const: true,
            }),
            ("[3]Box", Type::Array {
                element: Box::new(box_ty.clone()),
                len: 3,
            }),
            // A length and an element that could run together if the length
            // were not delimited.
            ("[1]Box", Type::Array {
                element: Box::new(box_ty.clone()),
                len: 1,
            }),
            ("[]Box", Type::UnsizedArray(Box::new(box_ty.clone()))),
            ("const Box", Type::Const(Box::new(box_ty.clone()))),
            // A type whose *name* looks like an encoding, which is what the
            // length prefixes exist to keep apart.
            ("P10N3Box", nominal("P10N3Box")),
            ("Box_i32", nominal("Box_i32")),
        ]
    }

    #[test]
    fn distinct_types_get_distinct_symbols() {
        // The property that matters. Two types sharing an encoding means two
        // overloads sharing a symbol, and the second definition silently
        // replaces the first -- which is not a diagnostic anywhere, just the
        // wrong function being called.
        let types = confusable();
        for (index, (name, ty)) in types.iter().enumerate() {
            for (other_name, other) in &types[index + 1..] {
                assert_ne!(
                    mangle_type(ty),
                    mangle_type(other),
                    "`{name}` and `{other_name}` share the encoding `{}`",
                    mangle_type(ty)
                );
            }
        }
    }

    #[test]
    fn a_pointer_keeps_both_of_its_const_bits() {
        // The case that started this: `*const Box` and `const *Box` are
        // different types, and a printer cannot tell them apart.
        let box_ty = nominal("Box");
        let pointer = |pointee_is_const, pointer_is_const| Type::Pointer {
            pointee: Box::new(box_ty.clone()),
            pointee_is_const,
            pointer_is_const,
        };
        let bare = mangle_type(&pointer(false, false));
        let pointee = mangle_type(&pointer(true, false));
        let itself = mangle_type(&pointer(false, true));
        let both = mangle_type(&pointer(true, true));
        assert_ne!(bare, pointee);
        assert_ne!(bare, itself);
        assert_ne!(pointee, itself);
        assert_ne!(both, pointee);
        assert_ne!(both, itself);
    }

    #[test]
    fn a_name_cannot_imitate_structure() {
        // A type named `P10N3Box` must not encode like `*const Box`, or the
        // length prefixes are not doing their job.
        let box_ty = nominal("Box");
        let pointer = Type::Pointer {
            pointee: Box::new(box_ty),
            pointee_is_const: true,
            pointer_is_const: false,
        };
        assert_ne!(mangle_type(&pointer), mangle_type(&nominal("P10N3Box")));
    }

    #[test]
    fn a_length_cannot_run_into_its_element() {
        // `A12_` reads as length 12; without the `_`, `A12N...` could be read
        // as length 1 followed by a type whose encoding starts `2N`.
        let element = nominal("Box");
        let twelve = Type::Array {
            element: Box::new(element.clone()),
            len: 12,
        };
        let one = Type::Array {
            element: Box::new(element),
            len: 1,
        };
        assert_ne!(mangle_type(&twelve), mangle_type(&one));
        assert!(mangle_type(&twelve).starts_with("A12_"));
        assert!(mangle_type(&one).starts_with("A1_"));
    }

    #[test]
    fn the_encoding_is_safe_for_a_symbol() {
        // It ends up in a linker symbol, so it must contain nothing that
        // confuses an assembler.
        for (_, ty) in confusable() {
            let mangled = mangle_type(&ty);
            assert!(
                mangled
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "`{mangled}` is not symbol-safe"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_table_round_trips() {
        let mut table = TypeTable::new();
        let id = ExprId::from_raw(0);
        table.insert(id, Type::unspecified("i32", Span::SYNTHETIC));
        assert_eq!(table.len(), 1);
        assert!(table.get(id).is_some_and(Type::is_unspecified));
        assert!(table.get(ExprId::from_raw(1)).is_none());
    }

    #[test]
    fn type_table_insert_overwrites() {
        let mut table = TypeTable::new();
        let id = ExprId::from_raw(0);
        table.insert(id, Type::unspecified("a", Span::SYNTHETIC));
        table.insert(id, Type::unspecified("b", Span::SYNTHETIC));
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(id).and_then(Type::span), Some(Span::SYNTHETIC));
    }
}

#[cfg(test)]
mod primitive_tests {
    use super::*;

    #[test]
    fn every_integer_name_resolves() {
        for (name, bits, unsigned) in [
            ("i8", 8, false),
            ("i16", 16, false),
            ("i32", 32, false),
            ("i64", 64, false),
            ("i128", 128, false),
            ("u8", 8, true),
            ("u16", 16, true),
            ("u32", 32, true),
            ("u64", 64, true),
            ("u128", 128, true),
        ] {
            let ty = Type::from_name(name).unwrap_or_else(|| panic!("`{name}` must resolve"));
            match ty {
                Type::Int(int) => {
                    assert_eq!(int.bits(), bits, "{name} width");
                    assert_eq!(int.is_unsigned(), unsigned, "{name} signedness");
                }
                other => panic!("`{name}` resolved to {other:?}"),
            }
        }
    }

    #[test]
    fn aliases_resolve_to_their_targets() {
        // int = i32, uint = u32, isize = i64, usize = u64.
        assert_eq!(Type::from_name("int"), Some(Type::Int(IntType::I32)));
        assert_eq!(Type::from_name("uint"), Some(Type::Int(IntType::U32)));
        assert_eq!(Type::from_name("isize"), Some(Type::Int(IntType::I64)));
        assert_eq!(Type::from_name("usize"), Some(Type::Int(IntType::U64)));
    }

    #[test]
    fn an_alias_and_its_target_are_the_same_type() {
        // This is what makes `int` and `i32` interchangeable in every pass.
        assert_eq!(Type::from_name("int"), Type::from_name("i32"));
        assert_eq!(Type::from_name("uint"), Type::from_name("u32"));
        assert_eq!(Type::from_name("isize"), Type::from_name("i64"));
        assert_eq!(Type::from_name("usize"), Type::from_name("u64"));
    }

    #[test]
    fn character_types_are_distinct_from_the_integers_they_sit_on() {
        // The point of the distinction, as with `byte`: a character is not a
        // number, so it takes no part in arithmetic.
        for (name, bits, base) in [("c8", 8, Type::C8), ("c16", 16, Type::C16), ("c32", 32, Type::C32)] {
            let character = Type::from_name(name).unwrap_or_else(|| panic!("`{name}` resolves"));
            assert_eq!(character, base);
            assert!(!character.is_integer());
            assert!(character.is_char());
            assert_eq!(character.bits(), Some(bits));
            assert_eq!(character.spelling(), name);
        }
        // And none of them is the integer of the same width.
        assert_ne!(Type::C8, Type::from_name("u8").expect("`u8` resolves"));
        assert_ne!(Type::C16, Type::from_name("u16").expect("`u16` resolves"));
        assert_ne!(Type::C32, Type::from_name("u32").expect("`u32` resolves"));
    }

    #[test]
    fn char_is_an_alias_for_c8() {
        // `char` is the spelling to reach for by default; `c16` and `c32` are
        // for text that needs more than a byte per code unit.
        assert_eq!(Type::from_name("char"), Some(Type::C8));
    }

    #[test]
    fn byte_is_based_on_u8_but_is_a_distinct_type() {
        let byte = Type::from_name("byte").expect("`byte` resolves");
        assert_eq!(byte, Type::Byte);
        // The point of the distinction: these must not be equal, or `byte`
        // would silently be `u8` and no explicit conversion would be needed.
        assert_ne!(byte, Type::from_name("u8").expect("`u8` resolves"));
        assert!(!byte.is_integer());
        assert!(byte.is_byte());
        // It still has u8's width.
        assert_eq!(byte.bits(), Some(8));
        // And it prints as itself, not as its representation.
        assert_eq!(byte.spelling(), "byte");
    }

    #[test]
    fn floats_resolve_with_their_widths() {
        // `f32` and `f64` are the names the types have; `float` and `double`
        // are aliases, and both spellings resolve to the same type.
        for name in ["f32", "float"] {
            match Type::from_name(name) {
                Some(Type::Float(f)) => {
                    assert_eq!(f.bits(), 32);
                    // The spelling is the real name even when the alias was
                    // written, so a diagnostic says `f32` either way.
                    assert_eq!(f.spelling(), "f32");
                }
                other => panic!("`{name}` resolved to {other:?}"),
            }
        }
        for name in ["f64", "double"] {
            match Type::from_name(name) {
                Some(Type::Float(f)) => {
                    assert_eq!(f.bits(), 64);
                    assert_eq!(f.spelling(), "f64");
                }
                other => panic!("`{name}` resolved to {other:?}"),
            }
        }
    }

    #[test]
    fn unknown_names_do_not_resolve() {
        // A user-defined type is not a primitive, and must not be invented.
        assert_eq!(Type::from_name("MyStruct"), None);
        assert_eq!(Type::from_name("std.Exception"), None);
        assert_eq!(Type::from_name(""), None);
        // `char` exists, so it does resolve now.
        assert_eq!(Type::from_name("char"), Some(Type::C8));
    }

    #[test]
    fn bool_is_a_distinct_type() {
        let boolean = Type::from_name("bool").expect("`bool` resolves");
        assert_eq!(boolean, Type::Bool);
        assert!(boolean.is_bool());
        assert!(!boolean.is_integer());
        assert!(!boolean.is_byte());
        assert_eq!(boolean.spelling(), "bool");
    }

    #[test]
    fn only_numbers_and_bool_have_truthiness() {
        // Truthiness is the language's only implicit conversion, so its domain
        // is worth pinning down exactly.
        assert!(Type::INT.is_truth_testable());
        assert!(Type::BYTE.is_truth_testable());
        assert!(Type::BOOL.is_truth_testable());
        assert!(
            Type::from_name("float")
                .expect("float resolves")
                .is_truth_testable()
        );
        // An unmodelled type has no truthiness rule.
        assert!(!Type::unspecified("Foo", Span::SYNTHETIC).is_truth_testable());
    }

    #[test]
    fn numeric_classification() {
        assert!(Type::INT.is_integer());
        assert!(Type::INT.is_numeric());
        assert!(!Type::INT.is_float());
        let single = Type::from_name("float").expect("float resolves");
        assert!(single.is_float());
        assert!(single.is_numeric());
        // `byte` is not an integer type, so it is not numeric either.
        assert!(!Type::BYTE.is_integer());
        assert!(!Type::BYTE.is_numeric());
    }
}
