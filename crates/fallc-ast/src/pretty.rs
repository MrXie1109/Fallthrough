//! A stable, diffable dumper for the AST.
//!
//! Used by `fallc --emit=ast` (once that exists) and, more importantly, by
//! parser tests: a parser is much easier to develop against a golden textual
//! dump than against hand-built node comparisons.
//!
//! The format is intentionally boring: two-space indentation, one node per
//! line, `kind span` then children. It is not a serialisation format and makes
//! no stability promise beyond "a given compiler version prints the same tree
//! the same way".

use std::fmt::Write as _;

use crate::node::{
    Ast, BindingType, BlockId, ElseBranch, ExprId, ImportedMembers, Initializer, ItemId, ItemKind,
    LinkLocation, StmtId, StmtKind, Unresolved,
};
use crate::visit::Visitor;

/// Renders `ast` as an indented tree.
#[must_use]
pub fn dump(ast: &Ast) -> String {
    let mut dumper = Dumper {
        out: String::new(),
        indent: 0,
    };
    crate::visit::walk_ast(ast, &mut dumper);
    dumper.out
}

#[derive(Debug)]
struct Dumper {
    out: String,
    indent: usize,
}

impl Dumper {
    fn line(&mut self, text: &str) {
        let _ = writeln!(self.out, "{}{}", "  ".repeat(self.indent), text);
    }

    fn open(&mut self, text: &str) {
        self.line(text);
        self.indent += 1;
    }

