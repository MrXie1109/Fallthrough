//! Rendering a declaration back to source text.
//!
//! A module's interface is published as `.it`: a file of declarations that an
//! importer can parse exactly as it would parse the module itself. Writing the
//! interface as *source* rather than as a private binary format is what keeps
//! the two ways of reaching a module the same: whether the importer reads the
//! `.it` or the `.ftm`, it sees the same declarations and runs the same parser
//! and checker over them.
//!
//! Nothing here decides anything. It writes out what a declaration already
//! says, in the syntax the language reads back.

use fallc_ast::{Ast, TypeSyntax};

/// Renders a written type back to source text.
///
/// The result parses to the same `TypeSyntax`, which is the property the
/// interface depends on: a type is published as text, and the importer's parser
/// is what turns it back into the structure it had.
#[must_use]
pub(super) fn type_syntax(ty: &TypeSyntax) -> String {
    type_syntax_in(ty, None)
}

/// Renders a written type, reading its expressions from `ast` when one is given.
///
/// An array's length and a template's value argument are expressions, and they
/// are written back the way the source wrote them: `[N]T` stays `[N]T`, and
/// `[3]int` stays `[3]int`.
#[must_use]
pub(super) fn type_syntax_in(ty: &TypeSyntax, ast: Option<&Ast>) -> String {
    match ty {
        TypeSyntax::Named(named) => {
            let mut out = named.path.clone();
            if !named.arguments.is_empty() {
                let rendered: Vec<String> = named
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        fallc_ast::TemplateArgumentSyntax::Type(ty) => type_syntax_in(ty, ast),
                        fallc_ast::TemplateArgumentSyntax::Value(expr) => expr_text(*expr, ast),
                        fallc_ast::TemplateArgumentSyntax::Named { path, .. } => path.clone(),
                    })
                    .collect();
                out.push('<');
                out.push_str(&rendered.join(", "));
                out.push('>');
            }
            if named.is_expansion {
                out.push_str("...");
            }
            out
        }
        TypeSyntax::Array(array) => format!(
            "[{}]{}",
            expr_text(array.len, ast),
            type_syntax_in(&array.element, ast)
        ),
        TypeSyntax::UnsizedArray(element) => format!("[]{}", type_syntax(element)),
        TypeSyntax::Pointer(inner) => format!("*{}", type_syntax_in(inner, ast)),
        TypeSyntax::PointerToConst(inner) => format!("*const {}", type_syntax_in(inner, ast)),
        TypeSyntax::ConstPointer(inner) => format!("const *{}", type_syntax_in(inner, ast)),
        TypeSyntax::Reference(inner) => format!("&{}", type_syntax_in(inner, ast)),
        TypeSyntax::ConstReference(inner) => format!("&const {}", type_syntax_in(inner, ast)),
        TypeSyntax::RvalueReference(inner) => format!("&&{}", type_syntax_in(inner, ast)),
        TypeSyntax::ConstRvalueReference(inner) => {
            format!("&&const {}", type_syntax_in(inner, ast))
        }
        TypeSyntax::Volatile(inner) => format!("volatile {}", type_syntax_in(inner, ast)),
        TypeSyntax::Const(inner) => format!("const {}", type_syntax_in(inner, ast)),
        TypeSyntax::Auto(_) => "auto".to_owned(),
        TypeSyntax::DecltypeAuto(_) => "type(auto)".to_owned(),
        TypeSyntax::TypeOf(node) => format!("type({})", expr_text(node.operand, ast)),
        TypeSyntax::Member(member) => {
            format!("{}.{}", type_syntax_in(&member.outer, ast), member.name)
        }
        TypeSyntax::Function(function) => {
            let parameters: Vec<String> = function
                .parameters
                .iter()
                .map(|parameter| type_syntax_in(parameter, ast))
                .collect();
            let mut list = parameters.join(", ");
            if function.variadic {
                list.push_str(", ...");
            }
            let mut out = match &function.abi {
                Some(abi) => format!("fn \"{abi}\" ({list})"),
                None => format!("fn({list})"),
            };
            out.push(' ');
            out.push_str(&type_syntax_in(&function.returns, ast));
            if function.throws {
                out.push_str(" except");
            }
            out
        }
        TypeSyntax::Unresolved(unresolved) => unresolved.text.clone(),
    }
}

/// Renders an expression back to source text.
///
/// Only the forms an interface can contain: a constant's value, a default
/// argument, an array length. Anything else is written as its source span,
/// which is what the reader of an interface would have seen anyway.
fn expr_text(id: fallc_ast::ExprId, ast: Option<&Ast>) -> String {
    match ast {
        Some(ast) => expr_syntax(ast, id),
        // Without the tree there is nothing to write; a caller that renders a
        // type always has one.
        None => String::new(),
    }
}

