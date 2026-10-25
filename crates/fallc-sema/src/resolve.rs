//! Name resolution: binding every name occurrence to a definition.
//!
//! # Scopes
//!
//! A [`Resolver`] maintains a stack of [`ScopeId`]s. Each scope maps a name to
//! the [`DefId`] introduced in it, and lookup walks outward from the innermost
//! scope. Scoping is block scoping: a block, an `if`/loop body, a `for`
//! statement, a `switch` arm, and a bare `{ .. }` each get their own scope, and
//! an inner declaration shadows an outer one of the same name. Declaring one
//! name twice in the same scope is an error.
//!
//! Resolving is deliberately independent of typing: it produces a
//! [`Resolutions`] table keyed by source span, which the type checker reads.
//! Names are case-sensitive.

use std::collections::HashMap;

use fallc_ast::{Ast, ExprKind, ItemKind, NodeId, StmtKind};
use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_span::Span;

/// Identifies a definition (a name-introducing declaration).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DefId(u32);

impl DefId {
    /// A definition id that refers to no definition.
    pub const DUMMY: DefId = DefId(u32::MAX);

    /// Creates a definition id from its raw index.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        DefId(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns `true` if this is [`DefId::DUMMY`].
    #[must_use]
    pub const fn is_dummy(self) -> bool {
        self.0 == u32::MAX
    }
}

impl std::fmt::Display for DefId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_dummy() {
            f.write_str("def#dummy")
        } else {
            write!(f, "def#{}", self.0)
        }
    }
}

/// The type-trait queries the compiler answers itself.
///
/// Empty: every type predicate the language specifies is a standard-library
/// type family (`std.type_traits`) whose answer is a constant member chosen by
/// template specialisation -- `IsArray` through a `[]T` pattern that leaves the
/// length open, `IsSame` through a pattern that binds one parameter in two
/// positions, `MoveCanRaise` through the `except` query on a move
/// construction. The table and its arity accessor stay because the resolver
/// asks the arity question when deciding whether a name may be a trait at all;
/// a future compiler-owned predicate would be listed here.
pub const BUILTIN_TYPE_TRAITS: &[(&str, usize)] = &[];

/// How many type arguments a built-in type trait takes.
///
/// `None` means the name is not one of the compiler's traits and the query must
/// be looked up among the declared ones.
#[must_use]
pub fn builtin_type_trait_arity(name: &str) -> Option<usize> {
    BUILTIN_TYPE_TRAITS
        .iter()
        .find(|(trait_name, _)| *trait_name == name)
        .map(|(_, arity)| *arity)
}

/// Identifies a lexical scope.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScopeId(u32);

impl ScopeId {
    /// The scope id that refers to no scope (the parent of the outermost).
    pub const ROOT: ScopeId = ScopeId(u32::MAX);

    /// Creates a scope id from its raw index.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        ScopeId(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns `true` if this is [`ScopeId::ROOT`].
    #[must_use]
    pub const fn is_root(self) -> bool {
        self.0 == u32::MAX
    }
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_root() {
            f.write_str("scope#root")
        } else {
            write!(f, "scope#{}", self.0)
        }
    }
}

/// What sort of thing a definition introduces.
///
/// A type, a function, a parameter, a local binding, a module-level constant, or
/// a template parameter. `Unspecified` stands in when the kind cannot be taken
/// from a definition.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum DefKind {
    /// A nominal type declared by `type`.
    Type,
    /// A function.
    Function,
    /// A function parameter.
    Param,
    /// A local binding.
    Local,
    /// A module-level constant declared by `const`.
    Const,
    /// A template parameter.
    ///
    /// It stands for a type or a value the instantiation supplies, so what it
    /// *is* cannot be known until the template is instantiated. It is a
    /// definition so that the name resolves inside the body, and the
    /// instantiation is what gives it a meaning.
    TemplateParam,
    /// A definition whose kind is not known, used when the resolution has no
    /// definition to take a kind from.
    Unspecified,
}

/// What one module makes visible to its importers.
///
/// Only exported names appear here: a module's private declarations are not
/// reachable from outside, so there is nothing to record for them.
#[derive(Clone, Debug, Default)]
pub struct ExternalModule {
    /// Exported functions by name, each with every overload of it.
    ///
    /// A list rather than one signature, because a module's overloads must all
    /// survive export: a type with two constructors exported only the first,
    /// so `String s = "hi";` found the copy constructor and rejected the call.
    pub symbols: HashMap<String, Vec<FunctionSignature>>,
    /// Exported types by name.
    ///
    /// A type is exported the same way a function is, and for the same reason:
    /// a module's declarations are private unless published, so an importer
    /// cannot name a type its module did not offer.
    pub types: HashMap<String, ExportType>,
    /// Exported constants by name, already folded to their value.
    ///
    /// A constant is the third kind of export: it is neither callable nor
    /// nameable as a type, but a program that imported the module may use its
    /// value -- `C.EOF`, `std.INITIAL_CAPACITY`. Folded at the point of export
    /// because a constant is known before anything runs, and because the
    /// expression that computed it belongs to the exporting module's tree.
    pub constants: HashMap<String, i64>,
    /// The declared type of each exported constant, as written.
    ///
    /// A value on its own is not enough: an importer that read every constant
    /// as an `int` would store four bytes of an eight-byte slot. The type is
    /// resolved by the importer, because a written type names types in the
    /// declaring module's table.
    pub constant_types: HashMap<String, fallc_ast::TypeSyntax>,
    /// Exported `extern` variables by name.
    ///
    /// Its storage belongs to the library it is declared against, so what an
    /// importer gets is a name, a type, and a linker symbol; `C.stdin` is the
    /// reason it exists. A module-level variable this module *defines* crosses
    /// as [`Self::globals`] instead -- the object itself, shared by the whole
    /// program.
    pub variables: HashMap<String, ExportedVariable>,
    /// Exported module-level variables by name.
    ///
    /// A variable is one object for the whole program, so an importer reads and
    /// writes the *same* storage: what crosses is a symbol and a type, and the
    /// type is resolved already because a global's may be `auto`, which only
    /// the declaring module can deduce.
    pub globals: HashMap<String, ExportedGlobal>,
    /// The names of exported traits.
    ///
    /// A trait's *requirements* are expressions, so they travel with the
    /// declaring module's tree rather than here; the name is all that is needed
    /// to answer `from m import Trait` and to publish the name again.
    pub traits: std::collections::HashSet<String>,
    /// Exported enums by name.
    ///
    /// An enum is a *type* whose members are named constants of it, so it
    /// travels as its base type and the value of every member: a program that
    /// imported it writes `Color.RED` and gets the number the declaring module
    /// folded.
    pub enums: HashMap<String, ExportEnum>,
    /// Exported interfaces by name.
    ///
    /// An interface's value is a heap copy plus a method table, and its layout
    /// is the same everywhere; what an importer cannot invent is the list of
    /// methods, so that is what travels.
    pub interfaces: HashMap<String, ExportInterface>,
}

/// An exported module-level variable, in the form an importer needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportedGlobal {
    /// The symbol the linker resolves, which is the declared name.
    pub symbol: String,
    /// The type the variable is read and written as, resolved where it was
    /// declared.
    pub ty: crate::types::Type,
    /// The `@deprecated` message, when the declaration carries the attribute.
    pub deprecated: Option<String>,
    /// Whether the object is one per thread (`thread_local`).
    ///
    /// An importer reads it through the platform's TLS model, so the declaration
    /// it makes has to say so; a plain external declaration would resolve the
    /// same symbol against the wrong storage.
    pub thread_local: bool,
}

/// An exported enum, in the form an importer needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportEnum {
    /// The enum's name.
    pub name: String,
    /// The base type as written; `None` means `int`.
    ///
    /// Resolved by the importer, because a written type names types in the
    /// declaring module's table -- and the base is nearly always a primitive.
    pub base: Option<fallc_ast::TypeSyntax>,
    /// The members, in declaration order, with the values the declaring module
    /// folded for them.
    ///
    /// A member with no written value is one more than the previous one, and
    /// the first is zero; folding that here means every module agrees on the
    /// number without repeating the rule.
    pub members: Vec<(String, i64)>,
}

/// An exported interface, in the form an importer needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportInterface {
    /// The interface's name.
    pub name: String,
    /// The template parameters, empty for an ordinary interface.
    pub template_params: Vec<fallc_ast::TemplateParam>,
    /// The methods a box dispatches, in declaration order.
    ///
    /// Order is part of the interface: a value's method table is built in this
    /// order, and a virtual call is an index into it.
    pub methods: Vec<ExportInterfaceMethod>,
    /// Whether this is `interface Name;` -- the name and nothing else.
    pub is_forward: bool,
}

/// One method an exported interface's table holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportInterfaceMethod {
    /// The method's name.
    pub name: String,
    /// Its parameters' types, without the receiver: the receiver is the
    /// interface's own value.
    pub params: Vec<fallc_ast::TypeSyntax>,
    /// Its result, which the restriction stated with `-> Type`.
    pub returns: fallc_ast::TypeSyntax,
    /// Whether a call of it may raise.
    pub throws: bool,
}

/// A variable another module declared `extern`, as an importer sees it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportedVariable {
    /// The symbol the linker resolves, which is the declared name.
    pub symbol: String,
    /// The type the variable is read and written as, as written in the
    /// exporting module.
    pub ty: fallc_ast::TypeSyntax,
}

/// An exported type, in the form an importer needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportType {
    /// The type's name.
    pub name: String,
    /// Its fields, in declaration order.
    ///
    /// Order is part of the type: a positional initialiser `.{42, 37}` assigns
    /// by position, so an importer needs the same order the declaring module
    /// wrote.
    pub fields: Vec<ExportedField>,
    /// The template parameters, empty for an ordinary type.
    ///
    /// A family of types is a family wherever it is named: `Array<int>` has to
    /// mean the same thing to an importer as it does to the module that
    /// declared it, and the parameters are what say how many arguments the name
    /// takes and whether each is a type or a value.
    pub template_params: Vec<fallc_ast::TemplateParam>,
    /// Whether the fields share one address.
    ///
    /// Carried because a union's *layout* is what makes it a union: an
    /// importer that read it as a struct would put the members side by side,
    /// and every value crossing the boundary would be wrong.
    pub is_union: bool,
    /// The types declared inside this one, as `(name, target)` aliases.
    ///
    /// An importer names them through the owner -- `Array<int>.Iterator` -- so
    /// the names travel with the type. Only aliases are carried, because only
    /// an alias is a nested type the language specifies today.
    pub nested: Vec<(String, fallc_ast::TypeSyntax)>,
    /// The specialisations of this family declared in the same module.
    ///
    /// They travel resolved rather than as syntax: the pattern and the fields
    /// name types of the declaring module, and an importer has no record of the
    /// spans they were written at.
    pub specialisations: Vec<ExportedSpecialisation>,
    /// The constants declared in the family's own body, already folded.
    ///
    /// A family answers a question about its argument through a constant member
    /// -- `IsInteger<T>` holds `VALUE` -- so the answers have to cross with the
    /// family. They are folded where the expression's tree was in hand; the
    /// generic member's value does not depend on the arguments, so it is a
    /// single number per name.
    pub constants: Vec<(String, i64)>,
    /// Whether the type is only declared, not defined.
    ///
    /// A forward declaration crosses as what it is: an *incomplete* type, which
    /// another module may name behind a pointer or a reference. Publishing it as
    /// a complete type with no fields, or not publishing it at all, both broke
    /// the same way -- a function returning `*Node` was unusable, because the
    /// importer could not write `Node`.
    pub is_forward: bool,
    /// When the exported type is an *alias*, the type it names.
    ///
    /// An alias introduces no type of its own, so an importer has to reproduce
    /// the name as the same alias. Registering it as a nominal type made every
    /// imported alias an empty struct, and a value of the aliased type no longer
    /// fitted it.
    pub alias: Option<fallc_ast::TypeSyntax>,
}

