//! What a module offers to its importers.
//!
//! The exports of a file are collected here: its signatures, its declared
//! types, its constants and its traits, in the form the importing module can
//! install without seeing the declaring tree.

use super::*;

/// Returns the nominal types a tree declares, for resolving written type names.
///
/// A signature written in terms of a declared type has to resolve that name,
/// and the type table it belongs to is the declaring module's.
pub(super) fn declared_types_of(ast: &Ast) -> Vec<fallc_sema::NominalType> {
    let mut types = Vec::new();
    for &item in &ast.items {
        let fallc_ast::ItemKind::TypeDeclaration(declaration) = &ast.item(item).kind else {
            continue;
        };
        // The fields are resolved by the checker; here only the *names* are
        // needed, so a field's type stays unspecified and is filled in when the
        // module is checked.
        types.push(fallc_sema::NominalType {
            mutable_fields: Vec::new(),
            field_alignments: Vec::new(),
            // `type Foo;` names a type whose definition is elsewhere, so the
            // declaration itself is incomplete -- and so is what an importer
            // sees of it.
            is_incomplete: declaration.is_forward,
            // The kind is not carried across a module boundary yet, so an
            // importer reads a union as a struct. Conservatively wrong for a
            // union, and the reason one should not cross a boundary for now.
            is_union: false,
            union_storage: None,
            template_arguments: None,
            name: declaration.name.clone(),
            fields: declaration
                .fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        resolve_signature_type_with(&field.ty, false, &[]),
                    )
                })
                .collect(),
        });
    }
    types
}

/// Converts a signature into the resolved form code generation needs.
pub(super) fn external_signature(
    signature: &fallc_sema::FunctionSignature,
    declared: &[fallc_sema::NominalType],
) -> fallc_ir::ExternalSignature {
    // A signature collected from a checked module carries the resolution its
    // own checker produced. Using it is what keeps the two stages in agreement:
    // a type worked out again here could name a different family member, and
    // then the caller asks for a symbol the module never emitted.
    if let Some(resolved) = &signature.resolved {
        return fallc_ir::ExternalSignature {
            params: resolved.params.clone(),
            param_names: signature.param_names.clone(),
            returns: resolved.returns.clone(),
            variadic: signature.variadic,
            throws: signature.throws,
            symbol: resolved.symbol.clone(),
        };
    }
    // A declared type's name is not a primitive, so it has to be looked up in
    // the declaring module's own types. Without this the receiver `String`
    // came out as the literal text `string` -- the module's file stem -- and
    // the symbol a caller built did not match the one the module emitted.
    let resolve = |ty: &fallc_ast::TypeSyntax, in_parameter: bool| {
        resolve_signature_type_with(ty, in_parameter, declared)
    };
    let params: Vec<fallc_sema::Type> = signature
        .param_types
        .iter()
        .map(|ty| resolve(ty, true))
        .collect();
    // The symbol a module's function is emitted under, built from the same
    // parameters by the same rule every other function uses. Without this the
    // call site asked for the bare name while the module emitted a mangled one,
    // and the link failed with an undefined symbol.
    //
    // A function implemented elsewhere keeps its declared name: `@extern_C`
    // names a symbol the C library exports under exactly that spelling, and
    // mangling it would look for something that was never there.
    let symbol = if signature.instantiated {
        // Generated per use: there is no emitted symbol, and inventing one from
        // the generic signature would collide with a concrete type of the same
        // name.
        String::new()
    } else if signature.abi.is_some() {
        signature.written_name.clone()
    } else {
        fallc_ast::special::symbol_for(
            &signature.written_name,
            &params.iter().map(fallc_sema::mangle_type).collect::<Vec<_>>(),
        )
    };
    fallc_ir::ExternalSignature {
        params,
        param_names: signature.param_names.clone(),
        returns: resolve(&signature.return_type, false),
        variadic: signature.variadic,
        // A `??` function's result carries the exception, so a caller in
        // another module has to build the same shape for the call.
        throws: signature.throws,
        symbol,
    }
}

/// The names a tree's type families use for their parameters.
///
/// A method of a family is written against these -- `this &Crate<T>` -- and is
/// a template in every way that matters: it has one body and one member per set
/// of arguments, so the driver files its body with the templates'.
pub(super) fn family_parameter_names(ast: &Ast) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for &item in &ast.items {
        if let fallc_ast::ItemKind::TypeDeclaration(declaration) = &ast.item(item).kind {
            for parameter in &declaration.template_params {
                names.insert(parameter.name.clone());
            }
        }
    }
    names
}

