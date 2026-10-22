//! Traversal infrastructure for the AST.
//!
//! # Why both a visitor and a walker
//!
//! Passes fall into two groups. *Consuming* passes (type checking, lowering)
//! need to know the shape of every node and are naturally written with `match`
//! over the node enums. *Generic* passes (pretty-printing, id collection,
//! parent maps, span validation) only care about "walk everything reachable"
//! and should not need updating every time a node variant is added.
//!
//! [`Visitor`] has a default no-op method per node category, so a pass
//! overrides exactly what it cares about. [`walk_ast`] drives it. Every variant
//! in [`crate::node`] has its children reached from [`walk_ast`], and the
//! matches are exhaustive, so adding a node variant and forgetting its arm is a
//! compile error rather than a silently skipped subtree.

use crate::node::{
    Ast, BlockId, ElseBranch, ExprId, ExprKind, Initializer, Item, ItemId, ItemKind, NodeId,
    StmtId, StmtKind,
};

/// A read-only traversal of the AST.
///
/// All methods default to doing nothing. Override the ones you need and call
/// [`walk_ast`] to drive the traversal.
pub trait Visitor {
    /// Called before an item's children are visited.
    fn visit_item(&mut self, _ast: &Ast, _id: ItemId, _item: &Item) {}

    /// Called before a statement's children are visited.
    fn visit_stmt(&mut self, _ast: &Ast, _id: StmtId) {}

    /// Called before an expression's children are visited.
    fn visit_expr(&mut self, _ast: &Ast, _id: ExprId) {}

    /// Called before a block's statements are visited.
    fn visit_block(&mut self, _ast: &Ast, _id: BlockId) {}

    /// Called after an item's children have been visited.
    fn visit_item_post(&mut self, _ast: &Ast, _id: ItemId) {}

    /// Called after a statement's children have been visited.
    fn visit_stmt_post(&mut self, _ast: &Ast, _id: StmtId) {}

    /// Called after an expression's children have been visited.
    fn visit_expr_post(&mut self, _ast: &Ast, _id: ExprId) {}

    /// Called after a block's statements have been visited.
    fn visit_block_post(&mut self, _ast: &Ast, _id: BlockId) {}
}

/// A traversal that may rewrite the AST in place.
///
/// Rewriting visitors return an optional replacement id: `None` means "leave
/// this node alone", `Some(new_id)` means "use this node instead". In-place
/// mutation of a node's contents (rather than replacing its id) is also
/// allowed via [`Ast::item_mut`] and friends; that is the cheaper option and
/// is what most passes should use.
pub trait VisitorMut {
    /// Visits an item, optionally replacing its id.
    fn visit_item(&mut self, _ast: &mut Ast, id: ItemId) -> Option<ItemId> {
        Some(id)
    }

    /// Visits a statement, optionally replacing its id.
    fn visit_stmt(&mut self, _ast: &mut Ast, id: StmtId) -> Option<StmtId> {
        Some(id)
    }

    /// Visits an expression, optionally replacing its id.
    fn visit_expr(&mut self, _ast: &mut Ast, id: ExprId) -> Option<ExprId> {
        Some(id)
    }
}

/// Walks the whole tree depth-first, driving `visitor`.
pub fn walk_ast<V: Visitor + ?Sized>(ast: &Ast, visitor: &mut V) {
    for &item in &ast.items {
        walk_item(ast, item, visitor);
    }
}

/// Walks one item and its children.
pub fn walk_item<V: Visitor + ?Sized>(ast: &Ast, id: ItemId, visitor: &mut V) {
    let item = ast.item(id);
    visitor.visit_item(ast, id, item);
    walk_item_kind(ast, &item.kind, visitor);
    visitor.visit_item_post(ast, id);
}

