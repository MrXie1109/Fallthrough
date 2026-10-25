//! Lowering: the checked AST is walked and LLVM IR is emitted.
//!
//! # The [`Codegen`] trait
//!
//! Emitting an LLVM value for a node is one operation, and the trait names it:
//! `emit(node) -> Option<BasicValueEnum>`. Splitting it from the traversal
//! (which nodes exist, what order they are visited in) keeps the emission rules
//! separate from the walk, so a construct's IR can be stated on its own.
//!
//! The walk in [`Lowerer`] does not go through the trait: it emits each node
//! with its own `lower_*` method, so the three `emit_*` methods have no callers.
//! They are the trait's per-node entry points and report rather than emit, so
//! reaching one cannot lower a node twice.
//!
//! # `fallthrough`
//!
//! `fallthrough;` is a statement at the end of one arm of a `switch`: it
//! branches to the next arm's block, or to the join block after the last arm.
//! Every other arm branches to the join block when it reaches its end, which is
//! why cases do not fall through by default.
//!
//! That lowering lives in `Lowerer::lower_switch`, because it is the construct
//! that knows the next arm. `Lowerer::lower_fallthrough` is the standalone
//! entry point, and has no callers in the walk.

use std::collections::HashMap;

use inkwell::builder::Builder;
// `size_of` lives on the `BasicType` trait, and every aggregate-return decision
// and copy measures a `BasicTypeEnum`.
use inkwell::types::BasicType as _;
use inkwell::values::{BasicValue as _, BasicValueEnum, FunctionValue};

use fallc_ast::{
    AssignExpr, Ast, BinaryExpr, BinaryOp, BindStmt, BlockId, CallExpr, CastExpr, ElseBranch,
    ExprId, ExprKind, FieldExpr, ForCStmt, FunctionItem, IfStmt, IncDecExpr, IncDecOp,
    IndexExpr, Initializer, ItemId, ItemKind, LoopStmt, ReturnStmt, StmtId, StmtKind, SwitchStmt,
    TypeSyntax, TypedInitExpr, UnaryExpr, UnaryOp,
};
use fallc_sema::{BindingTypes, Type};

use crate::types::{TypeMapper, llvm_member_index};
use fallc_diag::{Diagnostic, DiagnosticBag};
use fallc_sema::TypeTable;
use fallc_span::Span;

use crate::backend::{Backend, CodegenError};

/// Emits the LLVM value for one AST node.
///
/// Implementors receive the AST node to emit and return the value it produced,
/// or `None` for nodes that produce no value (statements, declarations, and
/// any expression the spec says is valueless).
///
/// Returning `Err` aborts the enclosing function's lowering; the driver
/// records the diagnostic and moves on.
pub trait Codegen {
    /// Emits an item (a top-level declaration).
    ///
    /// # Errors
    ///
    /// Returns a [`CodegenError`] if the item cannot be lowered, including the
    /// spec-blocked case.
    fn emit_item(&mut self, ast: &Ast, item: ItemId) -> Result<(), CodegenError>;

    /// Emits a statement.
    ///
    /// # Errors
    ///
    /// Returns a [`CodegenError`] if the statement cannot be lowered.
    fn emit_stmt(
        &mut self,
        ast: &Ast,
        stmt: StmtId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError>;

    /// Emits an expression, returning the value it produces.
    ///
    /// # Errors
    ///
    /// Returns a [`CodegenError`] if the expression cannot be lowered.
    fn emit_expr(
        &mut self,
        ast: &Ast,
        expr: ExprId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError>;
}

/// Where `break` and `continue` branch to within the innermost loop.
#[derive(Clone, Copy, Debug)]
struct LoopTargets<'ctx> {
    /// The block `continue` jumps to: the loop condition, or the update clause
    /// of a C-style `for`.
    continue_to: inkwell::basic_block::BasicBlock<'ctx>,
    /// The block `break` jumps to.
    break_to: inkwell::basic_block::BasicBlock<'ctx>,
    /// How many scopes the loop body owns.
    ///
    /// `break` and `continue` leave those scopes, so the values in them have to
    /// be destroyed before the jump. Without this the loop leaked every value
    /// the iteration had built -- silently, because a missing destructor is
    /// not a diagnostic anywhere.
    scope_depth: usize,
}

/// Returns the symbol a function is emitted under.
///
/// A constructor carries its receiver's type, because `__init__` is written
/// once per type but each is a distinct function. Every other function keeps
/// its own name.
fn symbol_of(function: &FunctionItem, resolve: impl Fn(&TypeSyntax) -> Type) -> String {
    // One rule for every function: the written name, then every parameter type
    // mangled structurally.
    //
    // Nothing is special-cased. There were three ad-hoc schemes before this,
    // and all three had the same defect: they built a symbol from a type's
    // *printed* name, so `*const Box` and `const *Box` -- different types using
    // the same words -- shared one symbol, and the second definition silently
    // replaced the first rather than being reported.
    //
    // The types are appended whether or not the function is overloaded. That
    // decision used to be conditional, which made a function's symbol depend on
    // whether another definition of the same name existed anywhere in the
    // program: adding an overload changed the symbol of the original.
    let params: Vec<String> = function
        .params
        .iter()
        .map(|param| fallc_sema::mangle_type(&resolve(&param.ty)))
        .collect();
    fallc_ast::special::symbol_for(&function.name, &params)
}

/// Builds the address of a struct's field.
///
/// The index is the field's own position in the declaration that produced the
/// LLVM struct, so it is in range by construction.
fn build_field_gep(
    builder: &Builder<'static>,
    struct_ty: inkwell::types::BasicTypeEnum<'static>,
    storage: inkwell::values::PointerValue<'static>,
    index: u32,
) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
    builder
        .build_struct_gep(struct_ty, storage, index, "field")
        .map_err(|error| CodegenError::Emit(error.to_string()))
}

/// Builds the address of `base[subscript]`.
///
/// # Safety
///
/// inkwell marks `build_in_bounds_gep` `unsafe` because the element type has to
/// match what the pointer really points at, and a mismatch is undefined
/// behaviour that LLVM will not catch. The caller derives `element_ty` from the
/// indexed object's own type -- an array's element, or a pointer's pointee --
/// through the same mapper that produced the pointer, so the two agree by
/// construction.
///
/// This is the second place in this crate that needs `unsafe`, and it is named
/// and argued rather than scattered: the alternative is generating the LLVM IR
/// for a GEP as text, which would lose the type checking LLVM does give.
#[allow(unsafe_code)] // See the safety argument above.
fn build_index_gep(
    builder: &Builder<'static>,
    element_ty: inkwell::types::BasicTypeEnum<'static>,
    base: inkwell::values::PointerValue<'static>,
    subscript: inkwell::values::IntValue<'static>,
) -> Result<inkwell::values::PointerValue<'static>, CodegenError> {
    // SAFETY: `element_ty` is the type the pointer was built for -- an array's
    // element or a pointer's pointee, mapped by the same `TypeMapper` -- so the
    // GEP's element type and the real one agree, which is the condition inkwell
    // cannot check for itself.
    unsafe {
        builder
            .build_in_bounds_gep(element_ty, base, &[subscript], "index")
            .map_err(|error| CodegenError::Emit(error.to_string()))
    }
}