/// One specialisation of a family, in the form another module can match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportedSpecialisation {
    /// The parameters the pattern binds, with their defaults dropped.
    pub params: Vec<fallc_ast::TemplateParam>,
    /// The argument pattern, resolved; a parameter appears as an unnamed type
    /// carrying its name.
    pub pattern: Vec<crate::types::Type>,
    /// The fields of the specialised member, resolved the same way.
    pub fields: Vec<(String, crate::types::Type)>,
    /// Whether the fields share one address.
    pub is_union: bool,
    /// Which of the specialised member's fields may still be written inside a
    /// `const` member function.
    ///
    /// Part of the member's type, not a note: an importer that dropped it
    /// refused a write the declaring module allows.
    pub mutable_fields: Vec<String>,
    /// The alignment each field asked for with `alignas`, resolved to bytes.
    ///
    /// Part of the member's *layout*: an importer that dropped it read every
    /// field after the first at the wrong offset.
    pub alignments: Vec<(String, u64)>,
    /// The specialised member's own constants, folded.
    ///
    /// A full specialisation answers a trait query differently from the generic
    /// member -- `IsInteger<int>` holds `VALUE = true` -- and the importer has
    /// no tree to fold the initialiser in, so the value travels here.
    pub constants: Vec<(String, i64)>,
}

impl ExportType {
    /// The fields as declarations, for building a family member.
    ///
    /// A field's *type* is written syntax and its resolved form is the
    /// importer's to work out, so both travel: the syntax is what a member of a
    /// family is built from, and the resolved form is what an ordinary imported
    /// type reports.
    #[must_use]
    pub fn declaration_fields(&self) -> Vec<fallc_ast::FieldDecl> {
        self.fields
            .iter()
            .map(|field| fallc_ast::FieldDecl {
                name: field.name.clone(),
                name_span: fallc_span::Span::SYNTHETIC,
                ty: field.ty.clone(),
                // Carried for what it is worth: mutability has no expression
                // half, so a member of an imported family keeps it.
                mutable: field.mutable,
                // An alignment *is* an expression, and the id belongs to the
                // declaring tree, so it cannot be carried here. It travels
                // resolved, in the nominal type's `field_alignments`.
                align: None,
                span: fallc_span::Span::SYNTHETIC,
            })
            .collect()
    }
}

/// One field of an exported type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportedField {
    /// The field's name.
    pub name: String,
    /// Its type, as written.
    pub ty: fallc_ast::TypeSyntax,
    /// The same type as the declaring module resolved it, when it did.
    ///
    /// A written type is resolved again by the importer, and for most types
    /// that is enough. One is not: `[32]byte` has a length that is a constant
    /// *expression* in the declaring module's tree, and an importer has no tree
    /// to fold it in -- so the answer travels with the field.
    pub resolved: Option<crate::types::Type>,
    /// Whether the field may still be written inside a `const` member function.
    pub mutable: bool,
    /// The alignment it asked for with `alignas`, in bytes.
    ///
    /// Resolved rather than written: the value is a constant expression in the
    /// declaring module's tree, and an importer has no tree to evaluate it in.
    pub align: Option<u64>,
}

impl ExternalModule {
    /// Records an exported function, keeping every overload of its name.
    pub fn add_function(&mut self, name: impl Into<String>, signature: FunctionSignature) {
        self.symbols.entry(name.into()).or_default().push(signature);
    }

    /// Records an exported constant, already folded to its value.
    pub fn add_constant(&mut self, name: impl Into<String>, value: i64) {
        self.constants.insert(name.into(), value);
    }

    /// Adds every symbol of another module, re-exporting them as this one's.
    ///
    /// # The rule
    ///
    /// A name this module already exports wins: a module that declares `sqrt`
    /// itself keeps its own, whatever it also imports. Otherwise the imported
    /// name is added as it stands, so an umbrella module presents exactly the
    /// names its parts export.
    ///
    /// Nothing is merged that the other module did not itself export, so a name
    /// has to be published at every step of the chain. That is what makes
    /// `export` mean the same thing wherever it appears.
    pub fn merge(&mut self, other: &ExternalModule) {
        for (name, signatures) in &other.symbols {
            // Every overload is forwarded, and a name this module already
            // exports keeps its own alongside them.
            //
            // It used to win *outright*, which lost a whole method the moment
            // two modules were gathered under one umbrella: `String.length` and
            // `Array.length` are different functions -- their receivers differ
            // -- and dropping one meant `a.length()` on an `Array` was reported
            // as a wrong receiver for `String`'s. The two cannot be told apart
            // by name, so the name must carry both.
            let entry = self.symbols.entry(name.clone()).or_default();
            // A signature already present is not added twice, so that merging
            // the same module along two paths stays idempotent.
            for signature in signatures {
                let duplicate = entry.iter().any(|existing| {
                    existing.written_name == signature.written_name
                        && existing.param_types == signature.param_types
                        && existing.bound_as == signature.bound_as
                        && existing.variadic == signature.variadic
                });
                if !duplicate {
                    entry.push(signature.clone());
                }
            }
        }
        for (name, ty) in &other.types {
            self.types.entry(name.clone()).or_insert_with(|| ty.clone());
        }
        // A constant is forwarded like the other two kinds, so an umbrella
        // publishes it under its own name.
        for (name, value) in &other.constants {
            self.constants.entry(name.clone()).or_insert(*value);
        }
        for (name, ty) in &other.constant_types {
            self.constant_types
                .entry(name.clone())
                .or_insert_with(|| ty.clone());
        }
        // And a variable, which is what an umbrella re-exports when it says
        // `export from C.stdio import *;`: `stdin` is declared in the stdio
        // module and published by `C`, exactly as `printf` is.
        for (name, variable) in &other.variables {
            self.variables
                .entry(name.clone())
                .or_insert_with(|| variable.clone());
        }
        // A module-level variable and a trait are published the same way.
        for (name, value) in &other.globals {
            self.globals.entry(name.clone()).or_insert_with(|| value.clone());
        }
        for name in &other.traits {
            self.traits.insert(name.clone());
        }
        // An enum and an interface are published the same way: a name at every
        // step of the chain, or it is not exported.
        for (name, value) in &other.enums {
            self.enums.entry(name.clone()).or_insert_with(|| value.clone());
        }
        for (name, value) in &other.interfaces {
            self.interfaces
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
    }

    /// Records an exported type.
    pub fn add_type(&mut self, name: impl Into<String>, ty: ExportType) {
        self.types.insert(name.into(), ty);
    }

    /// Records an exported enum.
    pub fn add_enum(&mut self, name: impl Into<String>, value: ExportEnum) {
        self.enums.insert(name.into(), value);
    }

    /// Records an exported interface.
    pub fn add_interface(&mut self, name: impl Into<String>, value: ExportInterface) {
        self.interfaces.insert(name.into(), value);
    }

    /// Whether this module exports `name`, as any kind of member.
    ///
    /// A name is exported or it is not; *what* it is decides how a use of it is
    /// checked, and one function per kind would let a caller that forgot one
    /// report an export that exists as missing.
    #[must_use]
    pub fn exports(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
            || self.types.contains_key(name)
            || self.constants.contains_key(name)
            || self.variables.contains_key(name)
            || self.enums.contains_key(name)
            || self.interfaces.contains_key(name)
            || self.globals.contains_key(name)
            || self.traits.contains(name)
    }

    /// Returns the names this module exports, functions and types alike.
    ///
    /// Sorted, so that a caller binding them in order binds them the same way
    /// on every run.
    #[must_use]
    pub fn export_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .variables
            .keys()
            .chain(self.symbols.keys())
            .chain(self.types.keys())
            .chain(self.constants.keys())
            .chain(self.enums.keys())
            .chain(self.interfaces.keys())
            .chain(self.globals.keys())
            .chain(self.traits.iter())
            .cloned()
            .collect();
        names.sort();
        names
    }
}

/// A signature as the defining module's checker resolved it.
///
/// A written type is not enough to build a call across a module boundary. The
/// spelling `Crate<int>` names one *member* of a type family, and only the
/// module that declared the family knows which member that is and what symbol
/// it was emitted under -- the importer has neither the tree nor its
/// declarations. So the answers travel with the export instead of being worked
/// out a second time by a rule that would have to agree with the checker's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedSignature {
    /// The parameter types, in order.
    pub params: Vec<crate::types::Type>,
    /// The return type.
    pub returns: crate::types::Type,
    /// The symbol the defining module emitted the function under.
    pub symbol: String,
}

/// A function's signature, filled in once the function is declared.
///
/// Parameter and return types stay as written type syntax rather than resolved
/// types: resolution runs before type checking, and the checker resolves them
/// through the same path as any other written type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FunctionSignature {
    /// Parameter names, in order.
    pub param_names: Vec<String>,
    /// Parameter types, as written.
    ///
    /// Kept as syntax rather than as a rendered string, because rendering loses
    /// structure: `render_type_syntax` produces `*const int`, and parsing that
    /// back with `Type::from_name` fails, silently degrading the type to an
    /// unknown name that matches nothing. The checker resolves these itself.
    pub param_types: Vec<fallc_ast::TypeSyntax>,
    /// The return type, as written.
    pub return_type: fallc_ast::TypeSyntax,
    /// Whether the function takes any number of extra arguments.
    pub variadic: bool,
    /// How many parameters a call must supply.
    ///
    /// Parameters after this one have defaults and may be omitted, so this is
    /// where the defaulted tail begins.
    pub required_params: usize,
    /// The default value of each parameter, `None` where there is none.
    ///
    /// Kept as syntax and evaluated at the call site, as in C++.
    pub defaults: Vec<Option<fallc_ast::ExprId>>,
    /// The module the declaration came from, when it came from another one.
    ///
    /// A default expression names that module's tree, and evaluating it where
    /// the call is written needs the tree -- so the name is what identifies the
    /// tree to lower it with. `None` for a declaration in this file, whose
    /// defaults are lowered here already.
    pub declared_in: Option<String>,
    /// The default each of those parameters has, when the compiler worked the
    /// value out where it was declared.
    ///
    /// The expression above belongs to the declaring module's tree, so a call
    /// from another module cannot evaluate it. A value can cross, and that is
    /// what lets a call elsewhere leave such a parameter out: `getline(line)`
    /// for `getline(line, until = '\n')`. A default that is not a constant
    /// stays behind, and a caller in another module has to give it.
    pub default_values: Vec<Option<i64>>,
    /// Whether the function is visible to importers.
    pub exported: bool,
    /// Whether this function is implemented elsewhere.
    pub is_external: bool,
    /// The ABI the declaration names, when it is written `extern "C"`.
    ///
    /// An `extern "C"` function keeps the name it was declared with: the C
    /// library exports exactly that spelling, and a mangled one names no symbol
    /// anybody defines. A bodyless declaration *without* an ABI is a promise
    /// that another translation unit defines it, and its symbol is mangled like
    /// every other function's.
    pub abi: Option<String>,
    /// The index of the parameter that is a pack, if there is one.
    ///
    /// A pack parameter takes any number of arguments instead of one, so the
    /// count is not fixed and the elements become parameters of the
    /// instantiation. It is always the last parameter: everything before it is
    /// fixed, which is what makes "the rest of the arguments" a well-defined
    /// thing.
    pub pack: Option<usize>,
    /// The template parameters, empty for an ordinary function.
    ///
    /// A function with any is a *template*: it declares a family of functions,
    /// and which one a call means is decided by the arguments. It is not
    /// itself callable, because there is no `T` to call it with.
    pub template_params: Vec<fallc_ast::TemplateParam>,
    /// The name as written, before any receiver was appended.
    ///
    /// A symbol is built from this and the parameter types, so it must be the
    /// written name: `__as__`, not `__as__Box`.
    pub written_name: String,
    /// The name this definition is bound under, when it differs from `name`.
    ///
    /// A special method is bound as `__as__Box` and emitted as
    /// `__as__Box__const_Box__double`; a caller that resolved one needs both,
    /// so the definition name travels with the signature.
    pub bound_as: String,
    /// The condition of a `??(cond)` claim, when the declaration is in this
    /// tree.
    ///
    /// The expression id belongs to one tree, so a signature that crossed a
    /// module boundary carries none: a caller in another module cannot look at
    /// it, and treats the function as always able to raise.
    pub throws_condition: Option<fallc_ast::ExprId>,
    /// Whether calling this may raise an exception.
    ///
    /// True for a body marked `??`. A body marked `??(cond)` is true only when
    /// `cond` cannot be shown false: the claim is the function's, and a caller
    /// that trusts it without proof is trusting the wrong party.
    pub throws: bool,
    /// The deprecation message of a declaration marked `@deprecated`, when the
    /// signature came from another module.
    ///
    /// `Some("")` means deprecated with no message of its own. Part of what a
    /// declaration publishes: a use in an importing module has to say so too.
    pub deprecated: Option<String>,
    /// The signature as the defining module's checker resolved it, present only
    /// for a signature collected for export.
    ///
    /// A template's is absent: its parameter types name its own template
    /// parameters, and which types stand in them is decided per instantiation.
    pub resolved: Option<ResolvedSignature>,
    /// Whether the function is generated per use rather than emitted once.
    ///
    /// A template, and a method written against a type family's parameters, are
    /// both: they have no symbol of their own, because every instantiation is a
    /// function. Publishing one derived from the *generic* signature gave it a
    /// name that a concrete type of the same name could match -- a program's own
    /// `type Box` was taken for `std`'s `Box<T>`, and a destructor call was
    /// recorded for a type that has none.
    pub instantiated: bool,
}