/// Walks the children of an item kind.
pub fn walk_item_kind<V: Visitor + ?Sized>(ast: &Ast, kind: &ItemKind, visitor: &mut V) {
    match kind {
        ItemKind::Variable(node) => match &node.init {
            Some(crate::node::Initializer::Expr(expr)) => walk_expr(ast, *expr, visitor),
            Some(crate::node::Initializer::Designated(init)) => {
                for &element in &init.elements {
                    walk_expr(ast, element, visitor);
                }
            }
            None => {}
        },
        // An external variable's type is a type, not an expression: there is
        // nothing here for an expression walker to reach.
        ItemKind::ExternVariable(_) => {}
        ItemKind::Const(node) => walk_expr(ast, node.value, visitor),
        // The condition of an assertion is an expression like any other.
        ItemKind::StaticAssert(node) => walk_expr(ast, node.condition, visitor),
        // An interface declares signatures: their types are types, and there is
        // nothing in them for this walker to descend into.
        ItemKind::Interface(_) => {}
        // A `__setup__` block is a body: its statements are part of the tree.
        ItemKind::Setup(block) => walk_block(ast, *block, visitor),
        // An enumerator's value is an expression like any other, so it is
        // walked; a type declaration's fields are types, which this walker does
        // not descend into.
        ItemKind::EnumDeclaration(node) => {
            for member in &node.members {
                if let Some(value) = member.value {
                    walk_expr(ast, value, visitor);
                }
            }
        }
        ItemKind::Function(node) => {
            // An external declaration has no body to walk.
            if let Some(body) = node.body {
                walk_block(ast, body, visitor);
            }
        }
        // An import names a module; it has no children.
        ItemKind::Import(_) => {}
        // A link instruction names a library, not an expression, so there is
        // nothing for the expression walker to reach.
        ItemKind::Link(_) => {}
        // A type declaration's fields carry types, not expressions, but its
        // constant members carry initialisers, and a visitor that inspects
        // expressions has to reach them.
        ItemKind::TypeDeclaration(node) => {
            for constant in &node.constants {
                walk_expr(ast, constant.value, visitor);
            }
        }
        // A trait's requirements *are* expressions: they are the operations
        // being required, and a visitor that folds or inspects expressions has
        // to reach them.
        ItemKind::Trait(node) => {
            for requirement in &node.requirements {
                walk_expr(ast, requirement.expr, visitor);
            }
        }
        // An unresolved item is raw text, so it has no children.
        ItemKind::Unresolved(_) => {}
    }
}

/// Walks one statement and its children.
pub fn walk_stmt<V: Visitor + ?Sized>(ast: &Ast, id: StmtId, visitor: &mut V) {
    let stmt = ast.stmt(id);
    visitor.visit_stmt(ast, id);
    walk_stmt_kind(ast, &stmt.kind, visitor);
    visitor.visit_stmt_post(ast, id);
}