/// Returns the pointer forms a receiver may have been declared with.
///
/// A member function's receiver is `this *T` or `this *const T`, and the
/// object's own type does not say which -- both accept the same value. The
/// const form is tried first because it is the more permissive signature.
/// Returns the receiver forms a destructor may be written with.
///
/// A destructor receives the value it destroys either by pointer -- `this *T`
/// -- or by reference -- `this &T` -- and both spellings are in use: the
/// standard containers write `&String`, and a small type of one's own may
/// write `*T`. What the function is handed is the address either way, so every
/// spelling is a candidate for the same call.
fn drop_receiver_forms(receiver: &Type) -> Vec<Type> {
    let mut forms = pointer_forms(receiver);
    if !receiver.is_indirection() {
        for referent_is_const in [false, true] {
            forms.push(Type::Reference {
                referent: Box::new(receiver.clone()),
                referent_is_const,
                is_rvalue: false,
            });
        }
    }
    forms
}

fn pointer_forms(receiver: &Type) -> Vec<Type> {
    if receiver.is_indirection() {
        return vec![receiver.clone()];
    }
    [true, false]
        .into_iter()
        .map(|pointee_is_const| Type::Pointer {
            pointee: Box::new(receiver.clone()),
            pointee_is_const,
            pointer_is_const: false,
        })
        .collect()
}

/// Returns the symbol a type's constructor is emitted under.
///
/// `__init__` is written once per type, so its receiver is what tells the
/// definitions apart -- and the receiver is a parameter like any other, so the
/// same mangling applies. Building the name from the bare type name instead
/// would make `__init__(*Point)` and `__init__(*const Point)` share a symbol.
fn constructor_symbol(ty: &Type) -> String {
    fallc_ast::special::symbol_for(
        fallc_ast::special::SpecialMethod::Init.name(),
        &[fallc_sema::mangle_type(ty)],
    )
}

/// The symbol prefix of a type's implicit constructor.
fn implicit_constructor_symbol(ty: &Type) -> String {
    fallc_ast::special::symbol_for("__im_init__", &[fallc_sema::mangle_type(ty)])
}

/// Returns the name an expression denotes, if it is a plain name.
/// Returns the span an initialiser's constructor was resolved at.
///
/// A construction has no call to write `??` on, so the marker goes on the
/// initialiser as a whole; the constructor was resolved from the value beneath
/// it, and that value is what the checker keyed the call by.
fn initialiser_span(ast: &Ast, expr: fallc_ast::ExprId) -> fallc_span::Span {
    match &ast.expr(expr).kind {
        ExprKind::Propagate(inner) => ast.expr(inner.value).span,
        _ => ast.expr(expr).span,
    }
}

fn name_of(kind: &ExprKind) -> &str {
    match kind {
        ExprKind::Name(name) => name,
        ExprKind::Sizeof(_) => "sizeof",
        ExprKind::Alignof(_) => "alignof",
        _ => "",
    }
}

/// Builds an LLVM constant array from already-built element constants.
///
/// `ArrayType::const_array` is typed with the element's own value class, so the
/// elements are collected into the concrete handle the LLVM call expects. A
/// form with no such constructor here is refused rather than guessed at.
fn constant_array<'ctx>(
    element: inkwell::types::BasicTypeEnum<'ctx>,
    values: Vec<BasicValueEnum<'ctx>>,
) -> Option<BasicValueEnum<'ctx>> {
    Some(match element {
        inkwell::types::BasicTypeEnum::IntType(int) => {
            let mut typed = Vec::with_capacity(values.len());
            for value in values {
                let BasicValueEnum::IntValue(value) = value else {
                    return None;
                };
                typed.push(value);
            }
            int.const_array(&typed).as_basic_value_enum()
        }
        inkwell::types::BasicTypeEnum::FloatType(float) => {
            let mut typed = Vec::with_capacity(values.len());
            for value in values {
                let BasicValueEnum::FloatValue(value) = value else {
                    return None;
                };
                typed.push(value);
            }
            float.const_array(&typed).as_basic_value_enum()
        }
        inkwell::types::BasicTypeEnum::PointerType(pointer) => {
            let mut typed = Vec::with_capacity(values.len());
            for value in values {
                let BasicValueEnum::PointerValue(value) = value else {
                    return None;
                };
                typed.push(value);
            }
            pointer.const_array(&typed).as_basic_value_enum()
        }
        inkwell::types::BasicTypeEnum::StructType(structure) => {
            let mut typed = Vec::with_capacity(values.len());
            for value in values {
                let BasicValueEnum::StructValue(value) = value else {
                    return None;
                };
                typed.push(value);
            }
            structure.const_array(&typed).as_basic_value_enum()
        }
        inkwell::types::BasicTypeEnum::ArrayType(array) => {
            let mut typed = Vec::with_capacity(values.len());
            for value in values {
                let BasicValueEnum::ArrayValue(value) = value else {
                    return None;
                };
                typed.push(value);
            }
            array.const_array(&typed).as_basic_value_enum()
        }
        _ => return None,
    })
}

/// Parses an integer literal's digits, honouring the base prefix and ignoring
/// separators.
///
/// Returns `None` for a literal that is malformed or too large, which the
/// caller reports.
fn parse_integer_literal(digits: &str) -> Option<u64> {
    // Separators carry no value, and the scanner has already checked that they
    // sit between digits.
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();

    let (radix, body) = if let Some(rest) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        (2, rest)
    } else {
        (10, cleaned.as_str())
    };
    u64::from_str_radix(body, radix).ok()
}

/// Decodes a character literal into its code point.
///
/// The text includes the quotes and possibly a width prefix. Exactly one
/// character, or one escape sequence, is expected between the quotes.
///
/// The decoding itself lives in `fallc-ast`, so that the type checker's range
/// check and this one cannot come to disagree about what a literal means.
fn decode_char_literal(text: &str, span: fallc_span::Span) -> Result<u32, CodegenError> {
    fallc_ast::literal::decode_char_code_point(text).ok_or_else(|| {
        CodegenError::Emit(format!(
            "a character literal holds one character, at {span:?}"
        ))
    })
}

/// An external function's signature, in resolved form.
///
/// The type checker produces these; code generation needs them to emit an LLVM
/// declaration, because there is no definition in this module to read a
/// signature from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternalSignature {
    /// Parameter types, in order.
    pub params: Vec<Type>,
    /// Parameter names, in order.
    ///
    /// Carried across the boundary so that a caller in another module can name
    /// an argument: `.name = value` is placed by the name, and the types alone
    /// do not say what the parameters are called.
    pub param_names: Vec<String>,
    /// The return type. A type spelled with an empty name means no value.
    pub returns: Type,
    /// Whether extra arguments are accepted.
    pub variadic: bool,
    /// Whether calling it may raise, which changes the result's shape.
    pub throws: bool,
    /// The symbol the defining module emitted, mangled from the parameters.
    ///
    /// Recorded rather than derived at the call site, so that the two cannot
    /// drift: a call to an imported function asks for exactly the symbol its
    /// module produced.
    pub symbol: String,
}

/// A value that must be destroyed when its scope ends.
///
/// The storage is the variable's address, which is what `__drop__` takes as its
/// receiver; the symbol is the destructor to call.
#[derive(Clone, Debug)]
struct DropEntry<'ctx> {
    /// The variable holding the value.
    storage: inkwell::values::PointerValue<'ctx>,
    /// The destructor to call on it, when the type defines one.
    ///
    /// `None` when the type has no destructor itself but a field does, in which
    /// case the fields are destroyed individually.
    symbol: Option<String>,
    /// The value's type, so its fields can be found.
    ty: Type,
}