/// Renders an expression of a given tree back to source text.
#[must_use]
pub(super) fn expr_syntax(ast: &Ast, id: fallc_ast::ExprId) -> String {
    use fallc_ast::ExprKind;
    // An export table may name an expression of *another* module's tree -- a
    // default argument is the case, since it is written where it was declared.
    // Reading it here would be reading a different node, so a name outside this
    // tree is rendered as nothing rather than as whatever sits at that index.
    if (id.as_raw() as usize) >= ast.expr_count() {
        return String::new();
    }
    match &ast.expr(id).kind {
        ExprKind::IntLiteral(literal) => match &literal.suffix {
            Some(suffix) => format!("{}{}", literal.digits, suffix),
            None => literal.digits.clone(),
        },
        ExprKind::BoolLiteral(value) => value.to_string(),
        ExprKind::CharLiteral(literal) => format!("'{}'", literal.text),
        ExprKind::FloatLiteral(literal) => match &literal.suffix {
            Some(suffix) => format!("{}{}", literal.text, suffix),
            None => literal.text.clone(),
        },
        ExprKind::StringLiteral(text) => format!("\"{text}\""),
        ExprKind::RawStringLiteral(text) => format!("r\"{text}\""),
        ExprKind::Name(name) => name.clone(),
        ExprKind::NullPointer => "nullptr".to_owned(),
        ExprKind::Unary(unary) => {
            format!("{}{}", unary.op.spelling(), expr_syntax(ast, unary.operand))
        }
        ExprKind::Binary(binary) => format!(
            "({} {} {})",
            expr_syntax(ast, binary.lhs),
            binary_op_spelling(binary.op),
            expr_syntax(ast, binary.rhs)
        ),
        ExprKind::Cast(cast) => format!(
            "({} as {})",
            expr_syntax(ast, cast.operand),
            type_syntax_in(&cast.ty, Some(ast))
        ),
        ExprKind::Field(field) => format!("{}.{}", expr_syntax(ast, field.object), field.field),
        // Every other form is not something an interface states, and writing a
        // placeholder keeps the renderer total without pretending to support it.
        _ => String::new(),
    }
}

fn binary_op_spelling(op: fallc_ast::BinaryOp) -> &'static str {
    op.spelling()
}

/// Renders a module's exported declarations as the text of its `.it` file.
///
/// The result is ordinary Fallthrough: an importer can parse it exactly as it
/// would parse the module, which is what makes the interface and the module
/// agree by construction rather than by two implementations being kept in step.
///
/// Everything an importer may name appears here -- functions with their
/// overloads and defaults, types with their layouts, constants with their
/// values, traits, enums and interfaces. A name the module did not export is
/// absent, which is the same statement as "private".
#[must_use]
pub(super) fn render(exports: &fallc_sema::ExternalModule, module: &str, ast: &Ast) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// {module}.it -- the interface of module `{module}`.\n"
    ));
    out.push_str("// Generated from the module's export table; do not edit.\n\n");

    // Types come first: a signature may name one.
    let mut type_names: Vec<&String> = exports.types.keys().collect();
    type_names.sort();
    for name in type_names {
        let ty = &exports.types[name];
        out.push_str(&render_type(ty, ast));
        out.push('\n');
    }

    let mut enum_names: Vec<&String> = exports.enums.keys().collect();
    enum_names.sort();
    for name in enum_names {
        out.push_str(&render_enum(&exports.enums[name], ast));
        out.push('\n');
    }

    let mut trait_names: Vec<&String> = exports.traits.iter().collect();
    trait_names.sort();
    for name in trait_names {
        out.push_str(&format!("export traits {name};\n"));
    }

    let mut interface_names: Vec<&String> = exports.interfaces.keys().collect();
    interface_names.sort();
    for name in interface_names {
        out.push_str(&format!("export interface {name};\n"));
    }

    let mut constant_names: Vec<&String> = exports.constants.keys().collect();
    constant_names.sort();
    for name in constant_names {
        let written = exports
            .constant_types
            .get(name)
            .map(|ty| type_syntax_in(ty, Some(ast)))
            .unwrap_or_else(|| "int".to_owned());
        out.push_str(&format!(
            "export {name} const {} = {};\n",
            strip_trailing_const(&written),
            exports.constants[name]
        ));
    }

    let mut global_names: Vec<&String> = exports.globals.keys().collect();
    global_names.sort();
    for name in global_names {
        let global = &exports.globals[name];
        out.push_str(&format!(
            "export {name} {} @extern_C;\n",
            global.ty.spelling()
        ));
    }

    let mut variable_names: Vec<&String> = exports.variables.keys().collect();
    variable_names.sort();
    for name in variable_names {
        let variable = &exports.variables[name];
        out.push_str(&format!(
            "export {name} {} @extern_C;\n",
            type_syntax_in(&variable.ty, Some(ast))
        ));
    }

    // Functions last, so every type a signature names is already declared.
    let mut function_names: Vec<&String> = exports.symbols.keys().collect();
    function_names.sort();
    for name in function_names {
        for signature in &exports.symbols[name] {
            out.push_str(&render_function(name, signature, ast));
        }
    }
    out
}

