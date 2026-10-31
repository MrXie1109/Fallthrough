//! Items: the declarations a file contains.
//!
//! A function, a type, an enum, an import and a link all live at the top level,
//! and each has a structure of its own; [`ItemKind`] names which one.

use super::*;

/// A top-level declaration.
///
/// Import, link instruction, type or union, enum, function, module-level
/// variable, constant, trait, and external variable. Declarations appear only at
/// file scope; `export` decides whether an importer can name one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ItemKind {
    /// `import Name;`
    Import(ImportItem),
    /// `link name dynamic from default;`
    Link(LinkItem),
    /// `type Name { field Type; ... }`, `type Name = Other;`, or
    /// `union Name { ... }`
    TypeDeclaration(TypeDecl),
    /// `enum Name Base { A, B = 3, C }`
    EnumDeclaration(EnumDecl),
    /// `fn name(params) ret_type { body }`
    ///
    /// The return type is written **postfix**, after the parameter list, like
    /// every type in the language. An absent return type means the function
    /// returns nothing, which is how a C `void` function is written.
    Function(FunctionItem),
    /// `name Type = expr;` — a module-level variable.
    ///
    /// It has storage for the whole run, `static` in C's sense, and unlike a
    /// `const` it may be assigned. Its initialiser has to be computable before
    /// the program runs, because a global is laid out with the program image;
    /// a variable whose initial value depends on run-time state is a local.
    Variable(BindStmt),
    /// `const NAME Type = value;` — a module-level constant.
    ///
    /// The value must be computable without running the program, which is what
    /// lets a constant be used where a value is needed before anything runs: an
    /// array length, a template argument, a `case` label.
    Const(ConstItem),
    /// `traits Name(it Iter) { .. }` — a named set of requirements.
    ///
    /// A trait declares no code and no storage: it says which operations a type
    /// must have, and the compiler answers whether a given type has them.
    Trait(TraitDecl),
    /// `extern "C" export NAME Type;` — a variable that lives elsewhere.
    ///
    /// It has no initialiser and no body, because its storage belongs to the
    /// library it is declared against: `stdin` is a `FILE *` in the C library,
    /// not in this program. What the declaration contributes is the name, the
    /// type it is read as, and a symbol for the linker.
    ExternVariable(ExternVariableItem),
    /// `static_assert(cond);` or `static_assert(cond, "message");`
    ///
    /// A claim the compiler checks: outside a template it is checked where it is
    /// written, and inside one when the template is instantiated -- a template
    /// that is never instantiated makes no claim.
    StaticAssert(StaticAssert),
    /// `interface Name(it T) { it.method() -> Type, .. }`
    ///
    /// A named set of restrictions about a type, declared the way a trait is.
    /// A type satisfies it structurally -- by satisfying the restrictions -- and
    /// a value of the interface type is a heap copy of a concrete value plus a
    /// table of the methods the restrictions name and type.
    Interface(InterfaceDecl),
    /// `__setup__ { .. }` -- the block a module runs before `main`.
    ///
    /// One per module at most. Every imported module's setup runs before the
    /// importing module's, and the program's own runs last, so that a module's
    /// initialisation may use whatever it imports.
    Setup(BlockId),
    /// `if (cond) @constexpr { .. } else { .. }` at the top level.
    ///
    /// Only the branch the condition selects is declared: the other is read but
    /// not checked, so it may name things this build does not have. That is
    /// what makes it a way to write a program that changes shape with its
    /// build, and it is the top-level form of the `if (c) @constexpr` that runs
    /// inside a function.
    Conditional(ConditionalItems),
    /// A top-level declaration the parser could not recognise.
    Unresolved(Unresolved),
}

/// A variable declared `extern`: storage that belongs to another library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternVariableItem {
    /// The symbol's name, which is also the name the program uses.
    pub name: String,
    /// The span of the name alone, for diagnostics.
    pub name_span: Span,
    /// The calling library's ABI, `"C"` for `extern "C"`.
    pub abi: String,
    /// The type the variable is read and written as.
    pub ty: TypeSyntax,
    /// Whether other modules may use it, from `export`.
    pub exported: bool,
    /// The whole declaration's source range.
    pub span: Span,
    /// Attributes written after the declaration: `name T @extern_C;`.
    pub attrs: Vec<super::Attr>,
}