/// What an escape from inside a `try` meets on its way out.
#[derive(Clone, Copy, Debug)]
enum ExcFrame {
    /// A `try` body with an `except`: the exception is stored in `slot` and
    /// control goes to `block`.
    Handler {
        /// Where the handler's `except` block starts.
        block: inkwell::basic_block::BasicBlock<'static>,
        /// The storage the caught exception is read from.
        slot: inkwell::values::PointerValue<'static>,
        /// How many drop scopes were live outside the `try` body.
        ///
        /// An exception leaving the body leaves the scopes it declared, so
        /// their destructors run before the handler does -- exactly as they
        /// would on a `return`.
        depth: usize,
    },
    /// A `finally`, which an escape runs on the way past.
    Finally(fallc_ast::BlockId),
}

/// A local variable's storage and the type stored in it.
///
/// The pointee type is recorded at binding time rather than recovered from the
/// pointer, because inkwell's `PointerType` does not expose its element type
/// (opaque pointers), so `build_load` cannot derive it.
#[derive(Clone, Copy, Debug)]
struct LocalBinding<'ctx> {
    /// The `alloca` holding the variable.
    storage: inkwell::values::PointerValue<'ctx>,
    /// The type stored in `storage`.
    ty: inkwell::types::BasicTypeEnum<'ctx>,
}

/// One module's tree and tables, kept so that a default written there can be
/// lowered at a call site in another module.
#[derive(Clone, Debug)]
pub struct DefaultTree {
    /// The module's parsed tree.
    ///
    /// Shared with the module body the driver keeps: a default argument is
    /// lowered only when a call omits one, and copying every module's tree on
    /// the chance that one is used is most of a compilation's memory.
    pub ast: std::rc::Rc<Ast>,
    /// The types its expressions were given.
    pub types: std::rc::Rc<fallc_sema::TypeTable>,
    /// Everything its own check resolved.
    pub resolutions: std::rc::Rc<ModuleResolutions>,
}

/// One module whose bodies are lowered alongside the main one.
///
/// The resolutions travel with the tree because a body's decisions were made by
/// its own check, and are meaningless under any other module's table.
#[derive(Debug)]
pub struct ModuleBodyRef<'a> {
    /// The module's own name, which is what a default's expression names.
    pub name: &'a str,
    /// The key the module-object cache stores this module's body under.
    pub cache_hash: u64,
    /// The module's parsed tree.
    pub ast: &'a std::rc::Rc<Ast>,
    /// The types its expressions were given.
    pub types: &'a std::rc::Rc<TypeTable>,
    /// The declared type of each of its local bindings.
    pub bindings: &'a BindingTypes,
    /// The nominal types the module declared, for their layout.
    pub declarations: &'a [fallc_sema::NominalType],
    /// Everything its own check resolved.
    pub resolutions: &'a std::rc::Rc<ModuleResolutions>,
    /// The spans in the module's tree that read storage another module owns.
    ///
    /// A span names a byte range in one file, so these travel per module; code
    /// generation reads them while lowering that module's bodies.
    pub external_global_refs: &'a std::collections::HashMap<Span, String>,
}

/// Everything one module's type check resolved.
///
/// Held per module rather than merged into one table, because a span identifies
/// a byte range in *one* file: two modules each have a call at offset 40, and
/// merging would let one shadow the other.
#[derive(Default, Clone, Debug)]
pub struct ModuleResolutions {
    /// Which casts are conversion methods.
    pub conversions: HashMap<Span, fallc_sema::ConversionMethod>,
    /// Which symbol each index expression resolved to.
    pub index_methods: HashMap<Span, String>,
    /// Which symbol each `__call__` resolved to.
    pub call_methods: HashMap<Span, String>,
    /// What each `str(x)`/`repr(x)` resolved to.
    pub text_calls: HashMap<Span, Option<String>>,
    /// Which definition each plain call reached.
    pub call_symbols: HashMap<Span, String>,
    /// The call of a closure value each call site makes.
    pub closure_calls: HashMap<Span, fallc_sema::ClosureCall>,
    /// The value each interface conversion boxes.
    pub interface_conversions: HashMap<Span, fallc_sema::InterfaceConversion>,
    /// The interface method each virtual call reaches.
    pub interface_calls: HashMap<Span, fallc_sema::InterfaceCall>,
    /// The methods of a type family, by their name spans.
    pub templated_methods: std::collections::HashSet<Span>,
    /// The declarations `@enable_if` removed.
    pub disabled_functions: std::collections::HashSet<Span>,
    /// The lambda each call calls.
    pub lambda_calls: HashMap<Span, fallc_sema::LambdaInstance>,
    /// The lambda each value-position use denotes.
    pub lambda_values: HashMap<Span, fallc_sema::LambdaInstance>,
    /// The constructor each `T(args)` call resolved to.
    pub constructor_calls: HashMap<Span, String>,
    /// The type each construction call builds.
    pub constructor_types: HashMap<Span, fallc_sema::Type>,
    /// Every function a call brought into being from a template.
    pub instantiations: HashMap<String, fallc_sema::Instantiation>,
    /// The folded value of each constant expression.
    pub constant_values: HashMap<Span, i64>,
    /// The arguments passed as their address.
    pub by_address_arguments: std::collections::HashSet<Span>,
    /// Which method each operator resolved to.
    pub operator_methods: HashMap<Span, fallc_sema::OperatorCall>,
    /// The type each argument literal adopted.
    pub adopted_types: HashMap<Span, fallc_sema::Type>,
    /// The constructor each literal used as a declared type calls.
    pub literal_constructions: HashMap<Span, String>,
    /// The type each such literal has.
    pub literal_construction_types: HashMap<Span, fallc_sema::Type>,
    /// Which `__bool__` each condition resolved to.
    pub bool_methods: HashMap<Span, String>,
    /// Which unary operator method each expression resolved to.
    pub unary_methods: HashMap<Span, String>,
    /// Which `__iadd__`-style method each compound assignment resolved to.
    pub in_place_methods: HashMap<Span, String>,
    /// Whether each compound assignment passes its operand by address.
    pub in_place_by_address: HashMap<Span, bool>,
    /// Which constructor each `T x = e;` resolved to.
    pub init_calls: HashMap<Span, String>,
    /// Which method each call resolved to.
    pub method_calls: HashMap<Span, String>,
    /// Which `__assign__` each assignment resolved to.
    pub assign_methods: HashMap<Span, String>,
    /// Where a `__deref__` result is read as a value.
    pub deref_reads: HashMap<Span, fallc_sema::Type>,
    /// The type each `->` chain ends at.
    pub pointer_targets: HashMap<Span, fallc_sema::Type>,
    /// The type each `type(expr)` names, by the span of the written form.
    pub type_of_types: HashMap<Span, fallc_sema::Type>,
    /// Which explicit `__init__`/`__drop__` calls were answered by a plain
    /// store or a field-wise destroy.
    pub in_place_operations: HashMap<Span, fallc_sema::InPlaceOperation>,
    /// Every `if` the checker decided at compile time, by the condition's span.
    pub compile_time_ifs: HashMap<Span, bool>,
    /// The constant arguments that fill the defaults a call left empty, by the
    /// call's span.
    pub default_arguments: HashMap<Span, Vec<(usize, i64, fallc_sema::Type)>>,
    /// The defaults a call left to an expression, by the call's span.
    pub default_expressions: fallc_sema::ExpressionDefaults,
    /// The calls whose callee may raise, so each is unwrapped after it returns.
    pub throwing_calls: std::collections::HashSet<Span>,
    /// Those written with `??`, whose exception leaves the function.
    pub propagating_calls: std::collections::HashSet<Span>,
}

