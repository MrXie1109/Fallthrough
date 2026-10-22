//! The table of compiler-provided functions.
//!
//! A builtin is a function the language provides without a declaration in
//! source. The compiler knows its name and signature, so resolution can bind a
//! call to it and code generation can declare it in the LLVM module.
//!
//! Keeping this in one place means adding a builtin is a single table entry,
//! rather than a change in the resolver, the type checker, and the code
//! generator separately. Which is what went wrong first: `__builtin_panic` was
//! known only to codegen, so calling it from source failed to resolve.
//!
//! The table currently holds one entry, `__builtin_panic`, which stands in for
//! `std.panic`. A builtin is added only once the language fixes its behaviour,
//! so allocation, memory intrinsics, and type inspection have no entry.

/// A function provided by the compiler.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Builtin {
    /// The name as written in source.
    pub name: &'static str,
    /// The parameter types, in order.
    ///
    /// Written as a small fixed set rather than as [`crate::Type`] values so
    /// that the table is a `const` and cannot depend on the type system
    /// evolving.
    pub params: &'static [BuiltinType],
    /// The return type.
    pub ret: BuiltinType,
    /// What the builtin does, for diagnostics.
    pub doc: &'static str,
}

/// The type of a builtin's parameter or return value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinType {
    /// No value, as a function that only has effects.
    Void,
    /// A pointer to bytes, which is how a string literal is passed.
    Bytes,
    /// Any value at all.
    ///
    /// `str` and `repr` take whatever they are given and decide what to do by
    /// its type, so there is no type to name here.
    Any,
    /// The standard string type.
    ///
    /// Named rather than spelled as a pointer, because a `String` owns its
    /// bytes: the caller receives something it must destroy.
    Text,
    /// Any integer, whose width the caller picks.
    ///
    /// A builtin that formats a value cannot name one integer type, because it
    /// is given whatever the caller has.
    Integer,
}

impl BuiltinType {
    /// Renders the type for a diagnostic.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            BuiltinType::Void => "void",
            BuiltinType::Bytes => "String",
            BuiltinType::Integer => "int",
            BuiltinType::Any => "any",
            BuiltinType::Text => "std.String",
        }
    }
}

/// Every builtin the compiler provides.
///
/// Must stay sorted by name, because [`lookup`] binary-searches it.
pub static BUILTINS: &[Builtin] = &[
    Builtin {
        name: "__builtin_panic",
        params: &[BuiltinType::Bytes],
        ret: BuiltinType::Void,
        doc: "abort with a message; the compiler's stand-in for `std.panic`",
    },
    // `str(x)` and `repr(x)` are the two ways text is asked for. The compiler
    // lowers them rather than providing a body, because what they do depends
    // on the argument's type: a type that defines `__str__` is asked for it,
    // and everything else is rendered by the compiler.
    //
    // They are builtins rather than members so that they work on any value,
    // including the ones that cannot have methods.
    Builtin {
        name: "repr",
        params: &[BuiltinType::Any],
        ret: BuiltinType::Text,
        doc: "render a value unambiguously, for debugging",
    },
    Builtin {
        name: "str",
        params: &[BuiltinType::Any],
        ret: BuiltinType::Text,
        doc: "render a value as human-readable text",
    },
];

/// Looks a builtin up by name.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS
        .binary_search_by(|builtin| builtin.name.cmp(name))
        .ok()
        .map(|index| &BUILTINS[index])
}

/// Returns `true` if `name` is a builtin.
#[must_use]
pub fn is_builtin(name: &str) -> bool {
    lookup(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted() {
        // `lookup` binary-searches, so an out-of-order entry silently stops
        // being found. This has caught the same mistake twice elsewhere.
        for pair in BUILTINS.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "BUILTINS is not sorted: {:?} then {:?}",
                pair[0].name,
                pair[1].name
            );
        }
    }

    #[test]
    fn every_entry_is_findable() {
        for builtin in BUILTINS {
            assert_eq!(lookup(builtin.name), Some(builtin));
            assert!(is_builtin(builtin.name));
        }
    }

    #[test]
    fn unknown_names_are_not_builtins() {
        assert!(!is_builtin("panic"));
        assert!(!is_builtin("__builtin_nonexistent"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn panic_takes_a_message_and_returns_nothing() {
        let panic = lookup("__builtin_panic").expect("the panic builtin exists");
        assert_eq!(panic.params, &[BuiltinType::Bytes]);
        assert_eq!(panic.ret, BuiltinType::Void);
    }
}