/// Whether a written type mentions one of `names`.
pub(super) fn mentions_name(
    ty: &fallc_ast::TypeSyntax,
    names: &std::collections::HashSet<String>,
) -> bool {
    use fallc_ast::{TemplateArgumentSyntax, TypeSyntax};
    match ty {
        TypeSyntax::Named(named) => {
            names.contains(&named.path)
                || named.arguments.iter().any(|argument| match argument {
                    TemplateArgumentSyntax::Type(inner) => mentions_name(inner, names),
                    TemplateArgumentSyntax::Value(_) => false,
                    // A value argument's name may be the family's own value
                    // parameter -- `Device<Mode>` -- which is exactly what makes
                    // the function a method of the family.
                    TemplateArgumentSyntax::Named { path, .. } => names.contains(path),
                })
        }
        TypeSyntax::Array(array) => mentions_name(&array.element, names),
        TypeSyntax::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| mentions_name(parameter, names))
                || mentions_name(&function.returns, names)
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
        | TypeSyntax::Const(inner) => mentions_name(inner, names),
        TypeSyntax::Member(member) => mentions_name(&member.outer, names),
        TypeSyntax::TypeOf(_)
        | TypeSyntax::DecltypeAuto(_)
        | TypeSyntax::Auto(_)
        | TypeSyntax::Unresolved(_) => false,
    }
}

/// Whether a function is a method of a type family.
///
/// Its receiver is a member written with the family's parameters, so the
/// function is not a function: each member of the family gets its own copy of
/// the body, checked and emitted from an instantiation.
pub(super) fn method_of_family(
    function: &fallc_ast::FunctionItem,
    names: &std::collections::HashSet<String>,
) -> bool {
    function
        .params
        .first()
        .is_some_and(|parameter| mentions_name(&parameter.ty, names))
}

/// The symbol a templated function would be emitted under.
///
/// Built by the same rule every function's is -- the written name, then the
/// mangled parameter types -- because that is the key its instantiation is
/// looked up by. The written parameters name template parameters, which mangle
/// as themselves; the instantiation's *resolved* parameters are what the
/// definition emits, and the link between the two is the template.
pub(super) fn symbol_of_template(ast: &Ast, function: &fallc_ast::FunctionItem) -> String {
    let _ = ast;
    let _ = ast;
    // One rule, in one place: the checker rebuilds this key from the signature
    // a call selected, so the two must not be two implementations.
    let types: Vec<fallc_ast::TypeSyntax> =
        function.params.iter().map(|param| param.ty.clone()).collect();
    fallc_sema::template_symbol_of(&function.name, &types)
}

/// Returns the name a function is bound under, when it differs from the written
/// one.
///
/// A special method is bound with its receiver's type appended, so `__init__`
/// on `String` is `__init__String`.
///
/// # One implementation
///
/// This was a copy of `fallc-sema`'s rule, and the copy is what broke: it
/// recognised only a *pointer* receiver, so when `this` became a reference the
/// exported name silently lost its type. The module exported `__init__` while
/// the importer asked for `__init__String`, and no imported type could be
/// constructed. There is one rule now, and it is the resolver's.
pub(super) fn definition_name(function: &fallc_ast::FunctionItem) -> String {
    fallc_sema::definition_name(function)
}

/// Replaces every use of one of this module's aliases with what it names.
///
/// An alias is the declaring module's business: `type Id = usize;` may name a
/// field's type, and an importer has to see `usize` -- it has no alias to
/// consult, because the alias was never exported. Substituting before the type
/// leaves the module keeps the equivalence without publishing a private name.
fn substitute_aliases(
    ty: &fallc_ast::TypeSyntax,
    aliases: &std::collections::HashMap<String, fallc_ast::TypeSyntax>,
) -> fallc_ast::TypeSyntax {
    substitute_aliases_within(ty, aliases, 0)
}