impl ModuleResolutions {
    /// Records everything a check resolved.
    #[must_use]
    pub fn of(checked: &fallc_sema::TypeCheckResult) -> Self {
        Self {
            conversions: checked.conversions.clone(),
            index_methods: checked.index_methods.clone(),
            call_methods: checked.call_methods.clone(),
            text_calls: checked.text_calls.clone(),
            call_symbols: checked.call_symbols.clone(),
            templated_methods: checked.templated_methods.clone(),
            disabled_functions: checked.disabled_functions.clone(),
            closure_calls: checked.closure_calls.clone(),
            interface_conversions: checked.interface_conversions.clone(),
            interface_calls: checked.interface_calls.clone(),
            lambda_calls: checked.lambda_calls.clone(),
            lambda_values: checked.lambda_values.clone(),
            constructor_calls: checked.constructor_calls.clone(),
            constructor_types: checked.constructor_types.clone(),
            instantiations: checked.instantiations.clone(),
            constant_values: checked.constant_values.clone(),
            compile_time_ifs: checked.compile_time_ifs.clone(),
            by_address_arguments: checked.by_address_arguments.clone(),
            operator_methods: checked.operator_methods.clone(),
            adopted_types: checked.adopted_types.clone(),
            literal_constructions: checked.literal_constructions.clone(),
            literal_construction_types: checked.literal_construction_types.clone(),
            bool_methods: checked.bool_methods.clone(),
            unary_methods: checked.unary_methods.clone(),
            in_place_methods: checked.in_place_methods.clone(),
            in_place_by_address: checked.in_place_by_address.clone(),
            init_calls: checked.init_calls.clone(),
            method_calls: checked.method_calls.clone(),
            assign_methods: checked.assign_methods.clone(),
            deref_reads: checked.deref_reads.clone(),
            pointer_targets: checked.pointer_targets.clone(),
            type_of_types: checked.type_of_types.clone(),
            in_place_operations: checked.in_place_operations.clone(),
            default_arguments: checked.default_arguments.clone(),
            default_expressions: checked.default_expressions.clone(),
            throwing_calls: checked.throwing_calls.clone(),
            propagating_calls: checked.propagating_calls.clone(),
        }
    }
}

/// A lambda whose body has been declared but not yet emitted.
///
/// The body is emitted once the function that wrote the lambda is finished:
/// lowering a body needs the lowering state to itself, and a call site is
/// reached in the middle of another function.
#[derive(Debug)]
struct PendingLambda {
    /// The tree the lambda expression belongs to.
    tree: Ast,
    /// The lambda expression.
    lambda: fallc_ast::LambdaExpr,
    /// What the checker decided about it.
    instance: fallc_sema::LambdaInstance,
}

/// The all-zero value of a type, as LLVM's `zeroinitializer`.
///
/// Returns `None` for types LLVM cannot zero-initialise directly.
fn zero_value(ty: inkwell::types::BasicTypeEnum<'static>) -> Option<BasicValueEnum<'static>> {
    use inkwell::types::BasicTypeEnum;
    Some(match ty {
        BasicTypeEnum::IntType(int) => int.const_zero().as_basic_value_enum(),
        BasicTypeEnum::FloatType(float) => float.const_zero().as_basic_value_enum(),
        BasicTypeEnum::ArrayType(array) => array.const_zero().as_basic_value_enum(),
        BasicTypeEnum::StructType(structure) => structure.const_zero().as_basic_value_enum(),
        BasicTypeEnum::VectorType(vector) => vector.const_zero().as_basic_value_enum(),
        // Pointers and the exotic types are not zero-initialised yet: what a
        // zero pointer means in Fallthrough is unspecified.
        _ => return None,
    })
}

/// Everything a comparison needs about its operands.
///
/// Grouped into a struct because the comparison lowering needs four facts about
/// its two operands — their values and their types — and passing them loose
/// made an eight-argument function.
struct Comparison<'ctx> {
    /// The lowered left operand.
    lhs: BasicValueEnum<'ctx>,
    /// The lowered right operand.
    rhs: BasicValueEnum<'ctx>,
    /// The type the checker gave the left operand.
    lhs_ty: Type,
    /// The type the checker gave the right operand.
    rhs_ty: Type,
    /// The operator.
    op: BinaryOp,
    /// Where the comparison was written, for diagnostics.
    span: fallc_span::Span,
}

/// Returns the integer predicate for an operator and signedness.
///
/// Signedness selects the predicate, because LLVM integers are sign-agnostic:
/// `-1 < 1` is true under `slt` and false under `ult`.
fn int_predicate(op: BinaryOp, signed: bool) -> Option<inkwell::IntPredicate> {
    use inkwell::IntPredicate;
    Some(match (op, signed) {
        (BinaryOp::Eq, _) => IntPredicate::EQ,
        (BinaryOp::Ne, _) => IntPredicate::NE,
        (BinaryOp::Lt, true) => IntPredicate::SLT,
        (BinaryOp::Lt, false) => IntPredicate::ULT,
        (BinaryOp::Le, true) => IntPredicate::SLE,
        (BinaryOp::Le, false) => IntPredicate::ULE,
        (BinaryOp::Gt, true) => IntPredicate::SGT,
        (BinaryOp::Gt, false) => IntPredicate::UGT,
        (BinaryOp::Ge, true) => IntPredicate::SGE,
        (BinaryOp::Ge, false) => IntPredicate::UGE,
        _ => return None,
    })
}

/// Returns the float predicate for an operator.
///
/// The ordered forms are used, so a comparison with NaN is false rather than
/// true, which is what every mainstream language does.
fn float_predicate(op: BinaryOp) -> Option<inkwell::FloatPredicate> {
    use inkwell::FloatPredicate;
    Some(match op {
        BinaryOp::Eq => FloatPredicate::OEQ,
        BinaryOp::Ne => FloatPredicate::ONE,
        BinaryOp::Lt => FloatPredicate::OLT,
        BinaryOp::Le => FloatPredicate::OLE,
        BinaryOp::Gt => FloatPredicate::OGT,
        BinaryOp::Ge => FloatPredicate::OGE,
        _ => return None,
    })
}

/// Collapses a builder result into a codegen result producing a value.
///
/// Inkwell's `build_*` methods return concrete value types (`IntValue`,
/// `FloatValue`, ...), so mixing them in one `match` needs a common type. This
/// is that conversion, plus the error mapping every call site would otherwise
/// repeat.
/// Returns the function type a callable value has, if it has one.
///
/// A value of function type is called directly, and so is a pointer to one: a
/// function name used as a value decays to a pointer, and calling through it is
/// the only thing a pointer to a function is for.
fn indirect_call_type(ty: &fallc_sema::Type) -> Option<&fallc_sema::Type> {
    ty.function_type()
}

fn emitted<T: Into<BasicValueEnum<'static>>>(
    result: Result<T, inkwell::builder::BuilderError>,
) -> Result<BasicValueEnum<'static>, CodegenError> {
    result
        .map(Into::into)
        .map_err(|error| CodegenError::Emit(error.to_string()))
}