/// A module-level constant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConstItem {
    /// Whether the declaration is marked `export`.
    pub exported: bool,
    /// The constant's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The declared type.
    pub ty: TypeSyntax,
    /// The value, which must be a constant expression.
    pub value: ExprId,
    /// The whole declaration's source range.
    pub span: Span,
    /// Attributes written after the declaration: `LIMIT const usize = 8 @export;`.
    pub attrs: Vec<super::Attr>,
}

/// `traits Name(it Iter) : Base(Iter) { ++it, *it -> !void }`
///
/// A named set of *requirements*: operations a type must have for the trait to
/// hold of it. Nothing is implemented and nothing is declared elsewhere -- a
/// type models a trait exactly when every requirement compiles for it, which is
/// why a trait is a question the compiler answers rather than a promise a
/// declaration makes.
///
/// The parameters name the values the requirements are written against, and
/// their types name the type variables. `(it Iter)` says "let `it` be a value of
/// some type `Iter`", and the requirements then say what must be true of such a
/// value. `Iter` is what the trait is *about*: asking whether a type `T` models
/// the trait substitutes `T` for `Iter`, and `it` for a place of type `T`.
///
/// ```text
/// traits InputIterator(it Iter) {
///     ++it,              // the operation exists
///     *it -> !void,      // and yields a value
///     it != it -> bool,  // and can be compared
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraitDecl {
    /// Whether the declaration is marked `export`.
    pub exported: bool,
    /// The trait's name.
    pub name: String,
    /// The span of the name alone, for diagnostics.
    pub name_span: Span,
    /// The parameters: a value of each, and the type it has.
    pub params: Vec<TraitParam>,
    /// What a type must be able to do, in source order.
    pub requirements: Vec<TraitRequirement>,
    /// Whether this is `traits Name(it Iter);` -- a declaration whose
    /// requirements arrive with the definition. Asking a query of it before
    /// then is an incomplete trait.
    pub is_forward: bool,
    /// The whole declaration's source range.
    pub span: Span,
}

impl TraitDecl {
    /// Returns the type variables, in the order they first appear.
    ///
    /// A parameter list such as `(it Iter, v Value)` declares two: `Iter` and
    /// `Value`. They are not written separately, because the parameter list
    /// already says what they are -- the types the parameters have.
    #[must_use]
    pub fn type_variables(&self) -> Vec<String> {
        let mut variables: Vec<String> = Vec::new();
        for param in &self.params {
            if let TypeSyntax::Named(named) = &param.ty
                && named.arguments.is_empty()
                && !named.path.is_empty()
                && !variables.contains(&named.path)
            {
                variables.push(named.path.clone());
            }
        }
        variables
    }
}

/// One parameter of a trait: `it Iter`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraitParam {
    /// The value's name, used inside the requirements.
    pub name: String,
    /// The span of the name alone, for diagnostics.
    pub name_span: Span,
    /// The type the value has, usually a type variable such as `Iter`.
    pub ty: TypeSyntax,
}

/// One requirement: an operation that must compile, and what its result has to
/// be.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraitRequirement {
    /// The operation, as an expression written against the parameters.
    pub expr: ExprId,
    /// What the result must be, when the requirement says so.
    pub result: Option<TraitResult>,
    /// The requirement's source text, for a diagnostic that can quote it.
    ///
    /// Kept because the text is what makes a failure readable: "the requirement
    /// `*it -> !void` is not satisfied" says which one at a glance, while a
    /// span into another file's declaration would not.
    pub text: String,
    /// The whole requirement's source range.
    pub span: Span,
}