/// Builds the type syntax a builtin's parameter or return type corresponds to.
///
/// A builtin's byte pointer is written `*byte` so that it resolves to a real
/// pointer type through the ordinary rules, rather than being special-cased in
/// the checker.
fn builtin_type_syntax(ty: crate::builtins::BuiltinType, span: Span) -> fallc_ast::TypeSyntax {
    use fallc_ast::TypeSyntax;
    match ty {
        // `void` is not a type in the language, so a builtin that returns
        // nothing is given no return type at all and the caller discards it.
        // The checker treats an empty name as "no value".
        crate::builtins::BuiltinType::Void => TypeSyntax::Named(fallc_ast::NamedType {
            path: String::new(),
            arguments: Vec::new(),
                is_expansion: false,
            span,
        }),
        // A builtin that takes characters takes a pointer to them. Written as
        // a real pointer type -- rather than as a string-only pseudo-type --
        // so that an ordinary string literal decays into it by the same rule
        // every other pointer parameter uses.
        // An integer parameter takes whatever integer the caller passes, so it
        // is declared as the default integer; a literal adopts it and a wider
        // argument is widened by the call.
        crate::builtins::BuiltinType::Integer => TypeSyntax::Named(fallc_ast::NamedType {
            path: "int".to_owned(),
            arguments: Vec::new(),
                is_expansion: false,
            span,
        }),
        crate::builtins::BuiltinType::Bytes => TypeSyntax::PointerToConst(Box::new(
            TypeSyntax::Named(fallc_ast::NamedType {
                path: "c8".to_owned(),
                arguments: Vec::new(),
                is_expansion: false,
                span,
            }),
        )),
        // `any` and `Text` are not types the argument or the result is checked
        // against: `str` and `repr` accept every value and are lowered by the
        // compiler according to what they were given. Their signatures exist
        // so that the names resolve and a call has an arity; the checker looks
        // at the builtin's name, not at these types, before reaching here.
        crate::builtins::BuiltinType::Any | crate::builtins::BuiltinType::Text => {
            TypeSyntax::Named(fallc_ast::NamedType {
                path: "void".to_owned(),
                arguments: Vec::new(),
                is_expansion: false,
                span,
            })
        }
    }
}

/// Returns the name a function is defined under, when it differs from the
/// written one.
///
/// A member function is written once per receiver type but is a distinct
/// symbol for each, so `__init__` becomes `__init__Point`. Every other function
/// keeps its own name.
#[must_use]
pub fn member_name(function: &fallc_ast::FunctionItem) -> Option<String> {
    let receiver = function.params.first()?;
    // A receiver is an indirection, because the method modifies the object it
    // was called on rather than a copy of it: either a reference or a pointer.
    //
    // A reference is the form to write -- `this &T` -- and a pointer is still
    // accepted, because a method and a free function taking a pointer are the
    // same function, and refusing the pointer spelling would break C-facing
    // code for no gain.
    let inner = match &receiver.ty {
        fallc_ast::TypeSyntax::Reference(inner)
        | fallc_ast::TypeSyntax::ConstReference(inner)
        | fallc_ast::TypeSyntax::RvalueReference(inner)
        | fallc_ast::TypeSyntax::ConstRvalueReference(inner)
        | fallc_ast::TypeSyntax::Pointer(inner)
        | fallc_ast::TypeSyntax::PointerToConst(inner)
        | fallc_ast::TypeSyntax::ConstPointer(inner) => inner,
        _ => return None,
    };
    let fallc_ast::TypeSyntax::Named(named) = inner.as_ref() else {
        return None;
    };
    if receiver.name != "this" {
        return None;
    }
    // A primitive's special method is bound under the name the type actually
    // has, not the alias it was written with: `__hash__` on an `int` is
    // `__hash__i32`, because a call reaches it through the type, whose
    // spelling is `i32`. Without this the declaration and the call were filed
    // under two names and the method did not exist.
    let path = match crate::types::Type::from_name(&named.path) {
        Some(primitive) if primitive.nominal_name().is_none() => primitive.spelling(),
        _ => named.path.clone(),
    };
    Some(path)
}

/// Returns the definition name of a special method, if this is one.
///
/// A special method is defined once per receiver type, so its name carries the
/// type: `__add__` on `Fraction` is `__add__Fraction`. That is what lets two
/// types each define `+`, and it is the same treatment `__init__` already had.
///
/// An ordinary member function keeps its own name, because a call to it is
/// written `value.name()` and the receiver is known at the call site; an
/// operator call has no name to look up, so the type has to be in the symbol.
fn special_method_name(function: &fallc_ast::FunctionItem) -> Option<String> {
    if !fallc_ast::special::is_special(&function.name) {
        return None;
    }
    member_name(function).map(|receiver| format!("{}{receiver}", function.name))
}

/// Returns the definition name a function is bound under.
///
/// A special method gets its receiver's type appended; everything else keeps
/// its written name, except a constructor, which is a special method too.
#[must_use]
pub fn definition_name(function: &fallc_ast::FunctionItem) -> String {
    special_method_name(function).unwrap_or_else(|| function.name.clone())
}

/// Extracts a signature from a function item.
fn function_signature(function: &fallc_ast::FunctionItem) -> Option<FunctionSignature> {
    Some(FunctionSignature {
        param_names: function.params.iter().map(|p| p.name.clone()).collect(),
        param_types: function.params.iter().map(|p| p.ty.clone()).collect(),
        return_type: function.return_type.clone(),
        variadic: function.variadic,
        // A call must supply every parameter up to the first defaulted one.
        required_params: function
            .params
            .iter()
            .position(|param| param.default.is_some())
            .unwrap_or(function.params.len()),
        defaults: function.params.iter().map(|p| p.default).collect(),
        declared_in: None,
        default_values: Vec::new(),
        exported: function.exported,
        is_external: function.body.is_none(),
        abi: function.abi.clone(),
        pack: function.params.iter().position(|param| param.is_pack),
        written_name: function.name.clone(),
        bound_as: definition_name(function),
        template_params: function.template_params.clone(),
        throws: function.throws.is_some(),
        throws_condition: function.throws.as_ref().and_then(|throws| throws.condition),
        // Filled in where a signature is collected for export: this one is a
        // definition inside a module, and its attributes are not here.
        deprecated: None,
        // Filled in where a signature is collected for export: this one is a
        // definition inside a module, and nothing outside the resolver sees it.
        resolved: None,
        instantiated: false,
    })
}

/// A resolved definition.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Def {
    /// The definition's id.
    pub id: DefId,
    /// What sort of thing this is.
    pub kind: DefKind,
    /// The name being defined.
    pub name: String,
    /// The AST node this definition came from, if it has one.
    pub node: Option<NodeId>,
    /// The span of the *name*, not of the whole declaration.
    pub name_span: Span,
    /// The span of the whole declaration.
    pub span: Span,
    /// The scope this definition lives in.
    pub scope: ScopeId,
    /// The function's signature, present only for [`DefKind::Function`].
    pub signature: Option<FunctionSignature>,
    /// The linker symbol this definition is emitted under, when it differs
    /// from `name`.
    ///
    /// A special method's symbol carries its receiver's type and, when it is
    /// overloaded, its parameter types -- mangling that needs resolved types.
    /// The checker has those and the resolver does not, so the checker records
    /// the symbol here and code generation reads it rather than reconstructing
    /// the scheme and hoping the two agree.
    pub emitted_as: Option<String>,
    /// For an imported function, the name it has in its defining module.
    ///
    /// Code generation must call the symbol the *defining* module emitted,
    /// which is the bare name, not the `Module.member` path used in source.
    pub imported_as: Option<String>,
    /// For an imported function, the module it is reached through here.
    ///
    /// This is the *importing* file's spelling, which is what the driver files
    /// the external signature under: a function declared in `Stdio` but
    /// re-exported by `C` is registered as `C.printf`, and a call written
    /// `printf` still has to find it.
    pub imported_via: Option<String>,
    /// Whether this definition is a compiler-provided builtin.
    ///
    /// Builtins have no source, so code generation declares them in the module
    /// instead of finding a definition for them.
    pub is_builtin: bool,
}

/// The output of name resolution.
#[derive(Clone, Debug, Default)]
pub struct Resolutions {
    /// Every definition, indexed by [`DefId`].
    pub defs: Vec<Def>,
    /// Maps each resolved name occurrence to its definition.
    bindings: Vec<(Span, DefId)>,
}

impl Resolutions {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Resolutions::default()
    }

    /// Records that the name at `span` resolves to `def`.
    pub fn bind(&mut self, span: Span, def: DefId) {
        self.bindings.push((span, def));
    }

    /// Returns the definition a name occurrence resolves to.
    #[must_use]
    pub fn resolve(&self, span: Span) -> Option<DefId> {
        self.bindings
            .iter()
            .find(|(s, _)| *s == span)
            .map(|(_, def)| *def)
    }

    /// Returns the definition with `id`.
    #[must_use]
    pub fn def(&self, id: DefId) -> Option<&Def> {
        self.defs.get(id.0 as usize)
    }

    /// Returns the number of recorded bindings.
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Returns `true` if any name resolves to `id`.
    ///
    /// A definition that no occurrence resolves to was never read, which is
    /// what the unused-binding warning is about.
    #[must_use]
    pub fn is_referenced(&self, id: DefId) -> bool {
        self.bindings.iter().any(|(_, bound)| *bound == id)
    }
}

/// The output of running the resolver.
#[derive(Clone, Debug)]
pub struct ResolveResult {
    /// The value of each exported constant the imports brought in, by the name
    /// the program writes. `C.EOF` is a value, and a value is the checker's.
    pub constants_by_name: std::collections::HashMap<String, i64>,
    /// The resolution table.
    pub resolutions: Resolutions,
    /// What went wrong (unknown names, duplicates, ...).
    pub diagnostics: Vec<Diagnostic>,
}

impl ResolveResult {
    /// Returns `true` if no fatal diagnostic was reported.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_fatal)
    }
}

/// One lexical scope: a map from name to the definition it denotes.
#[derive(Clone, Debug, Default)]
struct Scope {
    parent: Option<ScopeId>,
    /// The definitions introduced in this scope, by name.
    ///
    /// A name may have several: functions overload, and the choice between them
    /// is made where the call is checked, not here. Every other kind of
    /// definition is unique, so a list of one.
    names: HashMap<String, Vec<DefId>>,
}