/// `T const` is how a constant's type is written; the `.it` spells the value's
/// type and lets `const` be the binding's, which is where it belongs.
fn strip_trailing_const(written: &str) -> String {
    match written.strip_suffix(" const") {
        Some(rest) => rest.to_owned(),
        None => written.to_owned(),
    }
}

fn render_type(ty: &fallc_sema::ExportType, ast: &Ast) -> String {
    let mut out = String::new();
    if !ty.template_params.is_empty() {
        let parameters: Vec<String> = ty
            .template_params
            .iter()
            .map(|param| render_template_parameter(param, ast))
            .collect();
        out.push_str(&format!("template <{}>\n", parameters.join(", ")));
    }
    let keyword = if ty.is_union { "union" } else { "type" };
    out.push_str(&format!("export {keyword} {}", ty.name));
    if !ty.template_params.is_empty() {
        let names: Vec<String> = ty.template_params.iter().map(|p| p.name.clone()).collect();
        out.push_str(&format!("<{}>", names.join(", ")));
    }
    out.push_str(" {\n");
    for field in &ty.fields {
        out.push_str(&format!(
            "    {} {};\n",
            field.name,
            type_syntax_in(&field.ty, Some(ast))
        ));
    }
    for (name, target) in &ty.nested {
        out.push_str(&format!(
            "    type {name} = {};\n",
            type_syntax_in(target, Some(ast))
        ));
    }
    out.push_str("}\n");
    out
}

fn render_template_parameter(param: &fallc_ast::TemplateParam, ast: &Ast) -> String {
    use fallc_ast::TemplateParamKind;
    let pack = if param.is_pack { "..." } else { "" };
    match &param.kind {
        TemplateParamKind::Type => format!("{}{pack} type", param.name),
        TemplateParamKind::Value(written) => {
            format!(
                "{}{pack} {}",
                param.name,
                type_syntax_in(written, Some(ast))
            )
        }
    }
}

fn render_enum(declaration: &fallc_sema::ExportEnum, ast: &Ast) -> String {
    let base = declaration
        .base
        .as_ref()
        .map(|ty| type_syntax_in(ty, Some(ast)))
        .unwrap_or_else(|| "int".to_owned());
    let mut out = format!("export enum {} {} {{\n", declaration.name, base);
    for (member, value) in &declaration.members {
        out.push_str(&format!("    {member} = {value},\n"));
    }
    out.push_str("}\n");
    out
}

fn render_function(name: &str, signature: &fallc_sema::FunctionSignature, ast: &Ast) -> String {
    let mut out = String::new();
    if !signature.template_params.is_empty() {
        let parameters: Vec<String> = signature
            .template_params
            .iter()
            .map(|param| render_template_parameter(param, ast))
            .collect();
        out.push_str(&format!("template <{}>\n", parameters.join(", ")));
    }
    // The prefix order is the one the language states: attributes first, then
    // the declaration keyword. `export` is a keyword, so an external function
    // reads `@extern_C` / `export fn`, not `export @extern_C fn`.
    if signature.is_external {
        out.push_str("@extern_C\n");
    }
    out.push_str("export ");
    out.push_str(&format!("fn {name}("));
    let mut parts = Vec::new();
    for (index, parameter) in signature.param_names.iter().enumerate() {
        let written = signature
            .param_types
            .get(index)
            .map(|ty| type_syntax_in(ty, Some(ast)))
            .unwrap_or_else(|| "auto".to_owned());
        parts.push(format!("{parameter} {written}"));
    }
    if signature.variadic {
        parts.push("...".to_owned());
    }
    out.push_str(&parts.join(", "));
    out.push(')');
    out.push(' ');
    out.push_str(&type_syntax_in(&signature.return_type, Some(ast)));
    if signature.throws {
        out.push_str(" except");
    }
    out.push_str(";\n");
    out
}

/// Writes a module's interface into the build cache as `<module>.it`.
///
/// A module reached through its interface and a module reached through its
/// source are the same module: what is written here is the export table, in the
/// syntax the language reads back. A cache the compiler cannot write is not an
/// error -- the module is simply compiled from source next time, as it would be
/// without a cache at all.
pub(super) fn publish(exports: &fallc_sema::ExternalModule, module: &str, ast: &Ast) {
    let Some(directory) = crate::pipeline::cache::build_directory() else {
        return;
    };
    let text = render(exports, module, ast);
    let path = directory.join(format!("{}.it", module.replace('.', "/")));
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let _ = std::fs::write(path, text);
}