/// What a requirement says about its result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TraitResult {
    /// `-> !void`: the expression must yield something, and not `void`.
    NotVoid(Span),
    /// `-> T`: the expression must yield exactly this type.
    Exact(TypeSyntax),
    /// `-> true`: the expression must have this *value*, known at compile time.
    ///
    /// This is the form a requirement that is itself a question takes --
    /// `InputIterator(Iter) -> true` -- because such an expression already
    /// folds to a value and what the author wants to say is that the answer is
    /// yes.
    Value(TraitValue),
}

/// The value a requirement may ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraitValue {
    /// `-> true` or `-> false`.
    Bool(bool),
    /// `-> 7`.
    Int(i64),
}

impl TraitValue {
    /// The value as the constant folder reports it.
    #[must_use]
    pub fn as_int(self) -> i64 {
        match self {
            TraitValue::Bool(true) => 1,
            TraitValue::Bool(false) => 0,
            TraitValue::Int(value) => value,
        }
    }
}

/// A type declaration.
///
/// A nominal type with named fields. There is no visibility: a field whose name
/// begins with `_` is private by convention only, and the compiler does not
/// enforce it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeDecl {
    /// The template parameters, empty for an ordinary type.
    ///
    /// `template <T type> type Array { .. }` declares a family of types.
    pub template_params: Vec<TemplateParam>,
    /// The type's name.
    pub name: String,
    /// The span of the name alone, for diagnostics.
    pub name_span: Span,
    /// Whether the declaration is marked `export`.
    pub exported: bool,
    /// The fields, in declaration order.
    ///
    /// Order matters: a positional initialiser `.{42, 37}` assigns by position,
    /// so the layout follows the source. A `union` has the same list and a
    /// different layout: its members share one address rather than following
    /// one another.
    pub fields: Vec<FieldDecl>,
    /// What kind of type this declares.
    pub kind: TypeDeclKind,
    /// Types declared inside this one, in source order.
    ///
    /// A nested type is named through its owner -- `Array<int>.Iterator` -- and
    /// it is not a field: it occupies no storage in the value and is not
    /// initialised. Declaring one inside its owner keeps the name next to the
    /// thing it belongs to, which is the whole reason to nest it.
    pub nested: Vec<TypeDecl>,
    /// The argument pattern of a template specialisation, empty otherwise.
    ///
    /// `template <> type Box<int> { .. }` and
    /// `template <T type> type Box<*T> { .. }` both name the family and then
    /// write what the arguments must look like. An ordinary declaration writes
    /// no pattern: it declares the family itself, not a member of it.
    pub pattern: Vec<TypeSyntax>,
    /// Whether this is `type Name;` -- a declaration with no definition yet.
    ///
    /// An incomplete type: the name is known and its fields are not, so it may
    /// be used behind a pointer or a reference and nowhere else.
    pub is_forward: bool,
    /// The constants declared in the body, `VALUE const bool = false;`.
    ///
    /// A constant member belongs to the type rather than to a value of it:
    /// it takes no storage and every mention folds to its value. A type family
    /// uses one to answer a question about an argument -- `IsInteger<T>` holds
    /// `VALUE const bool` -- which is what lets a trait be a specialisation
    /// rather than a compiler built-in. The declaration order follows the
    /// source, like fields.
    pub constants: Vec<ConstantMember>,
}

/// One constant declared inside a type body.
///
/// Written `NAME const Type = value;`, the same shape as a module-level
/// constant. It has no storage of its own: a use of `Type.NAME` in a constant
/// position folds to the value, and one on a value of the type asks the type's
/// declaration. A member constant of a family is resolved per instantiation, so
/// a specialisation may answer differently for different arguments.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConstantMember {
    /// The constant's name.
    pub name: String,
    /// The span of the name alone, for diagnostics.
    pub name_span: Span,
    /// The declared type of the value.
    pub ty: TypeSyntax,
    /// The value, which must be a constant expression.
    pub value: ExprId,
    /// Whether the constant is marked `export`.
    ///
    /// No syntax declares one today -- a member cannot be named from outside
    /// without naming its owner -- so this is always `false`. It is carried
    /// because the constants a family answers with travel with the exported
    /// family, and the export pass decides what to publish from the
    /// declaration.
    pub exported: bool,
}