/// Which build mode code generation targets.
///
/// The difference is not optimisation: it is what the compiler does about
/// operations that are errors in debug builds and undefined in release builds.
/// Overflow and division by zero panic in debug and are left to the hardware in
/// release, so the mode changes the instructions emitted, not just their
/// quality.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuildMode {
    /// Guards arithmetic that can trap, and expects to be optimised.
    #[default]
    Debug,
    /// Emits the bare operations and relies on undefined behaviour.
    Release,
}

impl BuildMode {
    /// Returns `true` if arithmetic that can trap should be guarded.
    #[must_use]
    pub const fn checks_arithmetic(self) -> bool {
        matches!(self, BuildMode::Debug)
    }

}

/// How hard LLVM should work on the generated code.
///
/// A distinct axis from [`BuildMode`], which decides what the *source* means:
/// `--release` makes overflowing arithmetic undefined rather than a panic, and
/// that is a language-level choice, while `-O` only decides how much the
/// back end rewrites the result. They are independent because either is useful
/// alone -- `-O2` on a debug build gives fast code that still reports overflow,
/// and `--release -O0` gives a fast-to-compile build whose arithmetic is
/// undefined.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OptLevel {
    /// No optimisation. What the front end emitted, minus nothing.
    #[default]
    None,
    /// Basic local optimisation, cheap enough to be worth doing always.
    Less,
    /// The usual optimisation level.
    Default,
    /// Aggressive optimisation, including inlining across translation units.
    Aggressive,
    /// Optimise for code size.
    Size,
    /// Optimise hard for code size.
    SizeMin,
}

impl OptLevel {
    /// Parses the value of `-O`.
    ///
    /// Accepts what a C compiler accepts: a bare `-O` means `-O2`, and the
    /// named levels are spelled the way `clang` spells them.
    #[must_use]
    pub fn parse(value: &str) -> Option<OptLevel> {
        Some(match value {
            // `-O` on its own is `-O2`, as it is for `cc`.
            "" | "2" => OptLevel::Default,
            "0" => OptLevel::None,
            "1" => OptLevel::Less,
            "3" => OptLevel::Aggressive,
            // `-Os` and `-Oz` are the size-oriented levels.
            "s" => OptLevel::Size,
            "z" => OptLevel::SizeMin,
            _ => return None,
        })
    }

    /// Returns the level's spelling, for a diagnostic or a dump.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            OptLevel::None => "0",
            OptLevel::Less => "1",
            OptLevel::Default => "2",
            OptLevel::Aggressive => "3",
            OptLevel::Size => "s",
            OptLevel::SizeMin => "z",
        }
    }

    /// Returns the level in LLVM's own terms.
    #[must_use]
    pub fn as_llvm(self) -> inkwell::OptimizationLevel {
        match self {
            OptLevel::None => inkwell::OptimizationLevel::None,
            OptLevel::Less => inkwell::OptimizationLevel::Less,
            OptLevel::Default => inkwell::OptimizationLevel::Default,
            // LLVM's `Aggressive` is `-O3`; the size levels are expressed
            // through the pass manager builder's size level instead, so they
            // take the default here and are told apart below.
            OptLevel::Aggressive | OptLevel::Size | OptLevel::SizeMin => {
                inkwell::OptimizationLevel::Aggressive
            }
        }
    }

    /// Returns how much the passes should prefer small code, in LLVM's terms.
    ///
    /// `0` is no size preference and `2` is the strongest. This is what
    /// distinguishes `-Os` and `-Oz` from `-O3`, which share an
    /// [`inkwell::OptimizationLevel`].
    #[must_use]
    pub const fn size_level(self) -> u32 {
        match self {
            OptLevel::Size => 1,
            OptLevel::SizeMin => 2,
            _ => 0,
        }
    }
}

/// The result of lowering one compilation unit.
#[derive(Clone, Debug)]
pub struct LowerResult {
    /// The rendered LLVM IR, if lowering produced a module.
    pub llvm_ir: String,
    /// Object files the program must be linked with.
    ///
    /// A cached module library is kept as native object code: emitting it once
    /// and passing it to the linker is what keeps machine-code generation out
    /// of every compilation. Empty when the library was linked into the module
    /// instead (the `--emit=llvm` path, where the IR has to contain it).
    pub library_objects: Vec<std::path::PathBuf>,
    /// What went wrong.
    pub diagnostics: Vec<Diagnostic>,
}

impl LowerResult {
    /// Returns `true` if no fatal diagnostic was reported.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_fatal)
    }
}

/// Finds the functions that own `static` storage.
///
/// A call to one of them cannot be replaced by a value folded at compile time:
/// the folded evaluation treats the static as a local, while the program's call
/// reads and writes one object shared by every call. The names are collected
/// before any body is lowered, because a call may appear before the definition
/// it reaches.
#[derive(Default)]
struct StaticStorageFinder {
    found: bool,
}

impl fallc_ast::visit::Visitor for StaticStorageFinder {
    fn visit_stmt(&mut self, ast: &Ast, id: StmtId) {
        if let StmtKind::Bind(binding) = &ast.stmt(id).kind
            && binding.is_static
        {
            self.found = true;
        }
    }
}

/// How a function hands its result back to a caller.
///
/// An aggregate too large to travel through LLVM's SSA instruction selection
/// takes the classic out-of-line convention instead: the caller allocates a slot,
/// passes its address as a hidden first parameter, and the callee writes into it
/// and returns nothing. The threshold is deliberately a size limit rather than
/// "every aggregate": small records keep the value convention they always had,
/// so every existing call site, `??` result struct, and standard-library
/// signature that returns one stays unchanged.
#[derive(Clone, Debug)]
enum ReturnKind {
    /// The value is returned as LLVM's return value.
    Value,
    /// The function has a hidden first pointer parameter and returns `void`;
    /// `Type` is what the slot holds.
    OutOfLine(fallc_sema::Type),
}

/// The largest aggregate that is still returned as an ordinary LLVM value.
///
/// 128 bytes is comfortably above every record the language itself returns --
/// `std.Exception` is two 24-byte `String`s -- while keeping genuinely large
/// records, and every array, out of SSA selection. `matmul`'s
/// `[256][256]double` is half a megabyte, and LLVM 18 segfaults in
/// `SelectionDAG::ReplaceAllUsesWith` when asked to select it.
const MAX_RETURN_BY_VALUE: u64 = 128;