/// Resolves names in an AST.
#[derive(Debug)]
pub struct Resolver {
    scopes: Vec<Scope>,
    /// Type names, so that a type used as a callee is not called an unknown
    /// value. A type lives in the type namespace and is the checker's to look
    /// up; this is only what the resolver needs not to complain.
    known_types: std::collections::HashSet<String>,
    /// Names that are traits, which are *asked about* a type rather than
    /// called.
    ///
    /// Kept apart from the type names because the two are read in different
    /// positions: a type name in a call construct one, while a trait name in a
    /// call is a question -- and a qualified one, `std.InputIterator(x)`, is
    /// written as a field access that would otherwise be looked up as a member
    /// of the module.
    known_traits: std::collections::HashSet<String>,
    /// Type names declared with `type Name;` and not yet defined.
    ///
    /// A declaration and its definition share one name, so the resolver has to
    /// know which one it saw: the second mention of a forward-declared type is
    /// its definition, not a duplicate.
    forward_types: std::collections::HashSet<String>,
    /// The value of each exported constant an import brought in, by local name.
    ///
    /// `C.EOF` is a value, and a value is what the checker folds -- so it is
    /// recorded under the name the program writes and read from there.
    constants_by_name: std::collections::HashMap<String, i64>,
    /// How many compile-time `if`s are being walked whose condition is not yet
    /// decided. Inside one, an unresolvable name is left unresolved rather than
    /// reported: the branch may be the one the checker discards.
    lenient: u32,
    resolutions: Resolutions,
    diagnostics: DiagnosticBag,
    /// What other modules export, by module name.
    ///
    /// Filled by the module loader before resolution, so a qualified name such
    /// as `C.printf` can be bound without the resolver knowing anything about
    /// files or the file system.
    external: HashMap<String, ExternalModule>,
    /// Definitions by id, so item declaration can attach a signature after
    /// `define` has already returned.
    defs: Vec<Option<FunctionSignature>>,
    /// The local names an import introduces, by module.
    ///
    /// A plain `import C` binds the module's name, so `C.printf` resolves and
    /// a bare `printf` does not. A `from C import printf` binds `printf`
    /// itself. Both end up here as a name to install in the root scope,
    /// pointing at the module member it denotes, which is what lets the three
    /// spellings share every later pass.
    imports: Vec<ImportBinding>,
    /// Whether only declarations are resolved.
    ///
    /// A module whose compiled object is reused contributes declarations to the
    /// importer: its function bodies were resolved when the module was built.
    /// The bodies that must still be resolved are the ones an *instantiation*
    /// will be lowered from -- a template's own body is written here and checked
    /// elsewhere -- so those are resolved either way.
    declarations_only: bool,
    /// The names a type family declares its parameters with.
    ///
    /// A method written against them -- `fn push(this &Array<T>, ..)` -- is a
    /// template in every way that matters, so its body is resolved even in a
    /// declaration-only pass.
    family_parameters: std::collections::HashSet<String>,
}

/// One name an import brings into the importing file's scope.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportBinding {
    /// The name as it may be written in this file.
    pub local: String,
    /// The module it comes from.
    pub module: String,
    /// The member of that module it denotes.
    ///
    /// `None` for a plain `import C`, where the name is the module itself and
    /// the member is only known once `C.x` is written.
    pub member: Option<String>,
    /// Where the name was written, for diagnostics.
    pub span: Span,
    /// Whether this name may be re-exported by `from .. import *`.
    ///
    /// A `from M import *` re-exports what it brings in, so an umbrella module
    /// can forward another module's exports. A named import is a local
    /// convenience and is not forwarded, so that a file's exports stay a
    /// property of what it deliberately publishes.
    pub reexported: bool,
}

impl Resolver {
    /// Records that a name belongs to the type namespace.
    ///
    /// A type is not a value, so a use of one as a callee -- `Array<int>()` --
    /// must not be reported as an unknown name. Which types exist is the
    /// driver's knowledge, so they are handed over rather than discovered.
    pub fn add_known_type(&mut self, name: impl Into<String>) {
        self.known_types.insert(name.into());
    }

    /// Records that `name` is a trait.
    ///
    /// Marked separately from a type so that a *qualified* trait query --
    /// `std.InputIterator(x)` -- is not resolved as a member of a value: the
    /// field is a question about types, and the trait it names is looked up by
    /// the checker.
    pub fn add_known_trait(&mut self, name: impl Into<String>) {
        self.known_traits.insert(name.into());
    }

    /// Creates a resolver with a single root scope already pushed.
    #[must_use]
    pub fn new() -> Self {
        let mut resolver = Resolver {
            scopes: Vec::new(),
            known_types: std::collections::HashSet::new(),
            known_traits: std::collections::HashSet::new(),
            forward_types: std::collections::HashSet::new(),
            constants_by_name: std::collections::HashMap::new(),
            lenient: 0,
            resolutions: Resolutions::new(),
            diagnostics: DiagnosticBag::new(),
            external: HashMap::new(),
            defs: Vec::new(),
            imports: Vec::new(),
            declarations_only: false,
            family_parameters: std::collections::HashSet::new(),
        };
        resolver.push_scope();
        // A primitive type's name is a type, so a use of one is not an unknown
        // *value*: `Input(int)` asks about a type the program never declared,
        // and the resolver is the stage that would otherwise report `int` as
        // missing.
        for name in [
            "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "int", "uint",
            "isize", "usize", "byte", "bool", "char", "c8", "c16", "c32", "float", "double",
            "f32", "f64", "void",
        ] {
            resolver.known_types.insert(name.to_owned());
        }
        // Builtins live in the outermost scope, so a program can call them
        // without declaring them and any user definition of the same name
        // would shadow them in an inner scope.
        resolver.declare_builtins();
        resolver
    }

    /// Sets whether only declarations are resolved; see
    /// [`Resolver::declarations_only`].
    pub fn set_declarations_only(&mut self, declarations_only: bool) {
        self.declarations_only = declarations_only;
    }

    /// Records the parameter names of a type family declared in this tree.
    ///
    /// A function written against one of them is a template member: its body is
    /// resolved even when only declarations are wanted, because it is the body
    /// an instantiation is lowered from.
    pub fn add_family_parameters(&mut self, names: impl IntoIterator<Item = String>) {
        self.family_parameters.extend(names);
    }

    /// Runs resolution over `ast`.
    pub fn resolve(&mut self, ast: &Ast) -> ResolveResult {
        // Imported names are installed before anything is declared, so that an
        // item in this file can refer to one, and so that a local declaration
        // of the same name is reported as the duplicate it is rather than
        // silently shadowing the import.
        self.apply_imports();
        self.resolve_items(ast);
        ResolveResult {
            resolutions: std::mem::take(&mut self.resolutions),
            constants_by_name: std::mem::take(&mut self.constants_by_name),
            diagnostics: std::mem::take(&mut self.diagnostics).into_vec(),
        }
    }

    /// Resolves every top-level item.
    ///
    /// Two phases: every top-level name is declared first, then every body is
    /// resolved. That is what lets a module's items refer to one another
    /// regardless of the order they are written in.
    fn resolve_items(&mut self, ast: &Ast) {
        // Two phases, because functions may refer to each other regardless of
        // order: declare every top-level name first, then resolve bodies.
        for &item in &ast.items {
            self.declare_item(ast, item);
        }
        for &item in &ast.items {
            self.resolve_item_body(ast, item);
        }
    }

    /// Registers every compiler-provided function in the root scope.
    ///
    /// Their signatures are recorded as written type syntax so that the type
    /// checker resolves them through the same path as any other signature.
    fn declare_builtins(&mut self) {
        for builtin in crate::builtins::BUILTINS {
            let span = Span::SYNTHETIC;
            let def = self.define(builtin.name, DefKind::Function, span, span, None);

            // A builtin has no source, so its signature is built from the
            // table rather than read from syntax that does not exist. The
            // parameter syntax uses the type name a caller writes.
            let signature = FunctionSignature {
                param_names: (0..builtin.params.len())
                    .map(|index| format!("arg{index}"))
                    .collect(),
                param_types: builtin
                    .params
                    .iter()
                    .map(|param| builtin_type_syntax(*param, span))
                    .collect(),
                return_type: builtin_type_syntax(builtin.ret, span),
                is_external: true,
                abi: None,
                pack: None,
                written_name: builtin.name.to_owned(),
                bound_as: builtin.name.to_owned(),
                // A builtin is never a template: it is a fixed signature.
                template_params: Vec::new(),
                // `__builtin_panic` takes exactly one argument.
                variadic: false,
                required_params: builtin.params.len(),
                defaults: vec![None; builtin.params.len()],
                declared_in: None,
                default_values: vec![None; builtin.params.len()],
                exported: true,
                // A builtin raises nothing the language models: `panic` aborts.
                throws: false,
                throws_condition: None,
                deprecated: None,
                // A builtin is implemented elsewhere, so it keeps its name and
                // there is no checker resolution to carry.
                resolved: None,
                instantiated: false,
            };
            self.set_signature(def, signature);
            if let Some(entry) = self.resolutions.defs.get_mut(def.as_raw() as usize) {
                entry.is_builtin = true;
            }
        }
    }

    /// Binds an item's name, without looking at its body.
    fn declare_item(&mut self, ast: &Ast, id: fallc_ast::ItemId) {
        let item = ast.item(id);
        match &item.kind {
            // A module-level constant is a name, like a function's is. It is
            // declared before any body is resolved, so that a body may refer to
            // one declared later in the file.
            ItemKind::Const(constant) => {
                self.define(
                    constant.name.clone(),
                    DefKind::Const,
                    constant.name_span,
                    item.span,
                    Some(item.node),
                );
            }
            // An external variable is a name too, and the library that holds it
            // is reached through the linker rather than through this module.
            ItemKind::ExternVariable(variable) => {
                self.define(
                    variable.name.clone(),
                    DefKind::Local,
                    variable.name_span,
                    item.span,
                    Some(item.node),
                );
            }
            // A module-level variable is a name with storage. It is bound like
            // a constant, so that a body may read it before it is written in the
            // file.
            ItemKind::Variable(variable) => {
                self.define(
                    variable.name.clone(),
                    DefKind::Local,
                    variable.name_span,
                    item.span,
                    Some(item.node),
                );
            }
            // An assertion declares no name: it is a claim, not a declaration.
            ItemKind::StaticAssert(_) => {}
            ItemKind::Function(function) => {
                // A constructor is defined once per type, so its name alone
                // would collide. The definition carries the receiver's type,
                // which is also what a call site looks up.
                let defined_as = definition_name(function);
                let def = self.define(
                    defined_as,
                    DefKind::Function,
                    function.name_span,
                    item.span,
                    Some(item.node),
                );
                // The signature is recorded in both places: `self.defs` is the
                // resolver's own view, and `resolutions.defs` is what later
                // passes read. Storing it in only one silently leaves the
                // other empty, which shows up as "not a function".
                if let Some(signature) = function_signature(function) {
                    self.set_signature(def, signature);
                }
            }
            // An import's names are installed by `apply_imports`, from the
            // bindings the module loader produced.
            ItemKind::Import(_) => {}
            // A link instruction declares no name: it is read by the driver,
            // which is the only part that links.
            ItemKind::Link(_) => {}
            // A type's name is introduced so a declaration may refer to it,
            // and so a duplicate is reported.
            ItemKind::TypeDeclaration(declaration) => {
                // A specialisation is a member of a family, not a name: the
                // family declares the name, and the pattern decides which
                // member this is. Registering it here would make the family
                // "defined more than once".
                if !declaration.pattern.is_empty() {
                    return;
                }
                // A forward declaration and its definition are one type under
                // one name, so whichever comes second does not declare it
                // again. Two definitions are still a mistake, and each is a
                // definition once a definition has been seen.
                let already_a_type = self
                    .lookup(&declaration.name)
                    .and_then(|existing| self.resolutions.def(existing))
                    .is_some_and(|existing| existing.kind == DefKind::Type);
                if already_a_type
                    && (declaration.is_forward || self.forward_types.contains(&declaration.name))
                {
                    self.forward_types.remove(&declaration.name);
                    return;
                }
                if declaration.is_forward {
                    self.forward_types.insert(declaration.name.clone());
                }
                self.define(
                    declaration.name.clone(),
                    DefKind::Type,
                    declaration.name_span,
                    item.span,
                    Some(item.node),
                );
            }
            // An enum's name is a type, and each of its members is a constant
            // of that type. Both are declared here so that a body may use
            // `Color` and `Color.RED` however the members are ordered.
            ItemKind::EnumDeclaration(declaration) => {
                // The same declaration/definition rule as a `type`: the second
                // mention completes the first.
                let already_a_type = self
                    .lookup(&declaration.name)
                    .and_then(|existing| self.resolutions.def(existing))
                    .is_some_and(|existing| existing.kind == DefKind::Type);
                let forward_seen = self.forward_types.contains(&declaration.name);
                if already_a_type && (declaration.is_forward || forward_seen) {
                    self.forward_types.remove(&declaration.name);
                } else {
                    if declaration.is_forward {
                        self.forward_types.insert(declaration.name.clone());
                    }
                    self.define(
                        declaration.name.clone(),
                        DefKind::Type,
                        declaration.name_span,
                        item.span,
                        Some(item.node),
                    );
                }
                for member in &declaration.members {
                    self.define(
                        format!("{}.{}", declaration.name, member.name),
                        DefKind::Const,
                        member.name_span,
                        member.span,
                        Some(item.node),
                    );
                }
            }
            // A trait declares nothing a body can call: it is asked *about* a
            // type rather than used as one, so it takes no place in the value
            // namespace. Its name reaches the scope as a known type name, which
            // is what keeps `Input(x)` from being reported as an unknown
            // function before the checker has decided it is a trait query.
            ItemKind::Trait(_) => {}
            // An interface is a type name, which the checker resolves; the
            // resolver has no expression to bind here.
            ItemKind::Interface(_) => {}
            // A setup block introduces no name.
            ItemKind::Setup(_) => {}
            // An unresolved item introduces no name.
            ItemKind::Unresolved(_) => {}
        }
    }