/// The shape of a named type.
///
/// The three forms are one declaration with three meanings: what a value of the
/// type *is*. A struct is its fields side by side, a union is its fields at the
/// same address, an alias is another type entirely.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum TypeDeclKind {
    /// `type Point { x int; y int; }` -- the fields, one after another.
    #[default]
    Struct,
    /// `union Value { i int; f float; }` -- the fields, all at one address.
    ///
    /// Which member is live is the program's business: reading the one that was
    /// not written is undefined, exactly as in C. This is what makes a union
    /// useful for reinterpreting a value's bits, and dangerous for anything
    /// that owns a resource -- a union of two types that both free their
    /// storage cannot know which one to free, which is why a union should hold
    /// trivial types.
    Union,
    /// `type Id = usize;` -- another name for an existing type.
    ///
    /// An alias introduces no new type: `Id` and `usize` are the same type
    /// under two names, and a value of one is a value of the other with no
    /// conversion. What it buys is a name that says what the type is *for*.
    Alias(TypeSyntax),
}

/// One member of an `enum`.
///
/// An enumerator is a named constant of the enum's underlying type. Its value
/// is written with `=`, or is one more than the previous one -- starting at
/// zero -- when it is not.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnumMember {
    /// The member's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The value, if written.
    ///
    /// Kept as an expression rather than folded here: `GREEN = 2 * 3` is a
    /// constant, and what a constant is is the checker's rule to apply.
    pub value: Option<ExprId>,
    /// The whole member's source range.
    pub span: Span,
}

/// An `enum` declaration.
///
/// ```text
/// enum Color int {
///     RED,          // 0
///     GREEN = 5,    // 5
///     BLUE,         // 6
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnumDecl {
    /// The template parameters, empty for an ordinary enum.
    pub template_params: Vec<TemplateParam>,
    /// The enum's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// Whether the declaration is marked `export`.
    pub exported: bool,
    /// The underlying type, or `None` for `int`.
    ///
    /// The enumerators are values of this type, which is what makes an `enum`
    /// usable as a `switch` scrutinee and comparable with the integers it was
    /// built from.
    pub base: Option<TypeSyntax>,
    /// The members, in declaration order.
    pub members: Vec<EnumMember>,
    /// Whether this is `enum Name;` / `enum Name Base;` -- a declaration whose
    /// members arrive with the definition. The base type is known here, so the
    /// type itself is usable at once.
    pub is_forward: bool,
    /// The whole declaration's source range.
    pub span: Span,
}

/// One field of a type declaration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FieldDecl {
    /// The field's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The field's type, written after its name like every declaration.
    pub ty: TypeSyntax,
    /// Whether the field is marked `mutable`.
    ///
    /// A mutable field may be changed by a `const` member function, which is
    /// the one exception to "a const method does not modify its receiver". A
    /// cache, a reference count, or a lazily computed value is what it is for.
    ///
    /// Written after the type, because it is a property of the field rather
    /// than of the type: the same `int` is mutable in one type and not in
    /// another.
    pub mutable: bool,
    /// The alignment the field is required to have, if `alignas` named one.
    ///
    /// Written after the field's type, like every declaration modifier.
    ///
    /// Written before the type, with the other declaration attributes.
    pub align: Option<AlignSpec>,
    /// The whole field's source range.
    pub span: Span,
}

/// An `alignas(..)` request.
///
/// The alignment is a value rather than a type, because that is what it is: a
/// number of bytes. It has to be a power of two, which is checked where the
/// declaration is checked rather than where it is parsed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AlignSpec {
    /// The requested alignment in bytes.
    pub alignment: ExprId,
    /// The whole `alignas(..)`'s source range.
    pub span: Span,
}

impl Ast {
    /// Marks every function whose name is shared with another.
    ///
    /// Called once parsing is complete, because whether a name is overloaded is
    /// a property of the whole file rather than of one declaration.
    pub fn mark_overloads(&mut self) {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for &item in &self.items {
            if let ItemKind::Function(function) = &self.item(item).kind {
                *counts.entry(function.name.clone()).or_default() += 1;
            }
        }

        let shared: std::collections::HashSet<String> = counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(name, _)| name)
            .collect();