/// The recursion, with a depth limit so that `type A = B; type B = A;` -- which
/// is refused where it is written -- cannot make this loop.
fn substitute_aliases_within(
    ty: &fallc_ast::TypeSyntax,
    aliases: &std::collections::HashMap<String, fallc_ast::TypeSyntax>,
    depth: u32,
) -> fallc_ast::TypeSyntax {
    use fallc_ast::{TemplateArgumentSyntax, TypeSyntax};
    let inner = |ty: &fallc_ast::TypeSyntax| {
        Box::new(substitute_aliases_within(ty, aliases, depth))
    };
    match ty {
        TypeSyntax::Named(named) => {
            if named.arguments.is_empty()
                && depth < 16
                && let Some(target) = aliases.get(&named.path)
            {
                return substitute_aliases_within(target, aliases, depth + 1);
            }
            TypeSyntax::Named(fallc_ast::NamedType {
                path: named.path.clone(),
                arguments: named
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        TemplateArgumentSyntax::Type(argument) => {
                            TemplateArgumentSyntax::Type(*inner(argument))
                        }
                        other => other.clone(),
                    })
                    .collect(),
                is_expansion: named.is_expansion,
                span: named.span,
            })
        }
        TypeSyntax::Array(array) => TypeSyntax::Array(fallc_ast::ArrayType {
            len: array.len,
            len_span: array.len_span,
            element: inner(&array.element),
        }),
        TypeSyntax::Function(function) => {
            TypeSyntax::Function(fallc_ast::FunctionTypeSyntax {
                abi: function.abi.clone(),
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| substitute_aliases_within(parameter, aliases, depth))
                    .collect(),
                variadic: function.variadic,
                throws: function.throws,
                returns: inner(&function.returns),
                span: function.span,
            })
        }
        TypeSyntax::Member(member) => TypeSyntax::Member(fallc_ast::TypeMember {
            outer: inner(&member.outer),
            name: member.name.clone(),
            span: member.span,
        }),
        TypeSyntax::UnsizedArray(inner_ty) => TypeSyntax::UnsizedArray(inner(inner_ty)),
        TypeSyntax::Pointer(inner_ty) => TypeSyntax::Pointer(inner(inner_ty)),
        TypeSyntax::PointerToConst(inner_ty) => TypeSyntax::PointerToConst(inner(inner_ty)),
        TypeSyntax::ConstPointer(inner_ty) => TypeSyntax::ConstPointer(inner(inner_ty)),
        TypeSyntax::Reference(inner_ty) => TypeSyntax::Reference(inner(inner_ty)),
        TypeSyntax::ConstReference(inner_ty) => TypeSyntax::ConstReference(inner(inner_ty)),
        TypeSyntax::RvalueReference(inner_ty) => TypeSyntax::RvalueReference(inner(inner_ty)),
        TypeSyntax::ConstRvalueReference(inner_ty) => {
            TypeSyntax::ConstRvalueReference(inner(inner_ty))
        }
        TypeSyntax::Volatile(inner_ty) => TypeSyntax::Volatile(inner(inner_ty)),
        TypeSyntax::Const(inner_ty) => TypeSyntax::Const(inner(inner_ty)),
        TypeSyntax::Auto(_)
        | TypeSyntax::DecltypeAuto(_)
        | TypeSyntax::TypeOf(_)
        | TypeSyntax::Unresolved(_) => ty.clone(),
    }
}

/// Replaces every nominal a type names with the module's finished declaration.
///
/// A type is resolved while it is being declared, so a field that names the type
/// itself -- `next *Node` -- gets whatever `Node` looked like at that moment,
/// which is a name with no fields. The declarations handed here are the finished
/// ones, so a value built from them has a layout and fields.
fn refresh_nominals(
    ty: fallc_sema::Type,
    declared: &[fallc_sema::NominalType],
) -> fallc_sema::Type {
    use fallc_sema::Type;
    match ty {
        Type::Nominal(nominal) => Type::Nominal(
            match declared.iter().find(|found| found.name == nominal.name) {
                Some(found) => found.clone(),
                None => nominal,
            },
        ),
        Type::Pointer {
            pointee,
            pointee_is_const,
            pointer_is_const,
        } => Type::Pointer {
            pointee: Box::new(refresh_nominals(*pointee, declared)),
            pointee_is_const,
            pointer_is_const,
        },
        Type::Reference {
            referent,
            referent_is_const,
            is_rvalue,
        } => Type::Reference {
            referent: Box::new(refresh_nominals(*referent, declared)),
            referent_is_const,
            is_rvalue,
        },
        Type::Const(inner) => Type::Const(Box::new(refresh_nominals(*inner, declared))),
        Type::Array { element, len } => Type::Array {
            element: Box::new(refresh_nominals(*element, declared)),
            len,
        },
        Type::UnsizedArray(element) => {
            Type::UnsizedArray(Box::new(refresh_nominals(*element, declared)))
        }
        other => other,
    }
}