/// Lowers a type-checked AST into LLVM IR.
#[derive(Debug)]
pub struct Lowerer<'ctx> {
    backend: &'ctx Backend,
    diagnostics: DiagnosticBag,
    /// The function currently being lowered, if any.
    ///
    /// The LLVM builder is deliberately *not* stored here: inkwell's `Builder`
    /// is not `Copy`, and holding it on `self` while also calling `&mut self`
    /// methods would borrow-conflict on every emission. Lowering threads it
    /// through as a parameter instead.
    current_function: Option<FunctionValue<'static>>,
    /// The declared return type of the function being lowered.
    current_return_type: Option<Type>,
    /// The hidden return slot of the function being lowered, when its
    /// aggregate return type is passed out of line.
    ///
    /// `return` copies into it instead of building an aggregate value. A
    /// function that returns a small record has no slot and returns the value
    /// as it always did.
    sret_slot: Option<inkwell::values::PointerValue<'static>>,
    /// The alignment each binding asked for with `alignas`, by its name span.
    ///
    /// The checker validated the value; code generation only applies it to the
    /// storage it creates.
    binding_alignments: HashMap<Span, u64>,
    /// The names of functions that own `static` storage.
    ///
    /// A call to one is never replaced by its compile-time folded value; see
    /// [`StaticStorageFinder`].
    static_function_names: std::collections::HashSet<String>,
    /// A counter for the symbols of `static` locals.
    ///
    /// Two functions may each have a `static n`, and each needs its own object;
    /// a counter gives them distinct internal symbols without deriving one from
    /// a name that is not unique.
    static_count: u32,
    /// The locals that need destroying, innermost scope last.
    ///
    /// A binding whose type defines `__drop__` is recorded here when it is
    /// created, and every binding in the scope is destroyed when the scope
    /// exits. Keeping this parallel to `binding_scopes` means the two cannot
    /// disagree about which scope a value belongs to.
    drop_scopes: Vec<Vec<DropEntry<'static>>>,
    /// The `defer` blocks of each live scope, parallel to `drop_scopes`.
    ///
    /// They run at the same exits as destructors, innermost scope first and in
    /// reverse declaration order.
    defer_scopes: Vec<Vec<BlockId>>,
    /// The exception constructs enclosing the statement being lowered, innermost
    /// last. An escape walks it: `finally` blocks run as it passes them, and the
    /// first handler takes the exception.
    exc_frames: Vec<ExcFrame>,
    /// The type of that slot, kept so the load at the end does not rebuild it.
    exc_result_type: Option<inkwell::types::BasicTypeEnum<'static>>,
    /// The slot the current function's result is built in.
    ///
    /// A function marked `??` returns `{ ok, value, error }`, and every way out
    /// writes it here before returning: the ordinary `return`, a `raise`, and a
    /// propagated exception.
    exc_result: Option<inkwell::values::PointerValue<'static>>,
    /// `std.Exception`'s LLVM type, once found.
    exc_type: Option<inkwell::types::BasicTypeEnum<'static>>,
    /// The global a handler points at the exception it is handling, for
    /// `std.exception()`.
    handled_global: Option<inkwell::values::PointerValue<'static>>,
    /// The storage of each `except` body being lowered, innermost last: a bare
    /// `raise;` re-raises the exception it caught.
    handling: Vec<inkwell::values::PointerValue<'static>>,
    /// The `extern` variables imported modules exported, by the qualified name
    /// a program writes: `C.stdin`.
    external_globals: HashMap<String, (fallc_sema::Type, String, bool)>,
    /// Every loaded module's declared types.
    ///
    /// The exception runtime's `Exception` is declared in a module the unit may
    /// not import directly -- a program that imports `std.memory` alone loads
    /// `std.sync`, whose throwing functions need it -- so the lookup has this
    /// table to fall back on.
    module_declarations: Vec<fallc_sema::NominalType>,
    /// The qualified name of each imported variable a use names, by the span of
    /// that use. The checker recorded it, because only it can tell `C.stdin`
    /// from a field of a value called `C`.
    external_global_refs: HashMap<Span, String>,
    /// The globals module-level variables are emitted as, by name.
    ///
    /// A module-level variable is not a local: it has no alloca and no scope
    /// to look it up in. It is a `GlobalValue`, and reading or writing it goes
    /// through its address exactly as a local's does through its slot.
    globals: HashMap<
        String,
        (
            inkwell::values::GlobalValue<'static>,
            inkwell::types::BasicTypeEnum<'static>,
        ),
    >,
    /// Where each binding's storage lives, by name, innermost scope last.
    ///
    /// A name is looked up from the innermost scope outward, so an inner block
    /// can shadow an outer name and the outer one becomes visible again when
    /// the inner block ends.
    binding_scopes: Vec<HashMap<String, LocalBinding<'static>>>,
    /// Where `break` and `continue` branch to, innermost last.
    loop_stack: Vec<LoopTargets<'static>>,
    /// Blocks that some emitted branch targets.
    ///
    /// inkwell exposes no predecessor query, and a `phi`-free design still
    /// needs to know whether a join block is reachable: an unterminated block
    /// is invalid IR, and `unreachable` may only be emitted where nothing
    /// branches in. Recording every branch target answers that exactly.
    branched_to: Vec<inkwell::basic_block::BasicBlock<'static>>,
    /// Names already used for allocas, so duplicates get a suffix.
    used_binding_names: Vec<String>,
    /// How many string literals have been emitted, so each gets a unique global.
    string_literal_count: u32,
    /// The default value of each parameter, by function symbol.
    ///
    /// A default is evaluated at the call site, as in C++, so the expression
    /// is kept here and lowered where it is needed.
    function_defaults: HashMap<String, Vec<Option<ExprId>>>,
    /// The constant arguments recorded for the defaults a call left empty.
    ///
    /// A signature that came from another module, or one that was instantiated,
    /// has no expression this tree could lower: what the checker folded where
    /// the default was written is the value, and it is keyed by the call's span
    /// with the parameter's type so it can be stored at the right width.
    default_argument_values: HashMap<Span, Vec<(usize, i64, fallc_sema::Type)>>,
    /// The defaults a call left to an expression, by the call's span.
    ///
    /// The expression belongs to the module that declared the default, so it is
    /// lowered with that module's tree and tables -- which is what lets
    /// `b int = a + 1` work from another module.
    default_expressions: fallc_sema::ExpressionDefaults,
    /// The trees and tables of the imported modules, by the name a default
    /// names.
    default_trees: HashMap<String, DefaultTree>,
    /// The parameter names of each function, by the symbol it was emitted
    /// under.
    ///
    /// A default argument is an expression written against the function's own
    /// parameters, and it is evaluated at the call site -- so the call's
    /// arguments have to be bound under those names while it is lowered.
    function_parameters: HashMap<String, Vec<(String, fallc_sema::Type)>>,
    /// Every function's type, by the name the program writes for it.
    ///
    /// A function name is a value: `&f` is its address, and `p *fn(int) int =
    /// &f;` chooses the overload whose type the context wants. Code generation
    /// needs the symbol that address names, and the type is what tells two
    /// overloads apart.
    function_values: HashMap<String, Vec<(String, fallc_sema::Type)>>,
    /// The symbols overloaded functions were emitted under, by name.
    ///
    /// A mangled symbol cannot be reconstructed at the call site without
    /// repeating the mangling, so emission records it here.
    overload_symbols: HashMap<String, Vec<(Vec<Type>, String)>>,
    /// Every symbol emitted so far, so a call can find the one that matches.
    ///
    /// Recorded as functions are declared rather than derived from the call
    /// site, because the two must agree and only the declaration knows the
    /// parameter types exactly.
    emitted_symbols: Vec<String>,
    /// The instantiations the cached module library defines.
    ///
    /// A module's own check may instantiate a family member, and the body of
    /// that instantiation belongs to the module's object: it is compiled once
    /// into the cached library. This names what the library already defines, so
    /// the unit does not emit a second definition of the same symbol.
    library_instantiated: std::collections::HashSet<String>,
    /// The symbol each module-private function was emitted under, by its
    /// written name.
    ///
    /// A template body calls its own module's private functions by the name it
    /// wrote, and the instantiation is emitted where the template is used. The
    /// written name alone finds nothing there; this is the spelling the body's
    /// module emitted, and the one the call has to reach.
    module_function_symbols: HashMap<String, String>,
    /// How each declared function hands its result back, by symbol.
    ///
    /// Recorded when the signature is built, because the signature alone does
    /// not say: an out-of-line function and a `void` one both return `void`.
    /// A call site reads this rather than working the convention out again.
    return_kinds: HashMap<String, ReturnKind>,
    /// The `__as__` method each cast resolved to, by the cast's span.
    ///
    /// The checker decides this, and the symbol with it: which overload a cast
    /// selects is a question about types, and reconstructing the mangling here
    /// is how code generation came to call a symbol nothing was emitted under.
    conversions: HashMap<Span, fallc_sema::ConversionMethod>,
    /// The symbol each index expression resolved to, by the expression's span.
    ///
    /// Empty for an array or pointer, which is indexed directly.
    index_methods: HashMap<Span, String>,
    /// The symbol each `__call__` resolved to, by the call's span.
    call_methods: HashMap<Span, String>,
    /// What each `str(x)`/`repr(x)` resolved to, by the call's span; see
    /// `fallc_sema::CheckedProgram::text_calls`.
    text_calls: HashMap<Span, Option<String>>,
    /// The symbol each plain function call resolved to, by the call's span; see
    /// `fallc_sema::CheckedProgram::call_symbols`.
    call_symbols: HashMap<Span, String>,
    /// Whether the program has a `__setup__` block, and so needs its `main`
    /// wrapped.
    has_setup: bool,
    /// The declarations `@enable_if` removed, by name span.
    disabled_functions: std::collections::HashSet<Span>,
    /// The setup functions of the imported modules, in the order they run.
    module_setup_symbols: Vec<String>,
    /// The setup function of the file being compiled.
    unit_setup_symbols: Vec<String>,
    /// The call of a closure value each call site makes.
    closure_calls: HashMap<Span, fallc_sema::ClosureCall>,
    /// The value each interface conversion boxes, by the value's span.
    interface_conversions: HashMap<Span, fallc_sema::InterfaceConversion>,
    /// The interface method each virtual call reaches, by the call's span.
    interface_calls: HashMap<Span, fallc_sema::InterfaceCall>,
    /// The method table emitted for each interface and concrete type.
    interface_vtables: HashMap<String, inkwell::values::PointerValue<'static>>,
    /// The methods written against a type family's parameter, by name span.
    ///
    /// Such a function is emitted once per member of the family, as an
    /// instantiation; the declaration as written must not be emitted too.
    templated_methods: std::collections::HashSet<Span>,
    /// The lambda each call calls, by the call's span.
    lambda_calls: HashMap<Span, fallc_sema::LambdaInstance>,
    /// The lambda each value-position use denotes, by the lambda's span.
    lambda_values: HashMap<Span, fallc_sema::LambdaInstance>,
    /// The LLVM function declared for each lambda instantiation.
    lambda_functions: HashMap<String, inkwell::values::FunctionValue<'static>>,
    /// Lambda bodies declared but not yet lowered.
    pending_lambdas: Vec<PendingLambda>,
    /// The lambda bodies already emitted, by symbol.
    lambda_emitted: std::collections::HashSet<String>,
    /// The return type a lambda body is being lowered with, when it has no
    /// written type of its own.
    pre_resolved_return: Option<Type>,
    /// How many leading parameters of the function being lowered are bound as
    /// their incoming address, which is what a lambda's captures are.
    reference_params: usize,
    /// The constructor each `T(args)` call resolved to; see
    /// `fallc_sema::CheckedProgram::constructor_calls`.
    constructor_calls: HashMap<Span, String>,
    /// Every instantiation a call produced, by its key; see
    /// `fallc_sema::CheckedProgram::instantiations`.
    instantiations: HashMap<String, fallc_sema::Instantiation>,
    /// The type each `T(args)` call constructs, by the call's span.
    ///
    /// The callee names a *type*, which has no type of its own, so what is
    /// being built is recorded by the checker where it knows it.
    constructor_types: HashMap<Span, fallc_sema::Type>,
    /// The template parameters in scope while an instantiation is emitted.
    substitution: Option<fallc_sema::Substitution>,
    /// The declared type of each value template parameter in scope, so that a
    /// use of one is lowered as the array or integer it is.
    template_value_types: std::collections::HashMap<String, fallc_sema::Type>,
    /// The symbol the function being lowered is emitted under, when it is not
    /// the one its own name and parameters would give it.
    ///
    /// An instantiation is the case: its symbol was recorded by the checker,
    /// which is the only stage that knows which instantiation a call reached.
    current_symbol: Option<String>,
    /// The symbols already emitted, so a template used twice is one function.
    instantiated: std::collections::HashSet<String>,
    /// The folded value of each constant expression, by the expression's span;
    /// see `fallc_sema::CheckedProgram::constant_values`.
    constant_values: HashMap<Span, i64>,
    /// The instantiated types of the function being lowered, when it is an
    /// instantiation whose parameters were resolved by the checker.
    ///
    /// Empty for every ordinary function, whose parameters are resolved from
    /// their written types as they always were.
    pre_resolved_params: Vec<fallc_sema::Type>,
    /// The arguments passed as their address, by the argument's span; see
    /// `fallc_sema::CheckedProgram::by_address_arguments`.
    by_address_arguments: std::collections::HashSet<Span>,
    /// The operator methods the checker resolved, by the expression's span; see
    /// `fallc_sema::CheckedProgram::operator_methods`.
    operator_methods: HashMap<Span, fallc_sema::OperatorCall>,
    /// The type an argument expression adopted from its parameter, by the
    /// expression's span; see `fallc_sema::CheckedProgram::adopted_types`.
    adopted_types: HashMap<Span, fallc_sema::Type>,
    /// The constructor each string literal used as a declared type resolved to,
    /// by the literal's span; see
    /// `fallc_sema::CheckedProgram::literal_constructions`.
    literal_constructions: HashMap<Span, String>,
    /// The type each such literal is constructed into, by the literal's span.
    ///
    /// The symbol says which constructor, and the constructor's receiver says
    /// the type -- but reading a type out of a mangled symbol is guesswork, so
    /// the checker records it directly.
    literal_construction_types: HashMap<Span, fallc_sema::Type>,
    /// The storage holding a value the compiler produced, by the expression's
    /// span.
    ///
    /// A rendering has to be written somewhere before it can be used, and it
    /// is a value of a type with methods -- `str(x).data()` is the whole point
    /// of it. A local variable would have a name to look the storage up by;
    /// this has only a span, so that is the key.
    temporaries: HashMap<Span, LocalBinding<'static>>,
    /// The symbol each `__bool__` resolved to, by the condition's span.
    bool_methods: HashMap<Span, String>,
    /// The symbol each unary operator resolved to, by the expression's span.
    unary_methods: HashMap<Span, String>,
    /// The symbol each compound assignment resolved to, by the statement's span.
    in_place_methods: HashMap<Span, String>,
    /// Whether each compound assignment passes its operand by address, by the
    /// statement's span; see `fallc_sema::CheckedProgram::in_place_by_address`.
    in_place_by_address: HashMap<Span, bool>,
    /// The constructor each `T x = expr;` resolved to, by the initialiser's span.
    init_calls: HashMap<Span, String>,
    /// The symbol each method call resolved to, by the call's span.
    ///
    /// The checker decides this, because only it knows which definition the
    /// call selected.
    method_calls: HashMap<Span, String>,
    /// The calls whose callee may raise, so a check follows each one.
    throwing_calls: std::collections::HashSet<Span>,
    /// Those written with `??`, whose exception leaves the function rather than
    /// going to a handler in it.
    propagating_calls: std::collections::HashSet<Span>,
    /// The bindings of the function being lowered, kept so the escape paths --
    /// which run `finally` blocks -- can lower statements.
    current_bindings: BindingTypes,
    /// How many `expr!!` are being lowered around the current position.
    ///
    /// Nonzero means a raise is not an escape but a stop: `!!` is `try { .. }
    /// except { std.panic(); }`, so the unwrap of a throwing call checks this
    /// and panics instead of looking for a handler.
    abort_lowering: u32,
    /// The symbol each plain assignment resolved to, by the statement's span.
    assign_methods: HashMap<Span, String>,
    /// Where a `__deref__` result is meant as a value rather than a reference,
    /// the referent to read, by the expression's span.
    ///
    /// The checker decides it: whether a reference is read or passed on depends
    /// on what the surrounding expression wants, and only the checker knows.
    deref_reads: HashMap<Span, Type>,
    /// The type each `->` chain ends at, by the field access's span.
    pointer_targets: HashMap<Span, Type>,
    /// The type each `type(expr)` names, by the span of the written form.
    ///
    /// The checker answered the question where the expression was in hand;
    /// this is the copy code generation reads when it resolves a written type
    /// itself.
    type_of_types: HashMap<Span, Type>,
    /// Which explicit `__init__`/`__drop__` calls were answered by a plain
    /// store or a field-wise destroy, by the call's span.
    ///
    /// The checker decides it, because whether the substituted type declares
    /// the method is only known while the instantiation is checked.
    in_place_operations: HashMap<Span, fallc_sema::InPlaceOperation>,
    /// Every `if` the checker decided at compile time, by the condition's span.
    compile_time_ifs: HashMap<Span, bool>,
    /// What each `type Name = Other;` alias stands for, by name.
    ///
    /// The checker resolves aliases too; this copy exists because code
    /// generation resolves a few written types on its own -- a parameter's, an
    /// instantiated signature's -- and an alias has to mean the same thing in
    /// both places.
    aliases: HashMap<String, fallc_sema::Type>,
    /// The declared types, by name, so a written type resolves to a nominal
    /// one.
    ///
    /// The checker has the same table; code generation needs its own copy
    /// because it resolves a few types on its own, such as a constructor's
    /// receiver.
    nominal_types: HashMap<String, fallc_sema::NominalType>,
    /// The names of imported modules.
    ///
    /// Needed to tell `C.printf(..)` from `p.show()`, which are written
    /// identically.
    module_names: std::collections::HashSet<String>,
    /// The name of the module whose items are being lowered now.
    ///
    /// A module-level variable is one object per program, so its linker symbol
    /// may not be its bare name: a program that declares `out`, or a second
    /// module with a `state`, would collide with another module's object of the
    /// same name. The symbol is `module.name`; the internal tables of the module
    /// itself are keyed by the written name.
    current_module: String,
    /// Whether private free functions are emitted under the module prefix.
    ///
    /// A single-translation-unit program has no second unit for a private name
    /// to collide with, so it keeps the plain symbol; a program of several
    /// translation units always prefixes them, and imported modules are
    /// prefixed in either case.
    private_function_prefix: bool,
    /// The literal whose construction is being lowered.
    ///
    /// Its own argument lowering would otherwise see the recorded construction
    /// and construct it again, recursively: what is wanted inside is the
    /// literal's value, not the construction.
    lowering_literal: Option<fallc_span::Span>,
    /// External functions available to this unit, by qualified name.
    ///
    /// An `extern "C" export fn printf(..) int;` in an imported module is not
    /// defined here, so a call to it needs an LLVM declaration rather than a
    /// lookup that would fail.
    external_symbols: HashMap<String, ExternalSignature>,
    /// The linker symbol each imported name must call, by its local name.
    ///
    /// Recorded rather than derived: a name imported from another module is a
    /// local name here and the defining module's own symbol at link time, and
    /// the two can differ in every way. `from C import printf` calls `printf`,
    /// and `import C as Libc` makes `Libc.printf` call it too.
    imported_symbols: HashMap<String, (String, String)>,
    /// Whether trap-prone operations are guarded.
    mode: BuildMode,
    /// Where module bitcode is cached, when the driver enabled the cache.
    module_cache: Option<std::path::PathBuf>,
    /// Whether the module library should be kept as native object code.
    ///
    /// True when the program will be linked: the library's machine code is
    /// emitted once into the cache and passed to the linker, instead of being
    /// linked into this module and generated again for every program.
    object_library: bool,
    /// The optimisation level the library object is built at.
    optimization: OptLevel,
    /// Object files this lowering needs linked with it; see
    /// [`LowerResult::library_objects`].
    library_objects: Vec<std::path::PathBuf>,
}