        for index in 0..self.item_data.len() {
            if let ItemKind::Function(function) = &mut self.item_data[index].kind {
                function.overloaded = shared.contains(&function.name);
            }
        }
    }
}

impl TypeDecl {
    /// Returns the field with `name`.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(usize, &FieldDecl)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }
}

/// An `import` declaration.
///
/// The imported name is a module's name, which is its file's stem. There are
/// three spellings, and they differ in *what the importing file can name*, not
/// in what is loaded: all three load the module named by
/// [`ImportItem::module`].
///
/// ```text
/// import C;                       // C.printf
/// import C as Libc;               // Libc.printf
/// from C import printf;           // printf
/// from C import printf, puts;     // printf, puts
/// from C import *;                // printf, puts, .. every export
/// export from C import *;         // and re-exported from this module
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportItem {
    /// The module to load.
    pub module: String,
    /// The span of the module's name alone.
    pub name_span: Span,
    /// A local name for the module, from `import C as Libc`.
    ///
    /// The module is still loaded under its own name; this is the name the
    /// importing file uses for it. Two files may therefore give the same module
    /// different local names, and a file may import one module twice under two
    /// names.
    pub alias: Option<ImportedName>,
    /// Whether this import's names are re-exported by this module.
    ///
    /// A plain import brings names into *this* file. `export from C import *;`
    /// additionally publishes them, which is how a module is assembled out of
    /// others: an umbrella declares nothing itself and forwards what its parts
    /// export.
    ///
    /// The qualifier is written rather than implied, so what a module publishes
    /// is always something a reader can find by looking for `export`.
    pub exported: bool,
    /// The members named by a `from` import, if this is one.
    ///
    /// `None` for a plain `import`, where the module itself is the name that
    /// gets bound. `Some(ImportedMembers::All)` for `import *`, and
    /// `Some(ImportedMembers::Named(..))` for a list.
    pub members: Option<ImportedMembers>,
}

/// The members a `from` import brings into scope.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ImportedMembers {
    /// `from M import a, b;` — exactly these, in order.
    ///
    /// A list rather than a set, so that a diagnostic can point at the member
    /// that failed, and so that the written order survives into the dump.
    Named(Vec<ImportedName>),
    /// `from M import *;` — everything the module exports.
    All,
}

/// A name written in an import, with its span.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportedName {
    /// The name as written.
    pub name: String,
    /// Where it was written.
    pub span: Span,
}

/// A `link` declaration.
///
/// A link instruction, not a runtime call: it says what the produced binary
/// must be linked against. It is an item, at the same level as `import`, so the
/// instruction applies to the whole module and does not depend on which
/// function happens to call something.
///
/// ```text
/// link m dynamic from default;                    // libm, shared, default path
/// link my_lib static from "/path/to/my/lib";      // libmy_lib, archive, there
/// link "gcc_s" dynamic from default;              // a name no identifier could spell
/// ```
///
/// A name that cannot be written as an identifier -- because it collides with
/// an operator or a keyword, or is not an identifier at all -- is written in
/// quotes, as in `import`. The two spellings denote the same name: the quotes
/// are an escape hatch for the lexer, and nothing downstream distinguishes
/// them.
///
/// All three parts are required. Omitting the kind would leave the more
/// consequential of the two choices to a default, and a static link that was
/// meant to be dynamic is not a mistake worth making silent. `default` is
/// likewise written rather than left out, so a line names where its library
/// comes from even when the answer is "wherever the linker looks".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkItem {
    /// The library's name, as written.
    ///
    /// Without a `lib` prefix or a suffix, so that `link m` means `-lm` on a
    /// Unix link line. A name that already looks like a file — one containing a
    /// `/`, or ending in `.a` or `.so` — is passed through instead, so a
    /// library the caller has a full path for can still be named here.
    pub library: String,
    /// Where its name was written.
    pub name_span: Span,
    /// Whether the name was written in quotes rather than as an identifier.
    ///
    /// The two spellings mean the **same** name. The quotes are an escape hatch
    /// for the lexer, not a qualifier: they exist so that a library whose name
    /// would collide with an operator or a keyword can still be written. This
    /// flag is kept only so a diagnostic or a dump can echo what was typed.
    pub name_quoted: bool,
    /// How the library is linked.
    pub kind: LinkKind,
    /// Where to find it.
    pub location: LinkLocation,
    /// The span of the whole declaration.
    pub span: Span,
}