/// Walks the children of a statement kind.
pub fn walk_stmt_kind<V: Visitor + ?Sized>(ast: &Ast, kind: &StmtKind, visitor: &mut V) {
    match kind {
        StmtKind::Expr(node) => walk_expr(ast, node.expr, visitor),
        StmtKind::StaticAssert(node) => walk_expr(ast, node.condition, visitor),
        StmtKind::Bind(node) => {
            // The written type is not an expression, so it has nothing to
            // walk; only the initialiser can contain expressions.
            match &node.init {
                Some(Initializer::Expr(expr)) => walk_expr(ast, *expr, visitor),
                Some(Initializer::Designated(designated)) => {
                    for &element in &designated.elements {
                        walk_expr(ast, element, visitor);
                    }
                }
                None => {}
            }
        }
        StmtKind::If(node) => {
            walk_expr(ast, node.condition, visitor);
            walk_block(ast, node.then_block, visitor);
            match &node.else_branch {
                Some(ElseBranch::Block(block)) => walk_block(ast, *block, visitor),
                Some(ElseBranch::If(stmt)) => walk_stmt(ast, *stmt, visitor),
                None => {}
            }
        }
        StmtKind::While(node) => {
            walk_expr(ast, node.condition, visitor);
            walk_block(ast, node.body, visitor);
            if let Some(block) = node.else_block {
                walk_block(ast, block, visitor);
            }
        }
        StmtKind::ForC(node) => {
            if let Some(init) = node.init {
                walk_stmt(ast, init, visitor);
            }
            if let Some(condition) = node.condition {
                walk_expr(ast, condition, visitor);
            }
            if let Some(update) = node.update {
                walk_expr(ast, update, visitor);
            }
            walk_block(ast, node.body, visitor);
            if let Some(block) = node.else_block {
                walk_block(ast, block, visitor);
            }
        }
        StmtKind::Switch(node) => {
            walk_expr(ast, node.scrutinee, visitor);
            for arm in &node.arms {
                if let Some(labels) = &arm.labels {
                    for &label in labels {
                        walk_expr(ast, label, visitor);
                    }
                }
                for &stmt in &arm.body {
                    walk_stmt(ast, stmt, visitor);
                }
            }
        }
        StmtKind::Return(node) => {
            if let Some(value) = node.value {
                walk_expr(ast, value, visitor);
            }
        }
        StmtKind::Raise(node) => {
            if let Some(ty) = node.exception_type {
                walk_expr(ast, ty, visitor);
            }
            if let Some(message) = node.message {
                walk_expr(ast, message, visitor);
            }
        }
        StmtKind::Try(node) => {
            walk_block(ast, node.body, visitor);
            for clause in &node.except_clauses {
                for &name in &clause.names {
                    walk_expr(ast, name, visitor);
                }
                walk_block(ast, clause.block, visitor);
            }
            for block in [node.else_block, node.finally_block]
                .into_iter()
                .flatten()
            {
                walk_block(ast, block, visitor);
            }
        }
        StmtKind::Block(block) => walk_block(ast, *block, visitor),
        StmtKind::Defer(block) => walk_block(ast, *block, visitor),
        StmtKind::Fallthrough | StmtKind::Break | StmtKind::Continue => {}
        // An unresolved statement is raw text, so it has no children.
        StmtKind::Unresolved(_) => {}
    }
}

/// Walks one block and its statements.
pub fn walk_block<V: Visitor + ?Sized>(ast: &Ast, id: BlockId, visitor: &mut V) {
    visitor.visit_block(ast, id);
    for &stmt in &ast.block(id).statements {
        walk_stmt(ast, stmt, visitor);
    }
    visitor.visit_block_post(ast, id);
}

/// Walks one expression and its children.
pub fn walk_expr<V: Visitor + ?Sized>(ast: &Ast, id: ExprId, visitor: &mut V) {
    let expr = ast.expr(id);
    visitor.visit_expr(ast, id);
    walk_expr_kind(ast, &expr.kind, visitor);
    visitor.visit_expr_post(ast, id);
}