    /// Resolves the names inside an item's body.
    /// Whether this function's body is the source of an instantiation.
    ///
    /// A template declares its own parameters; a method of a type family uses
    /// the family's. Both are emitted once a member is chosen, so the body has
    /// to be resolved even when the module's other bodies are not.
    fn is_template_member(&self, function: &fallc_ast::FunctionItem) -> bool {
        if !function.template_params.is_empty() || function.has_pack() {
            return true;
        }
        if self.family_parameters.is_empty() {
            return false;
        }
        function
            .params
            .iter()
            .any(|parameter| type_mentions(&parameter.ty, &self.family_parameters))
            || type_mentions(&function.return_type, &self.family_parameters)
    }

    fn resolve_item_body(&mut self, ast: &Ast, id: fallc_ast::ItemId) {
        let item = ast.item(id);
        match &item.kind {
            ItemKind::Const(constant) => self.resolve_expr(ast, constant.value),
            // The block is a body of its own: its statements are resolved in a
            // scope of their own, like a function's.
            // A setup block is implementation: it runs before `main`, and its
            // resolved form belongs to the module's own object.
            ItemKind::Setup(block) if self.declarations_only => {
                let _ = block;
            }
            ItemKind::Setup(block) => self.scoped_block(ast, *block),
            // An assertion has a condition, and the names in it are resolved
            // like any others.
            ItemKind::StaticAssert(node) => self.resolve_expr(ast, node.condition),
            // An external variable has a type and no body: the type names
            // types, which the checker resolves.
            ItemKind::ExternVariable(_) => {}
            // An interface's restrictions are written against its subject, so
            // the subject is a name here exactly as a trait's parameter is:
            // whether the type behind it has the operation is the checker's
            // question, not this pass's.
            ItemKind::Interface(interface) if !interface.requirements.is_empty() => {
                self.push_scope();
                if let Some(subject) = &interface.subject {
                    self.define(
                        subject.name.clone(),
                        DefKind::Param,
                        subject.name_span,
                        subject.name_span,
                        None,
                    );
                }
                // An interface's type variables, exactly as a trait's: the
                // subject's type and the parameters written after it.
                if let Some(subject) = &interface.subject
                    && let fallc_ast::TypeSyntax::Named(named) = &subject.ty
                {
                    self.define(
                        named.path.clone(),
                        DefKind::TemplateParam,
                        interface.name_span,
                        interface.name_span,
                        None,
                    );
                }
                for parameter in &interface.template_params {
                    self.define(
                        parameter.name.clone(),
                        DefKind::TemplateParam,
                        parameter.name_span,
                        parameter.name_span,
                        None,
                    );
                }
                for requirement in &interface.requirements {
                    self.resolve_expr(ast, requirement.expr);
                }
                self.pop_scope();
            }
            ItemKind::Interface(_) => {}
            // The initialiser is resolved before the name is bound -- the same
            // rule a local follows -- and every initialiser form is walked.
            ItemKind::Variable(variable) => {
                match &variable.init {
                    Some(fallc_ast::Initializer::Expr(expr)) => self.resolve_expr(ast, *expr),
                    Some(fallc_ast::Initializer::Designated(init)) => {
                        for &element in &init.elements {
                            self.resolve_expr(ast, element);
                        }
                    }
                    None => {}
                }
            }
            // An enumerator's value is an expression -- `GREEN = 2 * 3` -- and
            // a *later* member may name an earlier one, so they are resolved in
            // order within one scope.
            ItemKind::EnumDeclaration(declaration) => {
                self.push_scope();
                for member in &declaration.members {
                    if let Some(value) = member.value {
                        self.resolve_expr(ast, value);
                    }
                    // Bound as it is resolved, so `YELLOW = GREEN + 1` finds
                    // `GREEN` -- the members are one family of constants read in
                    // the order they are written.
                    self.define(
                        member.name.clone(),
                        DefKind::Const,
                        member.name_span,
                        member.span,
                        Some(item.node),
                    );
                }
                self.pop_scope();
            }
            ItemKind::Function(function) => {
                self.push_scope();
                // The template parameters are in scope inside the body, which
                // is what lets `T` be used as a type name there. They are bound
                // as a template parameter rather than as a value, because what
                // they stand for depends on the instantiation.
                for param in &function.template_params {
                    self.define(
                        param.name.clone(),
                        DefKind::TemplateParam,
                        param.name_span,
                        param.span,
                        None,
                    );
                }
                // Parameters are bound in the function's own scope.
                for param in &function.params {
                    self.define(
                        param.name.clone(),
                        DefKind::Param,
                        param.name_span,
                        param.span,
                        None,
                    );
                }
                // The `except` condition is an expression written against the
                // parameters -- `fn save(amount int) void except (amount < 0)`
                // -- so it is resolved in the same scope as the body. Without
                // this its names were never resolved at all, and a checker
                // lookup fell back to the first definition of the name in the
                // program: a parameter of the same name in another function.
                if let Some(throws) = &function.throws
                    && let Some(condition) = throws.condition
                {
                    self.resolve_expr(ast, condition);
                }
                if let Some(body) = function.body
                    && (!self.declarations_only || self.is_template_member(function))
                {
                    self.resolve_block(ast, body);
                }
                self.pop_scope();
            }
            // An import has no body: the loader brings the module's exported
            // names into scope before resolution runs.
            ItemKind::Import(_) => {}
            // A link instruction has no body either.
            ItemKind::Link(_) => {}
            // A type declaration's fields carry types, which are resolved by
            // the checker rather than as name occurrences here. Its constant
            // members carry *values*, which are expressions: a name in one is a
            // name to resolve like any other, in a scope where the declaration's
            // template parameters are visible.
            ItemKind::TypeDeclaration(declaration) => {
                self.push_scope();
                for param in &declaration.template_params {
                    self.define(
                        param.name.clone(),
                        DefKind::TemplateParam,
                        param.name_span,
                        param.span,
                        None,
                    );
                }
                for constant in &declaration.constants {
                    self.resolve_expr(ast, constant.value);
                }
                self.pop_scope();
            }
            // A trait's requirements are expressions written against its
            // parameters, so the parameters are in scope while they are
            // resolved. They are ordinary locals as far as name resolution is
            // concerned: whether the type they stand for has the operation is
            // the checker's question, not this pass's.
            ItemKind::Trait(declaration) => {
                self.push_scope();
                for param in &declaration.params {
                    self.define(
                        param.name.clone(),
                        DefKind::Param,
                        param.name_span,
                        param.name_span,
                        None,
                    );
                }
                // The type variables the parameters are written against are
                // names too: a requirement may state a parameter's type --
                // `it.push(Iter)` -- and that is a use of `Iter`.
                for variable in declaration.type_variables() {
                    self.define(
                        variable.clone(),
                        DefKind::TemplateParam,
                        declaration.name_span,
                        declaration.name_span,
                        None,
                    );
                }
                for requirement in &declaration.requirements {
                    self.resolve_expr(ast, requirement.expr);
                }
                self.pop_scope();
            }
            ItemKind::Unresolved(_) => {}
        }
    }

    /// Resolves every name occurrence in a block.
    fn resolve_block(&mut self, ast: &Ast, block: fallc_ast::BlockId) {
        for &stmt in &ast.block(block).statements {
            self.resolve_stmt(ast, stmt);
        }
    }

    /// Resolves whichever branch follows an `if`.
    fn resolve_else(&mut self, ast: &Ast, branch: &Option<fallc_ast::ElseBranch>) {
        match branch {
            Some(fallc_ast::ElseBranch::Block(block)) => self.scoped_block(ast, *block),
            Some(fallc_ast::ElseBranch::If(stmt)) => self.resolve_stmt(ast, *stmt),
            None => {}
        }
    }