/// Whether a linked library is shared or an archive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    /// `dynamic` — a shared object, `.so`, resolved when the program starts.
    Dynamic,
    /// `static` — an archive, `.a`, copied into the program at link time.
    Static,
}

impl LinkKind {
    /// Returns the keyword's source spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            LinkKind::Dynamic => "dynamic",
            LinkKind::Static => "static",
        }
    }
}

/// Where the linker should look for a library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LinkLocation {
    /// `default` — the linker's own search path, which is where a system
    /// library is found.
    Default,
    /// A directory, written as a string literal.
    Directory(String),
}

/// A function declaration.
///
/// The return type is written after a parenthesised parameter list; an omitted
/// one means the function returns nothing. Parameters may carry defaults, the
/// last may be `...`, and `export`, `constexpr`, `extern "ABI"`, an attribute,
/// and a `template <..>` list may qualify the declaration. A semicolon instead
/// of a body declares a function implemented elsewhere.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FunctionItem {
    /// The ABI the function is declared with, if any.
    ///
    /// `extern "C"` names the C calling convention explicitly. An absent ABI
    /// means the language's own convention, which is only defined for
    /// functions this compilation unit implements.
    pub abi: Option<String>,
    /// Whether the declaration is marked `constexpr`.
    ///
    /// A `constexpr` function may be called where the language requires a
    /// constant -- an array length, a template argument, the condition of an
    /// `if constexpr` -- and every argument at such a call must itself be
    /// constant. A function without the marker may still be *folded* when the
    /// compiler can, but folding is an optimisation and not a promise: what a
    /// program may rely on is the marker.
    pub is_constexpr: bool,
    /// Whether the declaration is marked `export`.
    ///
    /// A module's declarations are private unless exported, so this is what
    /// decides whether an importer can name the function.
    pub exported: bool,
    /// What the declaration says about raising exceptions, if anything.
    ///
    /// `??` claims the body may raise; `??(cond)` claims it may raise only when
    /// `cond` holds, which is a constant the checker evaluates. A function
    /// without either may not raise, and a call to one that may needs a `try` or
    /// a `??` of its own.
    pub throws: Option<Throws>,
    /// Whether the declaration is only a signature, with no body.
    ///
    /// An `export fn f(..) T;` with a semicolon and no braces declares a
    /// function implemented elsewhere — a C function, for instance.
    pub is_external: bool,
    /// What the declaration says about its own body.
    ///
    /// A special method is generated by the compiler unless the type says
    /// otherwise, and this is where it says so: `= default` asks for the
    /// generated one, `= delete` refuses the operation outright.
    /// Whether the parameter list ends with `...`.
    ///
    /// A variadic function accepts any number of arguments after its declared
    /// parameters.
    pub variadic: bool,
    /// The template parameters, empty for an ordinary function.
    ///
    /// `template <T type> fn min(..) T { .. }` declares a family of functions,
    /// one per set of arguments the parameters take. A function with none is
    /// what every function was before there were templates.
    pub template_params: Vec<TemplateParam>,
    /// The function's name.
    pub name: String,
    /// Whether another function shares this name.
    ///
    /// Set once every declaration is known, so that code generation can give
    /// the overloads distinct symbols. A function that is not overloaded keeps
    /// its plain name, which is what a C function's symbol must be.
    pub overloaded: bool,
    /// The span of the name alone, for diagnostics that point at it.
    pub name_span: Span,
    /// The declared parameters.
    pub params: Vec<Param>,
    /// The return type, written before the body.
    pub return_type: TypeSyntax,
    /// The function body, absent for an external declaration.
    ///
    /// `export fn printf(fmt *const byte, ...) int;` declares a function this
    /// compilation unit does not define, so it has no body.
    pub body: Option<BlockId>,
}