mod install;
mod functions;
mod statements;
mod exceptions;
mod expressions;
mod interfaces;
mod lambdas;
mod arithmetic;
mod stores;
mod calls;
mod external;
pub mod cache;
mod arguments;
mod methods;
mod builtins;
mod runtime;
mod diagnostics;


impl Codegen for Lowerer<'_> {
    fn emit_item(&mut self, ast: &Ast, item: ItemId) -> Result<(), CodegenError> {
        // Items are emitted by `lower_items`, which the walk calls directly, so
        // this entry point has nothing to emit.
        let _ = (ast, item);
        Err(CodegenError::waiting_for_spec(
            "this item form",
            ast.item(item).span,
        ))
    }

    fn emit_stmt(
        &mut self,
        ast: &Ast,
        stmt: StmtId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // Statements are emitted by `lower_stmt`, which the walk calls
        // directly, so this entry point has nothing to emit.
        let _ = ast;
        Err(CodegenError::waiting_for_spec(
            "this statement form",
            ast.stmt(stmt).span,
        ))
    }

    fn emit_expr(
        &mut self,
        ast: &Ast,
        expr: ExprId,
    ) -> Result<Option<BasicValueEnum<'static>>, CodegenError> {
        // Expressions are emitted by `lower_expr`, which the walk calls
        // directly, so this entry point has nothing to emit.
        let _ = ast;
        Err(CodegenError::waiting_for_spec(
            "this expression form",
            ast.expr(expr).span,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_an_empty_ast_yields_a_valid_empty_module() {
        let backend = Backend::new("test");
        let mut lowerer = Lowerer::new(&backend);
        let result = lowerer.lower(&Ast::new(), &TypeTable::new(), &BindingTypes::new());

        assert!(result.is_ok());
        assert!(
            result.llvm_ir.contains("ModuleID = 'test'"),
            "{}",
            result.llvm_ir
        );
    }

    #[test]
    fn fallthrough_is_the_documented_decision_point() {
        let backend = Backend::new("test");
        let mut lowerer = Lowerer::new(&backend);
        let error = lowerer
            .lower_fallthrough(Span::SYNTHETIC)
            .expect_err("no control-flow spec yet, so this must report rather than guess");
        assert!(matches!(error, CodegenError::WaitingForSpec { .. }));
        assert!(error.to_string().contains("fallthrough"));
    }

    #[test]
    fn emitting_an_item_reports_rather_than_guesses() {
        use fallc_ast::{ItemKind, Unresolved};
        use fallc_span::FileId;

        let span = Span::new(FileId::from_raw(0), 0, 3);
        let mut ast = Ast::new();
        let item = ast
            .builder()
            .alloc_item(ItemKind::Unresolved(Unresolved::new("x", span)), span);
        ast.builder().push_item(item);

        let backend = Backend::new("test");
        let mut lowerer = Lowerer::new(&backend);
        assert!(lowerer.emit_item(&ast, item).is_err());
    }
}