/// Walks the children of an expression kind.
pub fn walk_expr_kind<V: Visitor + ?Sized>(ast: &Ast, kind: &ExprKind, visitor: &mut V) {
    match kind {
        ExprKind::Comma(node) => {
            walk_expr(ast, node.lhs, visitor);
            walk_expr(ast, node.rhs, visitor);
        }
        ExprKind::Binary(node) => {
            walk_expr(ast, node.lhs, visitor);
            walk_expr(ast, node.rhs, visitor);
        }
        ExprKind::Unary(node) => walk_expr(ast, node.operand, visitor),
        // A fold names a pack rather than an expression, so there is nothing
        // inside it for this walker to descend into.
        ExprKind::Fold(_) => {}
        // A pack expansion and a comma fold both name a pack *and* contain an
        // expression the elements are put into, so the pattern is walked like
        // any other expression.
        ExprKind::PackExpand(inner) => walk_expr(ast, *inner, visitor),
        ExprKind::CommaFold { pattern } => walk_expr(ast, *pattern, visitor),
        ExprKind::Propagate(node) => walk_expr(ast, node.value, visitor),
        ExprKind::ThrowQuery(inner) => walk_expr(ast, *inner, visitor),
        // `type(expr)` names a type, but its operand is an expression and has
        // names to resolve and expressions to fold.
        ExprKind::TypeOf(inner) => walk_expr(ast, *inner, visitor),
        // A written type holds no expressions; its names are types, which the
        // checker resolves where the type is used.
        ExprKind::TypeLiteral(_) => {}
        ExprKind::SizeofPack(_) => {}
        ExprKind::Conditional(node) => {
            walk_expr(ast, node.condition, visitor);
            walk_expr(ast, node.then_value, visitor);
            walk_expr(ast, node.else_value, visitor);
        }
        // `sizeof(T)` and `alignof(T)` name a type rather than containing an
        // expression, so there is nothing to walk: their operand is not part of
        // the expression tree.
        ExprKind::Sizeof(_) | ExprKind::Alignof(_) => {}
        ExprKind::IncDec(node) => walk_expr(ast, node.operand, visitor),
        ExprKind::Assign(node) => {
            walk_expr(ast, node.target, visitor);
            walk_expr(ast, node.value, visitor);
        }
        ExprKind::TypedInit(node) => {
            for &element in &node.init.elements {
                walk_expr(ast, element, visitor);
            }
            for argument in &node.init.named {
                walk_expr(ast, argument.value, visitor);
            }
        }
        ExprKind::NestedInit(init) => {
            for &element in &init.elements {
                walk_expr(ast, element, visitor);
            }
            for argument in &init.named {
                walk_expr(ast, argument.value, visitor);
            }
        }
        ExprKind::Field(node) => walk_expr(ast, node.object, visitor),
        // A null pointer names nothing and contains nothing.
        ExprKind::NullPointer => {}
        ExprKind::Index(node) => {
            walk_expr(ast, node.object, visitor);
            walk_expr(ast, node.index, visitor);
        }
        ExprKind::Call(node) => {
            walk_expr(ast, node.callee, visitor);
            for &arg in &node.args {
                walk_expr(ast, arg, visitor);
            }
            // A named argument is an argument: its value is an expression the
            // walker must reach, or a variable used only there looks unused.
            for argument in &node.named {
                walk_expr(ast, argument.value, visitor);
            }
        }
        ExprKind::Lambda(node) => {
            // A lambda's body belongs to its own scope, but every expression in
            // it is still part of this tree.
            match &node.body {
                crate::node::LambdaBody::Expression(value) => walk_expr(ast, *value, visitor),
                crate::node::LambdaBody::Block(block) => walk_block(ast, *block, visitor),
            }
        }
        ExprKind::Cast(node) => walk_expr(ast, node.operand, visitor),
        // Leaf expressions have no children to walk.
        ExprKind::IntLiteral(_)
        | ExprKind::CharLiteral(_)
        | ExprKind::FloatLiteral(_)
        | ExprKind::StringLiteral(_)
        | ExprKind::ByteStringLiteral(_)
        | ExprKind::RawStringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Name(_) => {}
        // An unresolved expression is raw text, so it has no children.
        ExprKind::Unresolved(_) => {}
    }
    let _ = (ast, visitor);
}

/// Collects the ids of every expression in the tree, in visitation order.
///
/// A convenience over [`Visitor`] that also serves as a worked example of how
/// to write a visitor.
#[derive(Debug, Default)]
pub struct CollectExprs {
    /// The ids collected so far, in the order they were visited.
    pub ids: Vec<ExprId>,
}

impl Visitor for CollectExprs {
    fn visit_expr(&mut self, _ast: &Ast, id: ExprId) {
        self.ids.push(id);
    }
}

/// Returns every expression id in `ast`, in depth-first order.
#[must_use]
pub fn collect_expr_ids(ast: &Ast) -> Vec<ExprId> {
    let mut collector = CollectExprs::default();
    walk_ast(ast, &mut collector);
    collector.ids
}