/// One parameter of a `template <..>` list.
///
/// # Two kinds
///
/// A *type* parameter names a type the caller chooses: `template <T type>`.
/// A *value* parameter names a constant the caller chooses, and its type is
/// restricted to the ones a value can be written down as -- an integer, or a
/// character sequence. That restriction is what lets a template be instantiated
/// from the argument alone, without running any code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TemplateParam {
    /// The parameter's name, as written.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// What kind of parameter this is.
    pub kind: TemplateParamKind,
    /// Whether the parameter is a pack, written `args ...Args`.
    ///
    /// A pack stands for a *sequence* of arguments rather than one, and it is
    /// the last parameter a template may declare: `template <Args ...type>`
    /// takes any number of types, and `template <values ...int>` any number of
    /// `int`s. What a pack is used *for* is an expansion, which is written by
    /// placing `...` after the name where the elements are wanted.
    pub is_pack: bool,
    /// The default, if one was written.
    ///
    /// `template <T type = int, n int = 4>`: a caller may leave these out, and
    /// what is left out is filled in from here. A default may use the
    /// parameters written before it -- `template <T type, n int = sizeof(T)>` --
    /// which is why it is kept as syntax rather than as a value.
    pub default: Option<TemplateDefault>,
    /// The whole parameter's source range.
    pub span: Span,
}

/// What a template parameter falls back to when a caller omits it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateDefault {
    /// `T type = int` -- a type, written as any type is.
    Type(TypeSyntax),
    /// `n int = 4` -- a value, written as any constant expression is.
    Value(ExprId),
}

/// What a template parameter stands for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateParamKind {
    /// `T type` — a type the caller supplies.
    Type,
    /// `N int` — a value the caller supplies, of the named type.
    ///
    /// Only an integer or a character sequence, because a template argument has
    /// to be known without running the program.
    Value(TypeSyntax),
}

impl TemplateParamKind {
    /// Whether the parameter stands for the type it declares, rather than a
    /// value of it.
    ///
    /// A pack of either kind is expanded the same way; only what each element
    /// *is* differs, and that is what this says.
    #[must_use]
    pub const fn is_type(&self) -> bool {
        matches!(self, TemplateParamKind::Type)
    }
}

/// One declared parameter.
///
/// A name and a postfix type, optionally followed by `= expr` for a default.
/// Defaults must come last. A trailing `...` makes the function variadic and is
/// recorded on [`FunctionItem`], not here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Param {
    /// The parameter's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The parameter's declared type, written after its name.
    pub ty: TypeSyntax,
    /// The default value, if the parameter has one.
    ///
    /// A parameter with a default may be omitted at a call. Defaults must come
    /// last, as in C++.
    pub default: Option<ExprId>,
    /// Whether the parameter is a *pack*, written `values ...Ts`.
    ///
    /// A pack takes any number of arguments instead of one, and is expanded
    /// where its elements are wanted: `f(values...)` passes them all, and
    /// `(this.write(values), ...)` repeats the call for each.
    pub is_pack: bool,
    /// The whole parameter's source range.
    pub span: Span,
}

impl FunctionItem {
    /// Returns `true` when a parameter is a pack.
    ///
    /// Such a function is not callable as it stands: the number of arguments
    /// decides how many parameters the call reaches, and the checker expands
    /// the declaration for each call. Every pass that asks "is this an ordinary
    /// function?" has to ask this as well.
    #[must_use]
    pub fn has_pack(&self) -> bool {
        self.params.iter().any(|param| param.is_pack)
    }
}