    /// Folds a condition made only of literals, or `None` when it is not.
    ///
    /// Deliberately small: enough to decide `if (true)`, `if (0)`, and
    /// `if (1 == 2)`, which is what a hand-written configuration switch looks
    /// like. Anything naming a value -- a constant, a template parameter -- is
    /// left to the checker, which is the stage that can answer.
    fn literal_condition(&self, ast: &Ast, expression: fallc_ast::ExprId) -> Option<bool> {
        use fallc_ast::{BinaryOp, ExprKind, UnaryOp};
        let value = |expr: fallc_ast::ExprId| self.literal_int(ast, expr);
        match &ast.expr(expression).kind {
            ExprKind::BoolLiteral(value) => Some(*value),
            ExprKind::IntLiteral(_) => value(expression).map(|n| n != 0),
            ExprKind::Unary(unary) if unary.op == UnaryOp::Not => {
                value(unary.operand).map(|n| n == 0)
            }
            // Arithmetic, so that `if (1 + 1 == 2)` is decided here rather
            // than reaching the checker with both branches unresolved.
            ExprKind::Binary(binary)
                if matches!(
                    binary.op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Rem
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                ) =>
            {
                value(expression).map(|n| n != 0)
            }
            ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
                value(unary.operand).map(|n| n != 0)
            }
            ExprKind::Binary(binary) => {
                let lhs = value(binary.lhs)?;
                match binary.op {
                    // Short-circuit, so the operand that is not needed is not
                    // evaluated -- it may be the one that does not fold.
                    BinaryOp::LogicalAnd => {
                        if lhs == 0 {
                            Some(false)
                        } else {
                            value(binary.rhs).map(|n| n != 0)
                        }
                    }
                    BinaryOp::LogicalOr => {
                        if lhs != 0 {
                            Some(true)
                        } else {
                            value(binary.rhs).map(|n| n != 0)
                        }
                    }
                    op => {
                        let rhs = value(binary.rhs)?;
                        Some(match op {
                            BinaryOp::Eq => lhs == rhs,
                            BinaryOp::Ne => lhs != rhs,
                            BinaryOp::Lt => lhs < rhs,
                            BinaryOp::Le => lhs <= rhs,
                            BinaryOp::Gt => lhs > rhs,
                            BinaryOp::Ge => lhs >= rhs,
                            _ => return None,
                        })
                    }
                }
            }
            _ => None,
        }
    }

    /// The value of an integer literal, or `None` for anything else.
    fn literal_int(&self, ast: &Ast, expression: fallc_ast::ExprId) -> Option<i64> {
        use fallc_ast::{BinaryOp, ExprKind, UnaryOp};
        match &ast.expr(expression).kind {
            ExprKind::IntLiteral(literal) => fallc_ast::literal::decode_int_value(&literal.digits),
            ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
                self.literal_int(ast, unary.operand).map(|n| -n)
            }
            ExprKind::Unary(unary) if unary.op == UnaryOp::BitNot => {
                self.literal_int(ast, unary.operand).map(|n| !n)
            }
            ExprKind::Binary(binary) => {
                let lhs = self.literal_int(ast, binary.lhs)?;
                let rhs = self.literal_int(ast, binary.rhs)?;
                match binary.op {
                    BinaryOp::Add => lhs.checked_add(rhs),
                    BinaryOp::Sub => lhs.checked_sub(rhs),
                    BinaryOp::Mul => lhs.checked_mul(rhs),
                    BinaryOp::Div => lhs.checked_div(rhs),
                    BinaryOp::Rem => lhs.checked_rem(rhs),
                    BinaryOp::BitAnd => Some(lhs & rhs),
                    BinaryOp::BitOr => Some(lhs | rhs),
                    BinaryOp::BitXor => Some(lhs ^ rhs),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn resolve_stmt(&mut self, ast: &Ast, id: fallc_ast::StmtId) {
        match &ast.stmt(id).kind {
            StmtKind::Expr(node) => self.resolve_expr(ast, node.expr),
            StmtKind::StaticAssert(node) => self.resolve_expr(ast, node.condition),
            StmtKind::Bind(node) => {
                // The initialiser is resolved before the name is bound, so
                // `x int = x;` refers to an outer `x` rather than to itself.
                // Every form of initialiser is walked, not only a plain
                // expression: `x T = .{ f() };` has names inside its elements,
                // and skipping them left those names unresolved -- so a use of
                // a local was looked up by name and found a parameter of some
                // other function instead.
                match &node.init {
                    Some(fallc_ast::Initializer::Expr(expr)) => self.resolve_expr(ast, *expr),
                    Some(fallc_ast::Initializer::Designated(init)) => {
                        for &element in &init.elements {
                            self.resolve_expr(ast, element);
                        }
                    }
                    None => {}
                }
                self.define(
                    node.name.clone(),
                    DefKind::Local,
                    node.name_span,
                    node.ty.span(),
                    None,
                );
            }
            StmtKind::Return(node) => {
                if let Some(value) = node.value {
                    self.resolve_expr(ast, value);
                }
            }
            StmtKind::Raise(node) => {
                for expr in [node.exception_type, node.message].into_iter().flatten() {
                    self.resolve_expr(ast, expr);
                }
            }
            StmtKind::If(node) => {
                self.resolve_expr(ast, node.condition);
                if node.is_constexpr {
                    // `if constexpr` exists so that the branch not taken may
                    // name things this program does not have -- a type that
                    // only exists for some arguments, or a function that is not
                    // declared for this configuration. So the branch that is
                    // not taken is left unresolved.
                    //
                    // Which branch that is may not be knowable yet: the
                    // condition can name a template parameter, and resolution
                    // runs before the substitution exists. Where it *is*
                    // knowable, the dead branch is skipped outright; where it
                    // is not, both are walked with unresolvable names tolerated
                    // and the checker drops one afterwards.
                    match self.literal_condition(ast, node.condition) {
                        Some(true) => self.scoped_block(ast, node.then_block),
                        Some(false) => self.resolve_else(ast, &node.else_branch),
                        None => {
                            self.lenient += 1;
                            self.scoped_block(ast, node.then_block);
                            self.resolve_else(ast, &node.else_branch);
                            self.lenient -= 1;
                        }
                    }
                    return;
                }
                // A plain `if` checks both branches, so both are resolved. A
                // constant condition does not change that: what it changes is
                // only which branch is *emitted*.
                self.scoped_block(ast, node.then_block);
                self.resolve_else(ast, &node.else_branch);
            }
            StmtKind::While(node) => {
                self.resolve_expr(ast, node.condition);
                self.scoped_block(ast, node.body);
                if let Some(block) = node.else_block {
                    self.scoped_block(ast, block);
                }
            }
            StmtKind::ForC(node) => {
                // The whole `for` is one scope, so the loop variable a `for`
                // introduces does not outlive it: two loops in one function may
                // each declare `i`.
                self.push_scope();
                if let Some(init) = node.init {
                    self.resolve_stmt(ast, init);
                }
                if let Some(condition) = node.condition {
                    self.resolve_expr(ast, condition);
                }
                if let Some(update) = node.update {
                    self.resolve_expr(ast, update);
                }
                self.scoped_block(ast, node.body);
                if let Some(block) = node.else_block {
                    self.scoped_block(ast, block);
                }
                self.pop_scope();
            }
            StmtKind::Switch(node) => {
                self.resolve_expr(ast, node.scrutinee);
                for arm in &node.arms {
                    if let Some(labels) = &arm.labels {
                        for &label in labels {
                            self.resolve_expr(ast, label);
                        }
                    }
                    for &stmt in &arm.body {
                        self.resolve_stmt(ast, stmt);
                    }
                }
            }
            StmtKind::Try(node) => {
                for clause in &node.except_clauses {
                    for &name in &clause.names {
                        self.resolve_expr(ast, name);
                    }
                }
                self.scoped_block(ast, node.body);
                for clause in &node.except_clauses {
                    self.scoped_block(ast, clause.block);
                }
                for block in [node.else_block, node.finally_block]
                    .into_iter()
                    .flatten()
                {
                    self.scoped_block(ast, block);
                }
            }
            StmtKind::Block(block) => self.scoped_block(ast, *block),
            StmtKind::Defer(block) => self.scoped_block(ast, *block),
            StmtKind::Fallthrough | StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Unresolved(_) => {}
        }
    }

    /// Resolves a block in a fresh scope.
    fn scoped_block(&mut self, ast: &Ast, block: fallc_ast::BlockId) {
        self.push_scope();
        self.resolve_block(ast, block);
        self.pop_scope();
    }

    /// Resolves the names inside an expression.
    fn resolve_expr(&mut self, ast: &Ast, id: fallc_ast::ExprId) {
        let expr = ast.expr(id);
        match &expr.kind {
            // `sizeof(T)` and `alignof(T)` name a type, which is not a name to
            // resolve: it is looked up in the type namespace, and the type
            // checker does that.
            ExprKind::Sizeof(_) | ExprKind::Alignof(_) => {}
            // A fold names a pack; the name is resolved by the checker, which
            // is what knows the arguments.
            ExprKind::Fold(_) => {}
            // `sizeof...` counts a pack, which is the checker's question too --
            // but a *function* parameter pack is also a parameter of this
            // function, and counting it is reading it, so the binding is
            // recorded and the unused-parameter warning sees a use. A template
            // parameter pack has no definition in scope and the lookup finds
            // nothing, which is not an error here.
            ExprKind::SizeofPack(reference) => {
                if let Some(definition) = self.lookup(&reference.pack)
                    && self.resolutions.def(definition).is_some_and(|def| def.kind == DefKind::Param)
                {
                    self.resolutions.bind(reference.pack_span, definition);
                }
            }
            // A pack expansion and a comma fold name the enclosing function's
            // pack *and* contain an expression, so the expression is resolved
            // like any other; which names are elements is the checker's
            // question, answered when the function is instantiated.
            ExprKind::PackExpand(inner) => self.resolve_expr(ast, *inner),
            ExprKind::CommaFold { pattern } => self.resolve_expr(ast, *pattern),
            ExprKind::Propagate(node) => self.resolve_expr(ast, node.value),
            ExprKind::Abort(node) => self.resolve_expr(ast, node.value),
            ExprKind::ThrowQuery(inner) => self.resolve_expr(ast, *inner),
            // `type(expr)` names the type of an expression: the expression is
            // still an expression, so its names resolve as usual.
            ExprKind::TypeOf(inner) => self.resolve_expr(ast, *inner),
            // A written type is a type: nothing in it is a value to resolve.
            ExprKind::TypeLiteral(_) => {}
            ExprKind::Conditional(node) => {
                self.resolve_expr(ast, node.condition);
                self.resolve_expr(ast, node.then_value);
                self.resolve_expr(ast, node.else_value);
            }
            fallc_ast::ExprKind::Name(name) => {
                // A *type* name is not a value, so it is not resolved as one:
                // `Array<int>()` names a type to construct, and the type
                // namespace is the checker's. Resolving it here reported every
                // construction as an unknown name.
                if !self.known_types.contains(name) {
                    self.resolve_name(name, expr.span);
                }
            }
            // A cast's type is written, not resolved as a name occurrence, so
            // only its operand can contain names.
            fallc_ast::ExprKind::Cast(cast) => {
                self.resolve_expr(ast, cast.operand);
            }
            fallc_ast::ExprKind::Binary(node) => {
                self.resolve_expr(ast, node.lhs);
                self.resolve_expr(ast, node.rhs);
            }
            fallc_ast::ExprKind::Unary(node) => {
                self.resolve_expr(ast, node.operand);
            }
            fallc_ast::ExprKind::IncDec(node) => {
                self.resolve_expr(ast, node.operand);
            }
            fallc_ast::ExprKind::Comma(node) => {
                self.resolve_expr(ast, node.lhs);
                self.resolve_expr(ast, node.rhs);
            }
            fallc_ast::ExprKind::Assign(node) => {
                self.resolve_expr(ast, node.target);
                self.resolve_expr(ast, node.value);
            }
            fallc_ast::ExprKind::Call(node) => {
                // A callee that names a *type* is not a value to resolve:
                // `Array<int>()` constructs, and the type is the checker's to
                // look up. Only a callee that names a value is resolved.
                let names_a_type = match &ast.expr(node.callee).kind {
                    fallc_ast::ExprKind::Name(name) => {
                        self.known_types.contains(name) || self.known_traits.contains(name)
                    }
                    // A trait naming a module: `std.InputIterator(x)` is a
                    // question about a type, and the module is only part of the
                    // name. Without this the field was looked up among the
                    // module's members, which no trait takes.
                    //
                    // A type named through a dotted module path --
                    // `std.string.String("hi")` -- is the same case: the whole
                    // path is the type's name, and resolving it as a value
                    // would report the last segment as a method of the module.
                    fallc_ast::ExprKind::Field(field) => {
                        self.known_traits.contains(&field.field)
                            || written_path(ast, node.callee)
                                .is_some_and(|path| self.known_types.contains(&path))
                    }
                    _ => false,
                };
                if !names_a_type {
                    self.resolve_expr(ast, node.callee);
                }
                for &arg in &node.args {
                    self.resolve_expr(ast, arg);
                }
                // A named argument is an argument: its value names things too,
                // and a variable used only there is used.
                for argument in &node.named {
                    self.resolve_expr(ast, argument.value);
                }
            }
            // Both operands are ordinary expressions: indexing an array is not
            // a field access, and `__getitem__` is looked up on the type rather
            // than by name.
            fallc_ast::ExprKind::Index(node) => {
                self.resolve_expr(ast, node.object);
                self.resolve_expr(ast, node.index);
            }
            fallc_ast::ExprKind::TypedInit(node) => {
                for &element in &node.init.elements {
                    self.resolve_expr(ast, element);
                }
                for argument in &node.init.named {
                    self.resolve_expr(ast, argument.value);
                }
            }
            fallc_ast::ExprKind::NestedInit(init) => {
                for &element in &init.elements {
                    self.resolve_expr(ast, element);
                }
                for argument in &init.named {
                    self.resolve_expr(ast, argument.value);
                }
            }
            fallc_ast::ExprKind::Field(node) => {
                // `C.printf` and `p.show` are written identically, so the
                // object is checked against the module table first: if it names
                // a module, the member is a symbol of that module rather than a
                // field of a value.
                //
                // A value of the same name shadows the module: `node->value`
                // where a local is called `node` is a field of that local, and
                // reading it as `node.value` of a module reported the field as
                // a missing export.
                if let fallc_ast::ExprKind::Name(object) = &ast.expr(node.object).kind
                    && self.external.contains_key(object.as_str())
                    && self.lookup(object).is_none()
                {
                    let qualified = format!("{object}.{}", node.field);
                    self.resolve_name(&qualified, expr.span);
                    return;
                }
                // Otherwise the object is a value, and the field is looked up
                // in its type rather than in scope.
                self.resolve_expr(ast, node.object);
            }
            // A lambda's parameters are a scope of its own, and its body is
            // resolved inside it: a name that is not a parameter resolves in
            // the enclosing scopes, which is what makes it a capture.
            fallc_ast::ExprKind::Lambda(lambda) => {
                self.push_scope();
                for parameter in &lambda.parameters {
                    self.define(
                        parameter.name.clone(),
                        DefKind::Param,
                        parameter.name_span,
                        parameter.span,
                        None,
                    );
                }
                match &lambda.body {
                    fallc_ast::LambdaBody::Expression(value) => self.resolve_expr(ast, *value),
                    fallc_ast::LambdaBody::Block(block) => self.resolve_block(ast, *block),
                }
                self.pop_scope();
            }
            // A null pointer names nothing.
            fallc_ast::ExprKind::NullPointer => {}
            // Literals name nothing.
            fallc_ast::ExprKind::IntLiteral(_)
            | fallc_ast::ExprKind::CharLiteral(_)
            | fallc_ast::ExprKind::FloatLiteral(_)
            | fallc_ast::ExprKind::StringLiteral(_)
            | fallc_ast::ExprKind::ByteStringLiteral(_)
            | fallc_ast::ExprKind::RawStringLiteral(_)
            | fallc_ast::ExprKind::BoolLiteral(_) => {}
            // An unresolved expression names nothing.
            fallc_ast::ExprKind::Unresolved(_) => {}
        }
    }

    /// Pushes a new scope whose parent is the current innermost scope.
    pub fn push_scope(&mut self) -> ScopeId {
        let id = ScopeId(u32::try_from(self.scopes.len()).expect("scope count exceeds u32::MAX"));
        let parent = id.0.checked_sub(1).map(ScopeId);
        self.scopes.push(Scope {
            parent,
            names: HashMap::new(),
        });
        id
    }

    /// Pops the innermost scope, returning its id.
    ///
    /// The root scope cannot be popped; popping it is a no-op returning
    /// `ScopeId::ROOT`.
    pub fn pop_scope(&mut self) -> ScopeId {
        if self.scopes.len() <= 1 {
            return ScopeId::ROOT;
        }
        let id = ScopeId(self.scopes.len() as u32 - 1);
        self.scopes.pop();
        id
    }

    /// Returns the innermost scope.
    #[must_use]
    pub fn current_scope(&self) -> ScopeId {
        ScopeId(self.scopes.len() as u32 - 1)
    }

    /// Introduces `name` in the innermost scope.
    ///
    /// Reports a duplicate-definition error and returns the existing definition
    /// if the name is already bound in *this* scope. A declaration in an inner
    /// scope shadows an outer one of the same name; a local or parameter that
    /// hides another name of the same function is warned about, because that is
    /// the case a reader is most likely to have written by accident.
    pub fn define(
        &mut self,
        name: impl Into<String>,
        kind: DefKind,
        name_span: Span,
        decl_span: Span,
        node: Option<NodeId>,
    ) -> DefId {
        let name = name.into();
        let scope = self.current_scope();
        // A local or a parameter that hides one of the *same function's* other
        // names is a well-known source of mistakes: the inner declaration is
        // easy to write when the outer one was meant. Module-level names are
        // left out, because a local named after a function or a constant is
        // ordinary.
        if matches!(kind, DefKind::Local | DefKind::Param)
            && name != "this"
            && !name.starts_with('_')
        {
            self.warn_shadowed(&name, name_span);
        }

        // `_` may be declared again in the same scope. It is one name, so a
        // second `_` shadows the first the way an inner scope shadows an outer
        // one -- reading `_` gives the most recent declaration, and that
        // declaration's scope runs to the end of the block or to the next `_`,
        // whichever comes first. Every declaration is still its own variable:
        // each has storage and each is destroyed at the end of the scope.
        if name == "_" {
            let id = DefId(
                u32::try_from(self.resolutions.defs.len())
                    .expect("definition count exceeds u32::MAX"),
            );
            self.defs.push(None);
            self.resolutions.defs.push(Def {
                id,
                kind,
                name,
                node,
                name_span,
                span: decl_span,
                scope,
                signature: None,
                emitted_as: None,
                imported_as: None,
                imported_via: None,
                is_builtin: false,
            });
            if let Some(scope) = self.scopes.last_mut() {
                scope.names.insert("_".to_owned(), vec![id]);
            }
            return id;
        }

        // A name may already be taken. Functions overload, so a second one
        // joins the first as a candidate; anything else is a genuine duplicate.
        if let Some(existing) = self.scopes.last().and_then(|s| s.names.get(&name)) {
            let existing = existing.clone();
            let first = existing
                .first()
                .copied()
                .expect("a bound name has at least one definition");
            let first_kind = self
                .resolutions
                .def(first)
                .map_or(DefKind::Unspecified, |def| def.kind);

            // Only two functions may share a name, and only when their
            // parameter types differ: the return type is not part of a
            // signature, so two functions differing only in it could never be
            // told apart at a call.
            let overloads = kind == DefKind::Function && first_kind == DefKind::Function;
            if overloads {
                let id = DefId(
                    u32::try_from(self.resolutions.defs.len())
                        .expect("definition count exceeds u32::MAX"),
                );
                self.defs.push(None);
                self.resolutions.defs.push(Def {
                    id,
                    kind,
                    name: name.clone(),
                    node,
                    name_span,
                    span: decl_span,
                    scope,
                    signature: None,
                    emitted_as: None,
                    imported_as: None,
                    imported_via: None,
                    is_builtin: false,
                });
                if let Some(scope) = self.scopes.last_mut()
                    && let Some(defs) = scope.names.get_mut(&name)
                {
                    defs.push(id);
                }
                return id;
            }

            self.diagnostics.push(
                Diagnostic::error(format!("`{name}` is defined more than once"))
                    .with_code(DiagnosticCode::sema(1))
                    .with_span(name_span)
                    .with_label(
                        self.resolutions
                            .def(first)
                            .map_or(Span::SYNTHETIC, |d| d.name_span),
                        "first definition is here",
                    ),
            );
            return first;
        }

        let id = DefId(
            u32::try_from(self.resolutions.defs.len()).expect("definition count exceeds u32::MAX"),
        );
        self.defs.push(None);
        self.resolutions.defs.push(Def {
            id,
            kind,
            name: name.clone(),
            node,
            name_span,
            span: decl_span,
            scope,
            signature: None,
            emitted_as: None,
            imported_as: None,
            imported_via: None,
            is_builtin: false,
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.names.insert(name, vec![id]);
        }
        id
    }

    /// Looks `name` up through the scope stack, returning the first definition.
    ///
    /// Used where a name must denote exactly one thing. A call uses
    /// [`Resolver::lookup_all`] instead, because a name may have several
    /// overloads.
    /// Warns when `name`, about to be declared in the current scope, hides a
    /// local or a parameter of an enclosing scope.
    ///
    /// The root scope is skipped: it holds the module's own declarations and
    /// the imports, and a local may reasonably be called `printf` or `Array`.
    fn warn_shadowed(&mut self, name: &str, span: Span) {
        let mut scope = self.scopes.last().and_then(|scope| scope.parent);
        while let Some(id) = scope {
            if id.0 == 0 {
                return;
            }
            let found = self
                .scopes
                .get(id.0 as usize)
                .and_then(|scope| scope.names.get(name))
                .and_then(|defs| defs.first())
                .copied();
            if let Some(def) = found
                && let Some(definition) = self.resolutions.defs.get(def.0 as usize)
                && matches!(definition.kind, DefKind::Local | DefKind::Param)
            {
                let original = definition.name_span;
                self.diagnostics.push(
                    Diagnostic::warning(format!("`{name}` shadows an outer binding"))
                        .with_code(DiagnosticCode::sema(143))
                        .with_warning("shadow")
                        .with_span(span)
                        .with_label(original, "the binding it hides")
                        .with_note(
                            "the inner declaration hides the outer one for the rest of its \
                             block: rename one of them, or read the outer one before it",
                        ),
                );
                return;
            }
            scope = self.scopes.get(id.0 as usize).and_then(|scope| scope.parent);
        }
    }

    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<DefId> {
        self.lookup_all(name).first().copied()
    }

    /// Looks `name` up through the scope stack, returning every definition.
    ///
    /// The innermost scope that binds the name wins entirely: an inner
    /// declaration hides the outer ones rather than adding to them, which is
    /// what shadowing means.
    #[must_use]
    pub fn lookup_all(&self, name: &str) -> Vec<DefId> {
        let mut scope = Some(self.current_scope());
        while let Some(id) = scope {
            let Some(entry) = self.scopes.get(id.0 as usize) else {
                return Vec::new();
            };
            if let Some(defs) = entry.names.get(name) {
                return defs.clone();
            }
            scope = entry.parent;
        }
        Vec::new()
    }

    /// Resolves a name occurrence, reporting an error if it is unknown.
    ///
    /// A qualified name is a module member. The diagnostic names the missing
    /// name; there is no "did you mean" suggestion.
    pub fn resolve_name(&mut self, name: &str, span: Span) -> Option<DefId> {
        // A qualified name is a module member: `C.printf`.
        if let Some((module, member)) = name.split_once('.') {
            return self.resolve_external(module, member, span);
        }
        match self.lookup(name) {
            Some(def) => {
                self.resolutions.bind(span, def);
                Some(def)
            }
            None => {
                // Inside a compile-time `if` whose branch is not yet chosen,
                // an unknown name is not an error: this may be the branch that
                // is discarded. The checker reports anything left unresolved
                // once the branch is known.
                if self.lenient == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(format!("cannot find `{name}` in this scope"))
                            .with_code(DiagnosticCode::sema(2))
                            .with_span(span),
                    );
                }
                None
            }
        }
    }

    /// Registers what a module exports.
    pub fn add_module(&mut self, name: impl Into<String>, module: ExternalModule) {
        self.external.insert(name.into(), module);
    }

    /// Returns what a module exports, by its own name.
    ///
    /// Used by the loader to register a module alias without loading the module
    /// a second time.
    #[must_use]
    pub fn module(&self, name: &str) -> Option<&ExternalModule> {
        self.external.get(name)
    }

    /// Records one name an import introduces into this file's scope.
    ///
    /// Called by the module loader, which is the part that knows what a module
    /// exports and therefore which names exist to be bound.
    pub fn add_import(&mut self, binding: ImportBinding) {
        self.imports.push(binding);
    }

    /// Installs every recorded import binding in the root scope.
    ///
    /// # Why this happens before resolution
    ///
    /// An imported name has to be findable by the same lookup every local name
    /// uses. Installing it in the outermost scope means the three import
    /// spellings need no special case anywhere downstream: after this,
    /// `printf` from `from C import printf` and `printf` declared in this file
    /// are the same kind of definition, and the checker, the code generator and
    /// the diagnostics cannot tell them apart.
    ///
    /// A plain `import C` binds no bare name: `C` is a module name, and
    /// `C.printf` is resolved as a qualified name by [`Self::resolve_name`].
    fn apply_imports(&mut self) {
        let imports = std::mem::take(&mut self.imports);
        for binding in imports {
            let Some(member) = binding.member.clone() else {
                // A plain `import M;` binds no bare name of its own, but its
                // members must still be *defined*, because a special method is
                // reached by its bound name rather than through the module:
                // `String s = "hi";` looks for `__init__String`, not for
                // `std.__init__String`. Without this, no imported type could be
                // constructed at all.
                if let Some(module) = self.external.get(&binding.module).cloned() {
                    let names: Vec<String> = module.symbols.keys().cloned().collect();
                    for name in names {
                        let _ = self.resolve_external(&binding.module, &name, binding.span);
                    }
                }
                continue;
            };
            // A named import binds the member under its own name, which is
            // what makes a bare `printf` a call: the checker finds candidates
            // by definition name, so an alias that only added a scope entry
            // would resolve the name and then fail to find a function.
            self.resolve_external_as(&binding.module, &member, &binding.local, binding.span);
        }
    }

    /// Returns the local names this unit re-exports from its imports.
    ///
    /// A `from M import *` forwards what it brought in, which is what lets an
    /// umbrella module be assembled out of other modules. Each name is returned
    /// with the member of this module it is published as.
    #[must_use]
    pub fn reexported_imports(&self) -> Vec<(String, String)> {
        self.imports
            .iter()
            .filter(|binding| binding.reexported)
            .filter_map(|binding| {
                binding
                    .member
                    .clone()
                    .map(|member| (binding.local.clone(), member))
            })
            .collect()
    }

    /// Resolves `module.member`, binding it as a definition in this unit.
    ///
    /// # Why an imported symbol becomes a definition
    ///
    /// Later passes look symbols up by `DefId`, and giving an imported function
    /// its own definition here means the type checker and code generator need
    /// no special case for external symbols: it is a function with a signature,
    /// exactly like a local one.
    fn resolve_external(&mut self, module: &str, member: &str, span: Span) -> Option<DefId> {
        // The definition's name is the qualified path, so that `C.printf` and
        // the module it came from agree.
        let qualified = format!("{module}.{member}");
        self.resolve_external_as(module, member, &qualified, span)
    }

    /// Resolves `module.member` under a chosen local name.
    ///
    /// The local name is what later passes look the definition up by, so a
    /// `from C import printf` defines `printf` while still remembering that the
    /// linker symbol is the module's own.
    fn resolve_external_as(
        &mut self,
        module: &str,
        member: &str,
        local: &str,
        span: Span,
    ) -> Option<DefId> {
        let Some(external) = self.external.get(module) else {
            self.diagnostics.push(
                Diagnostic::error(format!("cannot find module `{module}`"))
                    .with_code(DiagnosticCode::sema(3))
                    .with_span(span)
                    .with_note("add `import` for it at the top of the file"),
            );
            return None;
        };

        // Cloned so the borrow of `self.external` ends before `self` is
        // mutated below. Every overload of the member is taken: which one a
        // call means is decided by its arguments, and that decision needs all
        // of them.
        // A module member may be a *type* rather than a function. `std.Array`
        // is how an umbrella user names one, and the type is what the checker
        // resolves -- so it is accepted here and bound as a type definition.
        // Without this the resolver reported "does not export" for a name whose
        // module does export it.
        let signatures = external.symbols.get(member).cloned();
        let Some(signatures) = signatures else {
            // A constant is the third kind of module member: `C.EOF` names a
            // value rather than a function or a type.
            if let Some(value) = external.constants.get(member).copied() {
                // A member used twice is one member. `C.EOF` in one place and
                // again in another named the same constant, and defining it a
                // second time reported a duplicate declaration.
                if let Some(existing) = self.imported_member(local, member, module) {
                    self.resolutions.bind(span, existing);
                    return Some(existing);
                }
                self.constants_by_name
                    .insert(local.to_owned(), value);
                let def = self.define(local.to_owned(), DefKind::Const, span, span, None);
                self.resolutions.bind(span, def);
                return Some(def);
            }
            // A module-level variable the module exported, or an `extern` one:
            // both are storage owned by another module, so the importer gets a
            // definition marked with where it came from and with the symbol to
            // read. The *type* is registered by the driver -- resolved for a
            // global, written and resolved here for an `extern` one -- so the
            // definition needs nothing more.
            let symbol = external
                .globals
                .get(member)
                .map(|global| global.symbol.clone())
                .or_else(|| {
                    external
                        .variables
                        .get(member)
                        .map(|variable| variable.symbol.clone())
                });
            if let Some(symbol) = symbol {
                if let Some(existing) = self.imported_member(local, member, module) {
                    self.resolutions.bind(span, existing);
                    return Some(existing);
                }
                let def = self.define(local.to_owned(), DefKind::Local, span, span, None);
                if let Some(entry) = self.resolutions.defs.get_mut(def.as_raw() as usize) {
                    entry.imported_as = Some(member.to_owned());
                    entry.imported_via = Some(module.to_owned());
                    entry.emitted_as = Some(symbol);
                }
                self.resolutions.bind(span, def);
                return Some(def);
            }
            // A type, an enum, an interface or a trait: all four are names in
            // the type namespace, and a program names them the same way.
            if external.types.contains_key(member)
                || external.enums.contains_key(member)
                || external.interfaces.contains_key(member)
                || external.traits.contains(member)
            {
                if let Some(existing) = self.imported_member(local, member, module) {
                    self.resolutions.bind(span, existing);
                    return Some(existing);
                }
                let def = self.define(local.to_owned(), DefKind::Type, span, span, None);
                if let Some(entry) = self.resolutions.defs.get_mut(def.as_raw() as usize) {
                    entry.imported_as = Some(member.to_owned());
                    entry.imported_via = Some(module.to_owned());
                }
                self.resolutions.bind(span, def);
                return Some(def);
            }
            self.diagnostics.push(
                Diagnostic::error(format!("module `{module}` does not export `{member}`"))
                    .with_code(DiagnosticCode::sema(4))
                    .with_span(span)
                    .with_note("a module's declarations are private unless marked `export`"),
            );
            return None;
        };

        // Every overload becomes its own definition, all sharing the local
        // name. One definition per signature is what lets a call be resolved
        // by its arguments.
        //
        // A signature already defined is skipped: two imports of the same
        // module -- `import C;` and `from C import printf;` -- must agree, and
        // defining `printf` twice made every call to it ambiguous.
        //
        // The search covers every definition of the name, not only the ones
        // still in scope. A qualified name is resolved separately at each site
        // that writes it, and a scope that has since been popped no longer
        // answers `lookup`, so the same signature was defined once per use --
        // which read as a pile of identical overloads and made every call to a
        // library function ambiguous.
        let mut first = None;
        for signature in signatures {
            if let Some(index) = self
                .resolutions
                .defs
                .iter()
                .position(|entry| {
                    entry.name == local && entry.signature.as_ref() == Some(&signature)
                })
            {
                first.get_or_insert(DefId(u32::try_from(index).expect("def count exceeds u32::MAX")));
                continue;
            }
            let def = self.define(local.to_owned(), DefKind::Function, span, span, None);
            self.set_signature(def, signature);
            if let Some(entry) = self.resolutions.defs.get_mut(def.as_raw() as usize) {
                // Recorded so code generation knows the LLVM symbol to call: an
                // imported function keeps the name it was declared with,
                // without the module prefix.
                entry.imported_as = Some(member.to_owned());
                entry.imported_via = Some(module.to_owned());
            }
            first.get_or_insert(def);
        }
        let def = first?;
        self.resolutions.bind(span, def);
        Some(def)
    }

    /// Returns the definition a module member was already bound to, if any.
    ///
    /// A member reached through a module is bound once per local name: the
    /// imports are a set of names, and a name that already stands for
    /// `C.stdin` does not become a second definition because it was written
    /// twice. A constant, a variable and a type are all looked up this way;
    /// functions have their own rule, since they overload.
    fn imported_member(&self, local: &str, member: &str, module: &str) -> Option<DefId> {
        let def = self.lookup(local)?;
        let entry = self.resolutions.def(def)?;
        (entry.imported_as.as_deref() == Some(member)
            && entry.imported_via.as_deref() == Some(module))
        .then_some(def)
    }

    /// Attaches a signature to an already-declared definition.
    pub fn set_signature(&mut self, def: DefId, signature: FunctionSignature) {
        if let Some(slot) = self.defs.get_mut(def.as_raw() as usize) {
            *slot = Some(signature.clone());
        }
        if let Some(entry) = self.resolutions.defs.get_mut(def.as_raw() as usize) {
            entry.signature = Some(signature);
        }
    }

    /// Returns a definition's signature, if it has one.
    #[must_use]
    pub fn signature(&self, def: DefId) -> Option<&FunctionSignature> {
        self.defs.get(def.as_raw() as usize)?.as_ref()
    }

    /// Records a diagnostic produced by the resolver.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

/// The dotted path an expression writes, when it is a chain of names and
/// fields: `Array`, `std.Array`, `std.string.String`.
fn written_path(ast: &Ast, expr: fallc_ast::ExprId) -> Option<String> {
    match &ast.expr(expr).kind {
        ExprKind::Name(name) => Some(name.clone()),
        ExprKind::Field(field) => {
            let object = written_path(ast, field.object)?;
            Some(format!("{object}.{}", field.field))
        }
        _ => None,
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Resolver::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    fn span(lo: u32, hi: u32) -> Span {
        Span::new(FileId::from_raw(0), lo, hi)
    }

    #[test]
    fn define_then_lookup_in_same_scope() {
        let mut r = Resolver::new();
        let id = r.define("x", DefKind::Unspecified, span(0, 1), span(0, 1), None);
        assert_eq!(r.lookup("x"), Some(id));
        assert_eq!(r.lookup("y"), None);
    }

    #[test]
    fn inner_scope_shadows_outer() {
        let mut r = Resolver::new();
        let outer = r.define("x", DefKind::Unspecified, span(0, 1), span(0, 1), None);
        r.push_scope();
        let inner = r.define("x", DefKind::Unspecified, span(5, 6), span(5, 6), None);
        assert_ne!(outer, inner);
        assert_eq!(r.lookup("x"), Some(inner));
        r.pop_scope();
        assert_eq!(r.lookup("x"), Some(outer));
    }

    #[test]
    fn duplicate_in_same_scope_is_an_error() {
        let mut r = Resolver::new();
        let first = r.define("x", DefKind::Unspecified, span(0, 1), span(0, 1), None);
        let second = r.define("x", DefKind::Unspecified, span(5, 6), span(5, 6), None);
        assert_eq!(first, second);
        assert_eq!(r.diagnostics.len(), 1);
        assert!(r.diagnostics.has_errors());
    }

    #[test]
    fn unknown_name_is_reported_and_not_bound() {
        let mut r = Resolver::new();
        assert!(r.resolve_name("nope", span(0, 4)).is_none());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.resolutions.binding_count(), 0);
    }

    #[test]
    fn resolving_a_known_name_records_a_binding() {
        let mut r = Resolver::new();
        let def = r.define("x", DefKind::Unspecified, span(0, 1), span(0, 1), None);
        assert_eq!(r.resolve_name("x", span(10, 11)), Some(def));
        assert_eq!(r.resolutions.resolve(span(10, 11)), Some(def));
    }

    #[test]
    fn root_scope_cannot_be_popped() {
        let mut r = Resolver::new();
        assert!(r.pop_scope().is_root());
        assert_eq!(r.current_scope().as_raw(), 0);
    }

    #[test]
    fn resolve_over_empty_ast_is_ok() {
        let mut r = Resolver::new();
        let result = r.resolve(&Ast::new());
        assert!(result.is_ok());
        // The only definitions are the compiler's builtins, which are
        // registered before any source is seen.
        assert!(
            result.resolutions.defs.iter().all(|def| def.is_builtin),
            "an empty file must define nothing but builtins"
        );
        assert_eq!(
            result.resolutions.defs.len(),
            crate::builtins::BUILTINS.len()
        );
    }

    #[test]
    fn builtins_are_visible_to_source() {
        let mut r = Resolver::new();
        let _ = r.resolve(&Ast::new());
        assert!(r.lookup("__builtin_panic").is_some());
        assert!(r.lookup("nonexistent").is_none());
    }
}

/// Whether a written type names one of `names`.
fn type_mentions(
    ty: &fallc_ast::TypeSyntax,
    names: &std::collections::HashSet<String>,
) -> bool {
    use fallc_ast::{TemplateArgumentSyntax, TypeSyntax};
    match ty {
        TypeSyntax::Named(named) => {
            names.contains(&named.path)
                || named.arguments.iter().any(|argument| match argument {
                    TemplateArgumentSyntax::Type(inner) => type_mentions(inner, names),
                    TemplateArgumentSyntax::Value(_) => false,
                    TemplateArgumentSyntax::Named { path, .. } => names.contains(path),
                })
        }
        TypeSyntax::Array(array) => type_mentions(&array.element, names),
        TypeSyntax::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| type_mentions(parameter, names))
                || type_mentions(&function.returns, names)
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
        | TypeSyntax::Const(inner) => type_mentions(inner, names),
        TypeSyntax::Member(member) => type_mentions(&member.outer, names),
        TypeSyntax::TypeOf(_)
        | TypeSyntax::Auto(_)
        | TypeSyntax::DecltypeAuto(_)
        | TypeSyntax::Unresolved(_) => false,
    }
}