/// Collects a module's exported function signatures.
///
/// Only `export` items are included: a module's declarations are private by
/// default, so an unexported one is invisible to importers and must not appear
/// here, or the checker would accept a call the language forbids.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_exports(
    ast: &Ast,
    constants: &std::collections::HashMap<String, i64>,
    declared: &[fallc_sema::NominalType],
    type_of_types: &std::collections::HashMap<fallc_span::Span, fallc_sema::Type>,
    binding_types: &fallc_sema::BindingTypes,
    disabled: &std::collections::HashSet<fallc_span::Span>,
    default_values: &std::collections::HashMap<fallc_span::Span, i64>,
    // The folded value of every constant expression, by its span. A type
    // member's constant is published by the number its own tree folded to,
    // because the importer has no initialiser to fold.
    member_constants: &std::collections::HashMap<fallc_span::Span, i64>,
    module_name: &str,
) -> fallc_sema::ExternalModule {
    let mut exports = fallc_sema::ExternalModule::default();
    // The family parameters, so a method written against them is not given a
    // resolved signature: `Crate<T>` has no resolved form until a member is
    // instantiated, and its symbol is the instantiation's.
    let families = family_parameter_names(ast);
    // The aliases this module declares. A type written through one has to leave
    // as the type it names, because the alias itself stays private.
    let aliases: std::collections::HashMap<String, fallc_ast::TypeSyntax> = ast
        .items
        .iter()
        .filter_map(|item| match &ast.item(*item).kind {
            fallc_ast::ItemKind::TypeDeclaration(declaration) => match &declaration.kind {
                fallc_ast::TypeDeclKind::Alias(target) if declaration.pattern.is_empty() => {
                    Some((declaration.name.clone(), target.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    for &item in &ast.items {
        // An `extern` variable is exported like a function is: the name, the
        // type it is read as, and the symbol the linker resolves. `C.stdin` is
        // the one this exists for.
        if let fallc_ast::ItemKind::ExternVariable(variable) = &ast.item(item).kind
            && variable.exported
        {
            exports.variables.insert(
                variable.name.clone(),
                fallc_sema::ExportedVariable {
                    symbol: variable.name.clone(),
                    ty: variable.ty.clone(),
                },
            );
        }
        // A type is published on the same terms as a function: without
        // `export` its name is this module's own business, so an importer
        // cannot name it.
        if let fallc_ast::ItemKind::TypeDeclaration(declaration) = &ast.item(item).kind {
            // A declaration says the name exists; a definition says what it is.
            // When a module has both, the definition is what an importer has to
            // see -- so a declaration standing *after* the definition must not
            // put the type back to incomplete, which made every importer of a
            // complete type report it as only declared.
            let already_defined = exports
                .types
                .get(&declaration.name)
                .is_some_and(|existing| !existing.is_forward);
            if declaration.exported && !(declaration.is_forward && already_defined) {
                exports.add_type(
                    declaration.name.clone(),
                    fallc_sema::ExportType {
                        name: declaration.name.clone(),
                        fields: declaration
                            .fields
                            .iter()
                            .map(|field| fallc_sema::ExportedField {
                                name: field.name.clone(),
                                ty: substitute_aliases(&field.ty, &aliases),
                                // The checker's answer, with the module's
                                // *finished* declarations substituted for the
                                // nominals it names. A field written `next
                                // *Node` was resolved while `Node` was still
                                // being declared -- its own fields not filled
                                // in yet -- and publishing that answer gave an
                                // importer a `Node` with no fields, so
                                // `first->next->value` reported no such field.
                                resolved: type_of_types
                                    .get(&field.ty.span())
                                    .cloned()
                                    .map(|ty| refresh_nominals(ty, declared)),
                                mutable: field.mutable,
                                // The layout is the declaring module's to
                                // measure: an `alignas` is an expression in its
                                // tree, so what travels is the number.
                                align: declared
                                    .iter()
                                    .find(|ty| ty.name == declaration.name)
                                    .and_then(|ty| {
                                        ty.field_alignments
                                            .iter()
                                            .find(|(name, _)| *name == field.name)
                                            .map(|(_, align)| *align)
                                    }),
                            })
                            .collect(),
                        // With the parameters, so that an importer can write
                        // `Array<int>` rather than only `Array`.
                        template_params: declaration.template_params.clone(),
                        // The types declared inside it travel too: they are
                        // named through this one, and an importer that cannot
                        // see them cannot write `Array<int>.Iterator`.
                        nested: declaration
                            .nested
                            .iter()
                            .filter_map(|nested| match &nested.kind {
                                fallc_ast::TypeDeclKind::Alias(target) => Some((
                                    nested.name.clone(),
                                    substitute_aliases(target, &aliases),
                                )),
                                _ => None,
                            })
                            .collect(),
                        // The kind travels with the fields: a union read as a
                        // struct would lay its members side by side, and every
                        // value crossing the boundary would be wrong.
                        is_union: declaration.kind == fallc_ast::TypeDeclKind::Union,
                        // A trait family answers through a constant member.
                        // The value is folded here, where the initialiser's
                        // tree is in hand, so an importer can read the answer
                        // without re-evaluating an expression it does not own.
                        constants: declaration
                            .constants
                            .iter()
                            .filter_map(|constant| {
                                member_constants
                                    .get(&ast.expr(constant.value).span)
                                    .map(|value| (constant.name.clone(), *value))
                            })
                            .collect(),
                        // A declaration without fields is exported as an
                        // *incomplete* type, so an importer may name it behind
                        // a pointer without inventing a layout.
                        is_forward: declaration.is_forward,
                        // A family's specialisations go with the family: a
                        // member's layout is decided here, and an importer that
                        // did not know the specialisation would build the
                        // generic one and read the fields wrong.
                        alias: match &declaration.kind {
                            fallc_ast::TypeDeclKind::Alias(target) => Some(target.clone()),
                            _ => None,
                        },
                        specialisations: ast
                            .items
                            .iter()
                            .filter_map(|item| match &ast.item(*item).kind {
                                fallc_ast::ItemKind::TypeDeclaration(spec)
                                    if spec.name == declaration.name
                                        && !spec.pattern.is_empty() =>
                                {
                                    let pattern: Vec<fallc_sema::Type> = spec
                                        .pattern
                                        .iter()
                                        .map(|written| {
                                            let written =
                                                substitute_aliases(written, &aliases);
                                            resolve_signature_type_with(
                                                &written, false, declared,
                                            )
                                        })
                                        .collect();
                                    // The member's name, so that a member this
                                    // module already built hands over the
                                    // layout it measured.
                                    let arguments = fallc_sema::Arguments(
                                        pattern
                                            .iter()
                                            .cloned()
                                            .map(fallc_sema::TemplateArgument::Type)
                                            .collect(),
                                    );
                                    let member_name =
                                        format!("{}<{}>", spec.name, arguments.spelling());
                                    Some(fallc_sema::ExportedSpecialisation {
                                        params: spec
                                            .template_params
                                            .iter()
                                            .cloned()
                                            .map(|mut param| {
                                                // The default is an expression of
                                                // this tree; a pattern does not
                                                // use it.
                                                param.default = None;
                                                param
                                            })
                                            .collect(),
                                        pattern,
                                        fields: spec
                                            .fields
                                            .iter()
                                            .map(|field| {
                                                let written =
                                                    substitute_aliases(&field.ty, &aliases);
                                                (
                                                    field.name.clone(),
                                                    resolve_signature_type_with(
                                                        &written, false, declared,
                                                    ),
                                                )
                                            })
                                            .collect(),
                                        is_union: spec.kind == fallc_ast::TypeDeclKind::Union,
                                        mutable_fields: spec
                                            .fields
                                            .iter()
                                            .filter(|field| field.mutable)
                                            .map(|field| field.name.clone())
                                            .collect(),
                                        // The member's layout, when this module
                                        // built that member: an `alignas` is an
                                        // expression of this tree, so an importer
                                        // has nothing to evaluate.
                                        alignments: declared
                                            .iter()
                                            .find(|ty| ty.name == member_name)
                                            .map(|ty| ty.field_alignments.clone())
                                            .unwrap_or_default(),
                                        // The specialised member's answers:
                                        // `IsInteger<int>`'s `VALUE` is true,
                                        // and the importer has no tree to fold
                                        // the initialiser in.
                                        constants: spec
                                            .constants
                                            .iter()
                                            .filter_map(|constant| {
                                                member_constants
                                                    .get(&ast.expr(constant.value).span)
                                                    .map(|value| (constant.name.clone(), *value))
                                            })
                                            .collect(),
                                    })
                                }
                                _ => None,
                            })
                            .collect(),
                    },
                );
            }
            continue;
        }
        // A module-level variable is one object for the whole program, so an
        // exported one is published as its symbol and its type. `auto` is why
        // the type is resolved here: only this module can deduce it.
        if let fallc_ast::ItemKind::Variable(variable) = &ast.item(item).kind
            && variable.exported
        {
            let ty = match &variable.ty {
                fallc_ast::BindingType::Written(written) => type_of_types
                    .get(&written.span())
                    .cloned()
                    .unwrap_or_else(|| {
                        resolve_signature_type_with(written, false, declared)
                    }),
                // The checker deduced it. A binding's type is filed by its
                // name's span, which is what every other declaration uses.
                fallc_ast::BindingType::Auto(span) => binding_types
                    .get(variable.name_span)
                    .cloned()
                    .unwrap_or_else(|| fallc_sema::Type::unspecified("auto", *span)),
            };
            exports.globals.insert(
                variable.name.clone(),
                fallc_sema::ExportedGlobal {
                    symbol: format!("{module_name}.{}", variable.name),
                    ty,
                    // A deprecation is part of what the declaration publishes.
                    deprecated: fallc_sema::deprecated_message(
                        ast,
                        &ast.item(item).attrs,
                    ),
                    thread_local: variable.is_thread_local,
                },
            );
        }
        // A trait is published as a name. Its requirements are expressions, so
        // they travel with this module's tree -- see `LoadedImports`' -- and
        // the name here is what lets `from m import Trait` name it.
        if let fallc_ast::ItemKind::Trait(declaration) = &ast.item(item).kind
            && declaration.exported
        {
            exports.traits.insert(declaration.name.clone());
        }
        // An enum is exported as a type and as the value of each member: the
        // members are folded here, and a program that imported the enum writes
        // `Color.RED` and gets the number this module gave it.
        if let fallc_ast::ItemKind::EnumDeclaration(declaration) = &ast.item(item).kind
            && declaration.exported
        {
            let members: Vec<(String, i64)> = declaration
                .members
                .iter()
                .map(|member| {
                    let qualified = format!("{}.{}", declaration.name, member.name);
                    // A member's value is folded by the module's own check,
                    // which is where its expression's tree was in hand. A
                    // missing value means the check reported it already.
                    (member.name.clone(), constants.get(&qualified).copied().unwrap_or(0))
                })
                .collect();
            exports.add_enum(
                declaration.name.clone(),
                fallc_sema::ExportEnum {
                    name: declaration.name.clone(),
                    base: declaration
                        .base
                        .as_ref()
                        .map(|base| substitute_aliases(base, &aliases)),
                    members,
                },
            );
        }
        // An interface travels as its method table: its value's layout is
        // the same everywhere, and what an importer cannot invent is which
        // methods the table holds, in which order.
        if let fallc_ast::ItemKind::Interface(declaration) = &ast.item(item).kind
            && declaration.exported
        {
            exports.add_interface(
                declaration.name.clone(),
                fallc_sema::ExportInterface {
                    name: declaration.name.clone(),
                    template_params: declaration.template_params.clone(),
                    methods: declaration
                        .methods
                        .iter()
                        .map(|method| fallc_sema::ExportInterfaceMethod {
                            name: method.name.clone(),
                            // A virtual call is made by position: the
                            // restriction states types, not names, so there
                            // are none for a caller to write.
                            params: method
                                .params
                                .iter()
                                .map(|param| substitute_aliases(param, &aliases))
                                .collect(),
                            returns: substitute_aliases(&method.returns, &aliases),
                            throws: method.throws,
                        })
                        .collect(),
                    is_forward: declaration.is_forward,
                },
            );
        }
        // A constant is exported too, so that `C.EOF` or a module's own
        // `INITIAL_CAPACITY` can be named through the module that owns it.
        // Its value is folded in the declaring module, whose tree the
        // expression belongs to.
        if let fallc_ast::ItemKind::Const(constant) = &ast.item(item).kind {
            if constant.exported
                && let Some(value) = constants.get(&constant.name)
            {
                exports.add_constant(constant.name.clone(), *value);
                // The type travels with the value: it is what tells an importer
                // how many bytes the constant is, and the written form is what
                // the importer resolves in its own table.
                exports
                    .constant_types
                    .insert(constant.name.clone(), constant.ty.clone());
            }
            continue;
        }
        let fallc_ast::ItemKind::Function(function) = &ast.item(item).kind else {
            continue;
        };
        if !function.exported {
            continue;
        }
        // A declaration `@enable_if` removed for this build is not exported
        // either: publishing it would let an importer call a function nothing
        // defines, and the link would fail with no diagnostic to explain it.
        if disabled.contains(&function.name_span) {
            continue;
        }
        // The signature as the checker resolved it: a written `Crate<int>`
        // names one member of a family, and only this module knows which. The
        // answers travel with the export so an importer builds the same call
        // -- and asks for the same symbol -- as the definition emitted.
        let resolve = |ty: &fallc_ast::TypeSyntax, in_parameter: bool| {
            type_of_types
                .get(&ty.span())
                .cloned()
                .unwrap_or_else(|| resolve_signature_type_with(ty, in_parameter, declared))
        };
        let resolved = if function.template_params.is_empty() && !function.has_pack()
            && !method_of_family(function, &families)
        {
            let params: Vec<fallc_sema::Type> = function
                .params
                .iter()
                .map(|param| resolve(&param.ty, true))
                .collect();
            // A function implemented elsewhere keeps its declared name: the C
            // library exports it under exactly that spelling, and the linker
            // looks for nothing else.
            let symbol = if function.body.is_none() {
                function.name.clone()
            } else {
                fallc_ast::special::symbol_for(
                    &function.name,
                    &params.iter().map(fallc_sema::mangle_type).collect::<Vec<_>>(),
                )
            };
            Some(fallc_sema::ResolvedSignature {
                params,
                returns: resolve(&function.return_type, false),
                symbol,
            })
        } else {
            None
        };
        let signature = fallc_sema::FunctionSignature {
            param_names: function.params.iter().map(|p| p.name.clone()).collect(),
            param_types: function.params.iter().map(|p| p.ty.clone()).collect(),
            return_type: function.return_type.clone(),
            variadic: function.variadic,
            required_params: function
                .params
                .iter()
                .position(|param| param.default.is_some())
                .unwrap_or(function.params.len()),
            defaults: function.params.iter().map(|p| p.default).collect(),
            // Where the defaults are written. A default that is not a constant
            // is an expression in *this* module's tree, so a caller in another
            // module lowers it with this name's tree -- see the lowering of
            // omitted arguments.
            declared_in: Some(module_name.to_owned()),
            // A default that the compiler could work out travels as its value:
            // the expression it came from names this module's own tree, and a
            // caller elsewhere has no way to read it.
            default_values: function
                .params
                .iter()
                .map(|parameter| {
                    parameter
                        .default
                        .and_then(|default| default_values.get(&ast.expr(default).span).copied())
                })
                .collect(),
            exported: function.exported,
            is_external: function.body.is_none(),
            abi: function.abi.clone(),
            pack: function.params.iter().position(|param| param.is_pack),
            written_name: function.name.clone(),
            // The name the module bound it under, which for a special method
            // carries the receiver: an importer looking for a constructor asks
            // for `__init__String`, so that is the key it has to be filed
            // under. Using the written name made every exported constructor,
            // destructor, and operator invisible to importers.
            bound_as: definition_name(function),
            template_params: function.template_params.clone(),
            throws: function.throws.is_some(),
            // The condition belongs to the defining module's tree, so an
            // importer cannot evaluate it: it treats the function as always
            // able to raise.
            throws_condition: None,
            // A deprecation is part of what the declaration says, so a use in
            // another module is warned about too. The message is optional;
            // `Some("")` is "deprecated, with nothing to add".
            deprecated: fallc_sema::deprecated_message(ast, &ast.item(item).attrs),
            // A template or a family method has no symbol of its own: every
            // instantiation is a function.
            instantiated: resolved.is_none(),
            resolved,
        };
        exports.add_function(definition_name(function), signature);
    }
    exports
}