/// An `interface` declaration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterfaceDecl {
    /// The template parameters, empty for an ordinary interface.
    pub template_params: Vec<TemplateParam>,
    /// The interface's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The subject parameter, when the interface is written with a parameter
    /// list: `interface IIterable(it Iter)`.
    ///
    /// The first parameter is the subject, exactly as a trait's is, and the
    /// requirements below are written against it.
    pub subject: Option<TraitParam>,
    /// The methods the table holds, derived from the restrictions.
    ///
    /// An interface states its methods by calling them -- `it.next() -> int` --
    /// and a restriction that says what the call yields is a method a box can
    /// dispatch. The list is derived where the body is parsed, so every reader
    /// sees one statement of the contract rather than two.
    pub methods: Vec<InterfaceMethod>,
    /// The restrictions a type must satisfy to be boxed, in source order.
    ///
    /// The same list a trait carries, written the same way: an interface is
    /// declared like a trait, and the two differ in what the declaration is
    /// *for* -- a trait is a compile-time question, an interface a box built at
    /// run time.
    pub requirements: Vec<TraitRequirement>,
    /// Whether the declaration is marked `export`.
    pub exported: bool,
    /// Whether this is `interface Name;` -- the name and nothing else.
    ///
    /// An incomplete interface may only be named behind a pointer or a
    /// reference until a definition arrives.
    pub is_forward: bool,
    /// The whole declaration's source range.
    pub span: Span,
}

/// One method of an [`InterfaceDecl`], as its restriction states it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterfaceMethod {
    /// The method's name.
    pub name: String,
    /// The span of the name alone.
    pub name_span: Span,
    /// The parameters' types, without a receiver: the receiver is the value the
    /// interface holds.
    ///
    /// Types and not parameters, because a restriction states a type:
    /// `it.add(*int) -> int` says the call takes a pointer to an `int`, and a
    /// declared name would be one no call site can use. A method reached
    /// through a box is called by position for that reason.
    pub params: Vec<TypeSyntax>,
    /// The result type, which the restriction states with `-> Type`.
    pub returns: TypeSyntax,
    /// Whether a call of this method may raise.
    ///
    /// The annotation is part of the method's *type*: a method that may raise
    /// returns its result through the exception ABI, so a type satisfies the
    /// interface only when its method agrees about this. The restriction writes
    /// it with `??`, the same marker a call that passes an exception on uses:
    /// `it.save(String)?? -> void`.
    pub throws: bool,
    /// The whole signature's source range.
    pub span: Span,
}

impl InterfaceDecl {
    /// The same declaration, as a trait.
    ///
    /// One declaration, two uses: the requirements a box checks are the ones a
    /// trait query checks, so the restriction list is the trait's.
    ///
    /// The parameters are the subject and then the interface's own type
    /// parameters, in that order, because that is the order a box supplies
    /// them in: the concrete type it erases, and the arguments it was written
    /// with (`IValue<int>`). A restriction that names one of them --
    /// `it.get() -> T` -- is checked with the type the box actually holds.
    #[must_use]
    pub fn as_trait(&self) -> TraitDecl {
        let mut params = Vec::new();
        if let Some(subject) = &self.subject {
            params.push(subject.clone());
        }
        for parameter in &self.template_params {
            params.push(TraitParam {
                name: parameter.name.clone(),
                name_span: parameter.name_span,
                ty: TypeSyntax::Named(NamedType {
                    path: parameter.name.clone(),
                    arguments: Vec::new(),
                    is_expansion: false,
                    span: parameter.span,
                }),
            });
        }
        TraitDecl {
            exported: self.exported,
            name: self.name.clone(),
            name_span: self.name_span,
            params,
            requirements: self.requirements.clone(),
            is_forward: self.is_forward,
            span: self.span,
        }
    }
}

/// `if (cond) @constexpr { .. } else { .. }` around top-level declarations.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConditionalItems {
    /// The condition. It **must** be a compile-time constant.
    pub condition: crate::node::ExprId,
    /// The declarations the condition selects when it holds.
    pub then_items: Vec<super::ItemId>,
    /// The declarations selected otherwise, when there is an `else`.
    pub else_items: Vec<super::ItemId>,
}