/// Resolves a node id to a human-readable description, for debugging.
#[must_use]
pub fn describe_node(ast: &Ast, id: NodeId) -> String {
    if id.is_dummy() {
        return "#dummy".to_owned();
    }
    let raw = id.as_raw();
    if (raw as usize) < ast.item_count() && ast.item(ItemId::from_raw(raw)).node == id {
        return format!("item#{}", raw);
    }
    if (raw as usize) < ast.stmt_count() && ast.stmt(StmtId::from_raw(raw)).node == id {
        return format!("stmt#{}", raw);
    }
    if (raw as usize) < ast.expr_count() && ast.expr(ExprId::from_raw(raw)).node == id {
        return format!("expr#{}", raw);
    }
    format!("unknown#{}", raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{
        BlockId, ElseBranch, IfStmt, ReturnStmt, SwitchArm, SwitchStmt, TryStmt, Unresolved,
    };
    use fallc_span::{FileId, Span};

    #[test]
    fn walk_visits_every_item() {
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        for i in 0..3u32 {
            let span = Span::new(FileId::from_raw(0), i, i + 1);
            let id = builder.alloc_item(ItemKind::Unresolved(Unresolved::new("x", span)), span);
            builder.push_item(id);
        }
        // `builder` borrows `ast` mutably; ending its scope here releases it.

        let mut count = 0;
        struct Counter(usize);
        impl Visitor for Counter {
            fn visit_item(&mut self, _ast: &Ast, _id: ItemId, _item: &Item) {
                self.0 += 1;
            }
        }
        let mut counter = Counter(0);
        walk_ast(&ast, &mut counter);
        count += counter.0;
        assert_eq!(count, 3);
    }

    #[test]
    fn collect_exprs_sees_allocated_exprs() {
        let mut ast = Ast::new();
        let span = Span::new(FileId::from_raw(0), 0, 1);
        let e = ast
            .builder()
            .alloc_expr(ExprKind::Unresolved(Unresolved::new("1", span)), span);
        // Not reachable from any item yet, so the walk finds nothing.
        assert!(collect_expr_ids(&ast).is_empty());
        assert_eq!(describe_node(&ast, e.as_node()), "expr#0");
    }

    #[test]
    fn walk_reaches_an_if_condition_and_body() {
        let span = Span::new(FileId::from_raw(0), 0, 1);
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        let condition = builder.alloc_expr(ExprKind::Unresolved(Unresolved::new("c", span)), span);
        let inner = builder.alloc_stmt(StmtKind::Break, span);
        let then_block = builder.alloc_block(vec![inner], span);
        let if_stmt = builder.alloc_stmt(
            StmtKind::If(IfStmt {
                is_constexpr: false,
                condition,
                condition_attrs: Vec::new(),
                then_block,
                else_branch: None,
                else_attrs: Vec::new(),
            }),
            span,
        );
        // `builder` borrows `ast` mutably; ending its scope here releases it.

        let mut conditions = 0;
        let mut breaks = 0;
        struct Counter {
            conditions: usize,
            breaks: usize,
        }
        impl Visitor for Counter {
            fn visit_expr(&mut self, _ast: &Ast, _id: ExprId) {
                self.conditions += 1;
            }
            fn visit_stmt(&mut self, ast: &Ast, id: StmtId) {
                if matches!(ast.stmt(id).kind, StmtKind::Break) {
                    self.breaks += 1;
                }
            }
        }
        let mut counter = Counter {
            conditions: 0,
            breaks: 0,
        };
        // The test has only a statement, so walk from it directly.
        walk_stmt(&ast, if_stmt, &mut counter);
        conditions += counter.conditions;
        breaks += counter.breaks;
        assert_eq!(conditions, 1, "the condition must be visited");
        assert_eq!(breaks, 1, "the then-block body must be visited");
    }

    #[test]
    fn walk_visits_both_branches_of_an_if() {
        let span = Span::new(FileId::from_raw(0), 0, 1);
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        let condition = builder.alloc_expr(ExprKind::Unresolved(Unresolved::new("c", span)), span);
        let then_stmt = builder.alloc_stmt(StmtKind::Break, span);
        let then_block = builder.alloc_block(vec![then_stmt], span);
        let else_stmt = builder.alloc_stmt(StmtKind::Continue, span);
        let else_block = builder.alloc_block(vec![else_stmt], span);
        let if_stmt = builder.alloc_stmt(
            StmtKind::If(IfStmt {
                is_constexpr: false,
                condition,
                condition_attrs: Vec::new(),
                then_block,
                else_branch: Some(ElseBranch::Block(else_block)),
                else_attrs: Vec::new(),
            }),
            span,
        );
        // The test has no item, so it walks from the statement.
        let _ = if_stmt;

        let mut breaks = 0;
        let mut continues = 0;
        struct KindCounter {
            breaks: usize,
            continues: usize,
        }
        impl Visitor for KindCounter {
            fn visit_stmt(&mut self, ast: &Ast, id: StmtId) {
                match ast.stmt(id).kind {
                    StmtKind::Break => self.breaks += 1,
                    StmtKind::Continue => self.continues += 1,
                    _ => {}
                }
            }
        }
        let mut counter = KindCounter {
            breaks: 0,
            continues: 0,
        };
        walk_stmt(&ast, if_stmt, &mut counter);
        breaks += counter.breaks;
        continues += counter.continues;
        assert_eq!(breaks, 1);
        assert_eq!(continues, 1, "the else branch must be visited");
    }

    #[test]
    fn walk_reaches_switch_arms_and_labels() {
        let span = Span::new(FileId::from_raw(0), 0, 1);
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        let scrutinee = builder.alloc_expr(ExprKind::Unresolved(Unresolved::new("e", span)), span);
        let label_a = builder.alloc_expr(ExprKind::Unresolved(Unresolved::new("a", span)), span);
        let label_b = builder.alloc_expr(ExprKind::Unresolved(Unresolved::new("b", span)), span);
        let fallthrough = builder.alloc_stmt(StmtKind::Fallthrough, span);
        let switch = builder.alloc_stmt(
            StmtKind::Switch(SwitchStmt {
                scrutinee,
                arms: vec![
                    SwitchArm {
                        labels: Some(vec![label_a, label_b]),
                        body: vec![fallthrough],
                        span,
                        attrs: Vec::new(),
                    },
                    SwitchArm {
                        labels: None,
                        body: vec![],
                        span,
                        attrs: Vec::new(),
                    },
                ],
            }),
            span,
        );
        // `builder` borrows `ast` mutably; ending its scope here releases it.

        let mut exprs = 0;
        let mut fallthroughs = 0;
        struct Counter {
            exprs: usize,
            fallthroughs: usize,
        }
        impl Visitor for Counter {
            fn visit_expr(&mut self, _ast: &Ast, _id: ExprId) {
                self.exprs += 1;
            }
            fn visit_stmt(&mut self, ast: &Ast, id: StmtId) {
                if matches!(ast.stmt(id).kind, StmtKind::Fallthrough) {
                    self.fallthroughs += 1;
                }
            }
        }
        let mut counter = Counter {
            exprs: 0,
            fallthroughs: 0,
        };
        walk_stmt(&ast, switch, &mut counter);
        exprs += counter.exprs;
        fallthroughs += counter.fallthroughs;
        // scrutinee + two labels
        assert_eq!(exprs, 3, "scrutinee and both labels must be visited");
        assert_eq!(fallthroughs, 1);
    }

    #[test]
    fn walk_reaches_every_try_clause() {
        let span = Span::new(FileId::from_raw(0), 0, 1);
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        let body = builder.alloc_block(vec![], span);
        let except_stmt = builder.alloc_stmt(StmtKind::Break, span);
        let except_block = builder.alloc_block(vec![except_stmt], span);
        let else_stmt = builder.alloc_stmt(StmtKind::Continue, span);
        let else_block = builder.alloc_block(vec![else_stmt], span);
        let finally_stmt = builder.alloc_stmt(StmtKind::Return(ReturnStmt { value: None }), span);
        let finally_block = builder.alloc_block(vec![finally_stmt], span);
        let try_stmt = builder.alloc_stmt(
            StmtKind::Try(TryStmt {
                body,
                except_clauses: vec![fallc_ast::ExceptClause {
                    names: Vec::new(),
                    block: except_block,
                    span,
                }],
                else_block: Some(else_block),
                finally_block: Some(finally_block),
            }),
            span,
        );
        // `builder` borrows `ast` mutably; ending its scope here releases it.

        let mut blocks = 0;
        struct BlockCounter(usize);
        impl Visitor for BlockCounter {
            fn visit_block(&mut self, _ast: &Ast, _id: BlockId) {
                self.0 += 1;
            }
        }
        let mut counter = BlockCounter(0);
        walk_stmt(&ast, try_stmt, &mut counter);
        blocks += counter.0;
        assert_eq!(blocks, 4, "try, except, else, and finally blocks");
    }
}