    fn close(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    fn optional_block(&mut self, label: &str, block: Option<BlockId>) {
        match block {
            Some(block) => self.line(&format!("{label}: block#{}", block.as_raw())),
            None => self.line(&format!("{label}: (none)")),
        }
    }
}

/// Renders an unresolved fragment as `"text" @lo..hi`.
fn unresolved_text(u: &Unresolved) -> String {
    format!("{:?} @{}..{}", u.text, u.span.lo, u.span.hi)
}

impl Visitor for Dumper {
    fn visit_item(&mut self, _ast: &Ast, id: ItemId, item: &crate::node::Item) {
        match &item.kind {
            ItemKind::Conditional(node) => {
                self.open("Conditional");
                self.line(&format!(
                    "then: {} else: {}",
                    node.then_items.len(),
                    node.else_items.len()
                ));
                self.close();
            }
            ItemKind::EnumDeclaration(node) => {
                self.open(&format!("Enum {}", node.name));
                self.line(&format!("members: {}", node.members.len()));
                self.close();
            }
            ItemKind::Variable(node) => {
                self.open(&format!("Variable {}", node.name));
                self.line(&format!("type: {:?}", node.ty.span()));
                if let Some(init) = &node.init {
                    self.line(&format!("init: {init:?}"));
                }
                self.close();
            }
            ItemKind::ExternVariable(node) => {
                self.open(&format!(
                    "ExternVariable {} from \"{}\"",
                    node.name, node.abi
                ));
                self.line(&format!("type: {:?}", node.ty.span()));
                self.close();
            }
            ItemKind::StaticAssert(node) => {
                self.line(&format!(
                    "StaticAssert {id} expr#{}",
                    node.condition.as_raw()
                ));
                return;
            }
            ItemKind::Interface(node) => {
                self.open(&format!("Interface {}", node.name));
                self.line(&format!("methods: {}", node.methods.len()));
                self.close();
            }
            // The block's statements are the walker's to visit, so this only
            // says which block it is.
            ItemKind::Setup(block) => {
                self.line(&format!("Setup {id} block#{}", block.as_raw()));
                return;
            }
            ItemKind::Const(node) => {
                self.open(&format!("Const {}", node.name));
                self.line(&format!("type: {:?}", node.ty.span()));
                self.line(&format!("value: expr#{}", node.value.as_raw()));
            }
            ItemKind::Import(node) => {
                let alias = node
                    .alias
                    .as_ref()
                    .map_or(String::new(), |alias| format!(" as {}", alias.name));
                let members = match &node.members {
                    None => String::new(),
                    Some(ImportedMembers::All) => " from *".to_owned(),
                    Some(ImportedMembers::Named(names)) => format!(
                        " from {}",
                        names
                            .iter()
                            .map(|name| name.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                self.line(&format!("Import {id} {}{alias}{members}", node.module));
                return;
            }
            ItemKind::Link(node) => {
                let location = match &node.location {
                    LinkLocation::Default => "default".to_owned(),
                    LinkLocation::Directory(path) => format!("{path:?}"),
                };
                self.line(&format!(
                    "Link {id} {} {} from {location}",
                    node.library,
                    node.kind.spelling()
                ));
                return;
            }
            ItemKind::TypeDeclaration(node) => {
                let visibility = if node.exported { "export " } else { "" };
                self.open(&format!("Type {id} {visibility}{}", node.name));
                for field in &node.fields {
                    self.line(&format!("field: {} : {:?}", field.name, field.ty.span()));
                }
                for constant in &node.constants {
                    self.line(&format!(
                        "const: {} = expr#{:?}",
                        constant.name,
                        constant.value.as_raw()
                    ));
                }
                self.close();
                return;
            }
            ItemKind::Function(node) => {
                let abi = node
                    .abi
                    .as_ref()
                    .map_or(String::new(), |abi| format!("extern \"{abi}\" "));
                let visibility = if node.exported { "export " } else { "" };
                let variadic = if node.variadic { ", ..." } else { "" };
                let external = if node.is_external { " (external)" } else { "" };
                self.open(&format!(
                    "Function {id} {abi}{visibility}{}{variadic}{external}",
                    node.name
                ));
                self.line(&format!("name: {:?}", node.name));
                if node.params.is_empty() {
                    self.line("params: (none)");
                } else {
                    for param in &node.params {
                        self.line(&format!("param: {} : {:?}", param.name, param.ty.span()));
                    }
                }
                self.line(&format!("return_type: {:?}", node.return_type.span()));
                match node.body {
                    Some(body) => self.line(&format!("body: block#{}", body.as_raw())),
                    None => self.line("body: (external)"),
                }
            }
            ItemKind::Trait(node) => {
                let visibility = if node.exported { "export " } else { "" };
                self.open(&format!("Trait {id} {visibility}{}", node.name));
                for param in &node.params {
                    self.line(&format!("param: {} : {:?}", param.name, param.ty.span()));
                }
                for requirement in &node.requirements {
                    self.line(&format!("requires: {}", requirement.text));
                }
                self.close();
                return;
            }
            ItemKind::Unresolved(u) => {
                self.open(&format!("Item {} unresolved {}", id, unresolved_text(u)));
            }
        }
        for attr in &item.attrs {
            self.line(&format!(
                "Attr @{} ({} argument(s))",
                attr.name,
                attr.arguments.len()
            ));
        }
    }

    fn visit_item_post(&mut self, _ast: &Ast, _id: ItemId) {
        self.close();
    }

    fn visit_stmt(&mut self, ast: &Ast, id: StmtId) {
        match &ast.stmt(id).kind {
            StmtKind::Expr(node) => {
                self.open(&format!("ExprStmt {id}"));
                self.line(&format!("expr: expr#{}", node.expr.as_raw()));
            }
            StmtKind::StaticAssert(node) => {
                self.open(&format!("StaticAssert {id}"));
                self.line(&format!("condition: expr#{}", node.condition.as_raw()));
                return;
            }
            StmtKind::Bind(node) => {
                self.open(&format!("Bind {id} {}", node.name));
                match &node.ty {
                    BindingType::Written(ty) => {
                        self.line(&format!("type: {:?}", ty.span()));
                    }
                    BindingType::Auto(span) => {
                        self.line(&format!("type: auto @{span:?}"));
                    }
                }
                match &node.init {
                    Some(Initializer::Expr(expr)) => {
                        self.line(&format!("init: expr#{}", expr.as_raw()));
                    }
                    Some(Initializer::Designated(designated)) => {
                        self.line(&format!("init: .{{ {} }}", designated.text));
                    }
                    None => self.line("init: (zero-initialised)"),
                }
            }
            StmtKind::If(node) => {
                self.open(&format!("If {id}"));
                self.line(&format!("condition: expr#{}", node.condition.as_raw()));
                self.line(&format!("then: block#{}", node.then_block.as_raw()));
                match &node.else_branch {
                    Some(ElseBranch::Block(block)) => {
                        self.line(&format!("else: block#{}", block.as_raw()));
                    }
                    Some(ElseBranch::If(stmt)) => {
                        self.line(&format!("else: stmt#{}", stmt.as_raw()));
                    }
                    None => {}
                }
            }
            StmtKind::While(node) => {
                self.open(&format!("While {id}"));
                self.line(&format!("condition: expr#{}", node.condition.as_raw()));
                self.line(&format!("body: block#{}", node.body.as_raw()));
                self.optional_block("else", node.else_block);
            }
            StmtKind::ForC(node) => {
                self.open(&format!("ForC {id}"));
                match node.init {
                    Some(init) => self.line(&format!("init: stmt#{}", init.as_raw())),
                    None => self.line("init: (none)"),
                }
                match node.condition {
                    Some(condition) => {
                        self.line(&format!("condition: expr#{}", condition.as_raw()));
                    }
                    None => self.line("condition: (none, always true)"),
                }
                match node.update {
                    Some(update) => self.line(&format!("update: expr#{}", update.as_raw())),
                    None => self.line("update: (none)"),
                }
                self.line(&format!("body: block#{}", node.body.as_raw()));
                self.optional_block("else", node.else_block);
            }
            StmtKind::Switch(node) => {
                self.open(&format!("Switch {id}"));
                self.line(&format!("scrutinee: expr#{}", node.scrutinee.as_raw()));
                for (index, arm) in node.arms.iter().enumerate() {
                    self.open(&format!("Arm {index}"));
                    match &arm.labels {
                        Some(labels) => {
                            let ids: Vec<String> =
                                labels.iter().map(|l| format!("expr#{l}")).collect();
                            self.line(&format!("labels: {}", ids.join(", ")));
                        }
                        None => self.line("default"),
                    }
                    for &stmt in &arm.body {
                        self.line(&format!("stmt#{stmt}"));
                    }
                    self.close();
                }
            }
            StmtKind::Block(block) => self.line(&format!("Stmt {id} block {block}")),
            StmtKind::Defer(block) => self.line(&format!("Stmt {id} defer {block}")),
            StmtKind::Fallthrough => self.line(&format!("Fallthrough {id}")),
            StmtKind::Break => self.line(&format!("Break {id}")),
            StmtKind::Continue => self.line(&format!("Continue {id}")),
            StmtKind::Return(node) => {
                self.open(&format!("Return {id}"));
                match node.value {
                    Some(value) => self.line(&format!("value: expr#{}", value.as_raw())),
                    None => self.line("value: (none)"),
                }
            }
            StmtKind::Raise(node) => {
                self.open(&format!("Raise {id}"));
                if node.is_reraise() {
                    self.line("re-raise");
                } else {
                    self.line(&format!(
                        "type: expr#{:?}",
                        node.exception_type.map(|e| e.as_raw())
                    ));
                    self.line(&format!(
                        "message: expr#{:?}",
                        node.message.map(|e| e.as_raw())
                    ));
                }
            }
            StmtKind::Try(node) => {
                self.open(&format!("Try {id}"));
                self.line(&format!("body: block#{}", node.body.as_raw()));
                for (index, clause) in node.except_clauses.iter().enumerate() {
                    let names: Vec<String> = clause
                        .names
                        .iter()
                        .map(|name| format!("expr#{}", name.as_raw()))
                        .collect();
                    self.line(&format!(
                        "except#{index}: block#{}, names: [{}]",
                        clause.block.as_raw(),
                        names.join(", ")
                    ));
                }
                self.optional_block("else", node.else_block);
                self.optional_block("finally", node.finally_block);
            }
            StmtKind::Unresolved(u) => {
                self.line(&format!("Stmt {id} unresolved {}", unresolved_text(u)));
                return;
            }
        }
        self.indent += 1;
    }

    fn visit_stmt_post(&mut self, _ast: &Ast, id: StmtId) {
        let _ = id;
        self.close();
    }

    fn visit_block(&mut self, _ast: &Ast, id: BlockId) {
        self.open(&format!("Block {id}"));
        self.indent -= 1;
    }

    fn visit_block_post(&mut self, _ast: &Ast, _id: BlockId) {
        self.close();
    }

    fn visit_expr(&mut self, ast: &Ast, id: ExprId) {
        match &ast.expr(id).kind {
            crate::node::ExprKind::Fold(node) => {
                self.line(&format!(
                    "Expr {id} fold `{}` over `{}`",
                    node.pack.pack,
                    node.op.spelling()
                ));
            }
            crate::node::ExprKind::PackExpand(inner) => {
                self.line(&format!(
                    "Expr {id} pack expansion of expr#{}",
                    inner.as_raw()
                ));
            }
            crate::node::ExprKind::CommaFold { pattern } => {
                self.line(&format!(
                    "Expr {id} comma fold of expr#{}",
                    pattern.as_raw()
                ));
            }
            crate::node::ExprKind::ArrayLiteral(node) => {
                let items: Vec<String> = node
                    .elements
                    .iter()
                    .map(|element| format!("expr#{}", element.as_raw()))
                    .collect();
                self.line(&format!("Expr {id} array-literal [{}]", items.join(", ")));
            }
            crate::node::ExprKind::Range(node) => {
                let op = if node.inclusive { "..=" } else { ".." };
                self.line(&format!(
                    "Expr {id} range `{op}` expr#{} expr#{}",
                    node.low.as_raw(),
                    node.high.as_raw()
                ));
            }
            crate::node::ExprKind::ThrowQuery(inner) => {
                self.line(&format!("Expr {id} throw-query expr#{}", inner.as_raw()));
            }
            crate::node::ExprKind::TypeOf(inner) => {
                self.line(&format!("Expr {id} type-of expr#{}", inner.as_raw()));
            }
            crate::node::ExprKind::TypeLiteral(_) => {
                self.line(&format!("Expr {id} written type"));
            }
            crate::node::ExprKind::Propagate(node) => {
                self.line(&format!("Expr {id} propagate expr#{}", node.value.as_raw()));
            }
            crate::node::ExprKind::IfValue(node) => {
                self.line(&format!(
                    "Expr {id} if-value condition=expr#{} then=block#{}",
                    node.condition.as_raw(),
                    node.then_block.as_raw()
                ));
            }
            crate::node::ExprKind::Builtin(node) => {
                self.line(&format!("Expr {id} @{} ({})", node.name, node.argument));
            }
            crate::node::ExprKind::Block(block) => {
                self.line(&format!("Expr {id} block#{}", block.as_raw()));
            }
            crate::node::ExprKind::Abort(node) => {
                self.line(&format!("Expr {id} abort expr#{}", node.value.as_raw()));
            }
            crate::node::ExprKind::SizeofPack(node) => {
                self.line(&format!("Expr {id} sizeof... `{}`", node.pack));
            }
            crate::node::ExprKind::Yield(node) => match node.value {
                Some(value) => self.line(&format!("Expr {id} yield expr#{}", value.as_raw())),
                None => self.line(&format!("Expr {id} yield")),
            },
            crate::node::ExprKind::IntLiteral(literal) => {
                let suffix = literal.suffix.as_deref().unwrap_or("(int)");
                self.line(&format!("Expr {id} int {} {suffix}", literal.digits));
            }
            crate::node::ExprKind::CharLiteral(literal) => {
                self.line(&format!("Expr {id} char {}", literal.text));
            }
            crate::node::ExprKind::FloatLiteral(literal) => {
                let suffix = literal.suffix.as_deref().unwrap_or("(double)");
                self.line(&format!("Expr {id} float {} {suffix}", literal.text));
            }
            crate::node::ExprKind::StringLiteral(text) => {
                self.line(&format!("Expr {id} string {text}"));
            }
            crate::node::ExprKind::ByteStringLiteral(text) => {
                self.line(&format!("Expr {id} bytestring {text}"));
            }
            crate::node::ExprKind::RawStringLiteral(text) => {
                self.line(&format!("Expr {id} raw string {text}"));
            }
            crate::node::ExprKind::BoolLiteral(value) => {
                self.line(&format!("Expr {id} bool {value}"));
            }
            crate::node::ExprKind::Name(name) => {
                self.line(&format!("Expr {id} name {name}"));
            }
            crate::node::ExprKind::Comma(node) => {
                self.open(&format!("Expr {id} comma"));
                self.line(&format!("lhs: expr#{}", node.lhs.as_raw()));
                self.line(&format!("rhs: expr#{}", node.rhs.as_raw()));
            }
            crate::node::ExprKind::Binary(node) => {
                self.open(&format!("Expr {id} binary `{}`", node.op.spelling()));
                self.line(&format!("lhs: expr#{}", node.lhs.as_raw()));
                self.line(&format!("rhs: expr#{}", node.rhs.as_raw()));
            }
            crate::node::ExprKind::Unary(node) => {
                self.open(&format!("Expr {id} unary `{}`", node.op.spelling()));
                self.line(&format!("operand: expr#{}", node.operand.as_raw()));
            }
            crate::node::ExprKind::IncDec(node) => {
                let position = if node.postfix { "postfix" } else { "prefix" };
                self.open(&format!("Expr {id} {position} `{}`", node.op.spelling()));
                self.line(&format!("operand: expr#{}", node.operand.as_raw()));
            }
            crate::node::ExprKind::Assign(node) => {
                let op = node.op.map_or("=", |op| op.spelling());
                self.open(&format!("Expr {id} assign `{op}`"));
                self.line(&format!("target: expr#{}", node.target.as_raw()));
                self.line(&format!("value: expr#{}", node.value.as_raw()));
            }
            crate::node::ExprKind::TypedInit(node) => {
                self.open(&format!(
                    "Expr {id} typed init {}{{ {} }}",
                    node.type_name, node.init.text
                ));
            }
            crate::node::ExprKind::NestedInit(init) => {
                self.open(&format!("Expr {id} nested init .{{ {} }}", init.text));
            }
            crate::node::ExprKind::Field(node) => {
                let arrow = if node.through_pointer { "->" } else { "." };
                self.open(&format!("Expr {id} field `{arrow}{}`", node.field));
                self.line(&format!("object: expr#{}", node.object.as_raw()));
            }
            crate::node::ExprKind::NullPointer => {
                self.line(&format!("Expr {id} nullptr"));
            }
            crate::node::ExprKind::Index(node) => {
                self.open(&format!("Expr {id} index"));
                self.line(&format!("object: expr#{}", node.object.as_raw()));
                self.line(&format!("index: expr#{}", node.index.as_raw()));
            }
            crate::node::ExprKind::Call(node) => {
                self.open(&format!("Expr {id} call"));
                self.line(&format!("callee: expr#{}", node.callee.as_raw()));
                for &arg in &node.args {
                    self.line(&format!("arg: expr#{arg}"));
                }
                for argument in &node.named {
                    self.line(&format!("named {}: expr#{}", argument.name, argument.value));
                }
            }
            crate::node::ExprKind::Lambda(node) => {
                let names: Vec<&str> = node
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str())
                    .collect();
                self.open(&format!("Expr {id} lambda ({})", names.join(", ")));
                match &node.body {
                    crate::node::LambdaBody::Expression(value) => {
                        self.line(&format!("body: expr#{value}"));
                    }
                    crate::node::LambdaBody::Block(block) => {
                        self.line(&format!("body: block#{block}"));
                    }
                }
            }
            crate::node::ExprKind::Cast(node) => {
                self.open(&format!("Expr {id} cast"));
                self.line(&format!("to: {:?}", node.ty.span()));
                self.line(&format!("operand: expr#{}", node.operand.as_raw()));
            }
            crate::node::ExprKind::Conditional(node) => {
                self.open(&format!("Expr {id} conditional"));
                self.line(&format!("condition: expr#{}", node.condition.as_raw()));
                self.line(&format!("then: expr#{}", node.then_value.as_raw()));
                self.line(&format!("else: expr#{}", node.else_value.as_raw()));
            }
            crate::node::ExprKind::Sizeof(ty) => {
                self.open(&format!("Expr {id} sizeof"));
                self.line(&format!("type: {:?}", ty.span()));
            }
            crate::node::ExprKind::Alignof(ty) => {
                self.open(&format!("Expr {id} alignof"));
                self.line(&format!("type: {:?}", ty.span()));
            }
            crate::node::ExprKind::Unresolved(u) => {
                self.open(&format!("Expr {} unresolved {}", id, unresolved_text(u)));
            }
        }
    }

    fn visit_expr_post(&mut self, ast: &Ast, id: ExprId) {
        // Leaf expressions do not open an indentation level, so only pop for
        // the ones that did.
        if matches!(
            ast.expr(id).kind,
            crate::node::ExprKind::Unresolved(_)
                | crate::node::ExprKind::Cast(_)
                | crate::node::ExprKind::Conditional(_)
                | crate::node::ExprKind::Sizeof(_)
                | crate::node::ExprKind::Alignof(_)
                | crate::node::ExprKind::Binary(_)
                | crate::node::ExprKind::Unary(_)
                | crate::node::ExprKind::Assign(_)
                | crate::node::ExprKind::Call(_)
                | crate::node::ExprKind::IncDec(_)
                | crate::node::ExprKind::Comma(_)
                | crate::node::ExprKind::Field(_)
                | crate::node::ExprKind::NestedInit(_)
                | crate::node::ExprKind::TypedInit(_)
        ) {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{ItemKind, Unresolved};
    use fallc_span::{FileId, Span};

    #[test]
    fn dump_of_empty_ast_is_empty() {
        assert_eq!(dump(&Ast::new()), "");
    }

    #[test]
    fn dump_shows_item_indentation() {
        let mut ast = Ast::new();
        let span = Span::new(FileId::from_raw(0), 0, 5);
        let id = ast
            .builder()
            .alloc_item(ItemKind::Unresolved(Unresolved::new("hello", span)), span);
        ast.builder().push_item(id);

        let text = dump(&ast);
        assert!(
            text.starts_with("Item item#0 unresolved \"hello\" @0..5"),
            "{text}"
        );
    }
}
