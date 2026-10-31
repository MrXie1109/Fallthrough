//! AST nodes and the arena that owns them.
//!
//! # The arena
//!
//! [`Ast`] owns every node. Nodes reference each other by index ([`NodeId`]),
//! never by pointer or `Box`. A pass that wants to rewrite a subtree replaces
//! the node at that id; a pass that wants to *insert* a node calls
//! `AstBuilder` and gets a fresh id.
//!
//! # Node set
//!
//! [`ItemKind`], [`StmtKind`], [`ExprKind`], and [`TypeSyntax`] each carry a
//! variant per form the language has, and each has an `Unresolved` variant that
//! holds raw text -- a place for a construct the parser could not recognise, so
//! that one bad node never stops the rest of the tree from being built.

use fallc_span::Span;

/// Identifies a node inside an [`Ast`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(u32);

impl NodeId {
    /// A node id that never refers to a real node.
    ///
    /// Used as the "no parent" / "not yet resolved" sentinel so that
    /// `Option<NodeId>` is not needed in hot structures.
    pub const DUMMY: NodeId = NodeId(u32::MAX);

    /// Creates a node id from its raw index.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        NodeId(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns `true` if this is [`NodeId::DUMMY`].
    #[must_use]
    pub const fn is_dummy(self) -> bool {
        self.0 == u32::MAX
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_dummy() {
            f.write_str("#dummy")
        } else {
            write!(f, "#{}", self.0)
        }
    }
}

/// A complete parsed source file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ast {
    /// The item ids making up this file, in source order.
    pub items: Vec<ItemId>,
    /// Arena storage for items.
    pub(crate) item_data: Vec<Item>,
    /// Arena storage for statements.
    pub(crate) stmt_data: Vec<Stmt>,
    /// Arena storage for expressions.
    pub(crate) expr_data: Vec<Expr>,
    /// Arena storage for blocks.
    pub(crate) block_data: Vec<Block>,
}

impl Default for Ast {
    fn default() -> Self {
        Ast::new()
    }
}

impl Ast {
    /// Creates an empty tree.
    #[must_use]
    pub fn new() -> Self {
        Ast {
            items: Vec::new(),
            item_data: Vec::new(),
            stmt_data: Vec::new(),
            expr_data: Vec::new(),
            block_data: Vec::new(),
        }
    }

    /// Returns a builder that allocates into this tree.
    pub fn builder(&mut self) -> AstBuilder<'_> {
        AstBuilder { ast: self }
    }

    /// Returns the item with `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is a dummy or was not produced by this tree.
    #[must_use]
    pub fn item(&self, id: ItemId) -> &Item {
        &self.item_data[id.0 as usize]
    }

    /// Returns the statement with `id`.
    #[must_use]
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmt_data[id.0 as usize]
    }

    /// Returns the expression with `id`.
    #[must_use]
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.expr_data[id.0 as usize]
    }

    /// Returns a mutable reference to the item with `id`.
    pub fn item_mut(&mut self, id: ItemId) -> &mut Item {
        &mut self.item_data[id.0 as usize]
    }

    /// Returns a mutable reference to the statement with `id`.
    pub fn stmt_mut(&mut self, id: StmtId) -> &mut Stmt {
        &mut self.stmt_data[id.0 as usize]
    }

    /// Returns a mutable reference to the expression with `id`.
    pub fn expr_mut(&mut self, id: ExprId) -> &mut Expr {
        &mut self.expr_data[id.0 as usize]
    }

    /// Returns the block with `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is a dummy or was not produced by this tree.
    #[must_use]
    pub fn block(&self, id: BlockId) -> &Block {
        &self.block_data[id.0 as usize]
    }

    /// Returns a mutable reference to the block with `id`.
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.block_data[id.0 as usize]
    }

    /// Returns the number of allocated blocks.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.block_data.len()
    }

    /// Returns the number of allocated expressions.
    #[must_use]
    pub fn expr_count(&self) -> usize {
        self.expr_data.len()
    }

    /// Returns the number of allocated statements.
    #[must_use]
    pub fn stmt_count(&self) -> usize {
        self.stmt_data.len()
    }

    /// Returns the number of allocated items.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_data.len()
    }

    /// Resolves a [`NodeId`] to the span of the node it names.
    ///
    /// # Panics
    ///
    /// Panics if `id` does not name a node in this tree.
    #[must_use]
    pub fn span_of(&self, id: NodeId) -> Span {
        // Nodes are allocated with their kind tag in the high bits, so a bare
        // NodeId is not self-describing. Callers that need a span should use
        // the typed accessors; this exists for generic infrastructure and
        // resolves by probing the arenas.
        for item in &self.item_data {
            if item.node == id {
                return item.span;
            }
        }
        for stmt in &self.stmt_data {
            if stmt.node == id {
                return stmt.span;
            }
        }
        for expr in &self.expr_data {
            if expr.node == id {
                return expr.span;
            }
        }
        for block in &self.block_data {
            if block.node == id {
                return block.span;
            }
        }
        Span::SYNTHETIC
    }
}

/// Allocates nodes into an [`Ast`], returning their ids.
///
/// The parser is the main user: it builds a tree bottom-up and needs to create
/// parent nodes after their children exist.
#[derive(Debug)]
pub struct AstBuilder<'ast> {
    ast: &'ast mut Ast,
}

impl AstBuilder<'_> {
    /// Allocates an expression node.
    pub fn alloc_expr(&mut self, kind: ExprKind, span: Span) -> ExprId {
        let id = ExprId(
            u32::try_from(self.ast.expr_data.len()).expect("expression count exceeds u32::MAX"),
        );
        self.ast.expr_data.push(Expr {
            node: NodeId(id.0),
            kind,
            span,
        });
        id
    }

    /// Allocates a statement node.
    pub fn alloc_stmt(&mut self, kind: StmtKind, span: Span) -> StmtId {
        let id = StmtId(
            u32::try_from(self.ast.stmt_data.len()).expect("statement count exceeds u32::MAX"),
        );
        self.ast.stmt_data.push(Stmt {
            node: NodeId(id.0),
            kind,
            span,
        });
        id
    }

    /// Allocates a block.
    pub fn alloc_block(&mut self, statements: Vec<StmtId>, span: Span) -> BlockId {
        let id = BlockId(
            u32::try_from(self.ast.block_data.len()).expect("block count exceeds u32::MAX"),
        );
        self.ast.block_data.push(Block {
            node: NodeId(id.0),
            statements,
            span,
        });
        id
    }

    /// Allocates an item node.
    pub fn alloc_item(&mut self, kind: ItemKind, span: Span) -> ItemId {
        let id =
            ItemId(u32::try_from(self.ast.item_data.len()).expect("item count exceeds u32::MAX"));
        self.ast.item_data.push(Item {
            node: NodeId(id.0),
            kind,
            span,
            attrs: Vec::new(),
        });
        id
    }

    /// Appends an item to the file's top level.
    pub fn push_item(&mut self, id: ItemId) {
        self.ast.items.push(id);
    }
}

/// Identifies an [`Item`] in the arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ItemId(u32);

/// Identifies a [`Stmt`] in the arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StmtId(u32);

/// Identifies an [`Expr`] in the arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExprId(u32);

/// Identifies a [`Block`] in the arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BlockId(u32);

macro_rules! typed_id_impl {
    ($name:ident, $label:literal) => {
        impl $name {
            /// A placeholder id that refers to no node.
            pub const DUMMY: $name = $name(u32::MAX);

            /// Creates an id from its raw index.
            #[must_use]
            pub const fn from_raw(raw: u32) -> Self {
                $name(raw)
            }

            /// Returns the raw index.
            #[must_use]
            pub const fn as_raw(self) -> u32 {
                self.0
            }

            /// Returns `true` if this is the dummy id.
            #[must_use]
            pub const fn is_dummy(self) -> bool {
                self.0 == u32::MAX
            }

            /// Returns the corresponding untyped [`NodeId`].
            #[must_use]
            pub const fn as_node(self) -> NodeId {
                NodeId(self.0)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                if self.is_dummy() {
                    f.write_str(concat!($label, "#dummy"))
                } else {
                    write!(f, concat!($label, "#{}"), self.0)
                }
            }
        }
    };
}

typed_id_impl!(ItemId, "item");
typed_id_impl!(StmtId, "stmt");
typed_id_impl!(ExprId, "expr");
typed_id_impl!(BlockId, "block");

/// A top-level declaration node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item {
    /// The node's own id.
    pub node: NodeId,
    /// What kind of item this is.
    pub kind: ItemKind,
    /// The full source range of the item.
    pub span: Span,
    /// Attributes attached to this item.
    pub attrs: Vec<Attr>,
}

/// A statement node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stmt {
    /// The node's own id.
    pub node: NodeId,
    /// What kind of statement this is.
    pub kind: StmtKind,
    /// The full source range of the statement.
    pub span: Span,
}

/// A brace-delimited sequence of statements.
///
/// Blocks are arena nodes rather than inline `Vec<StmtId>` fields so that a
/// pass can rewrite a block's contents in place, and so that every construct
/// that owns a block refers to it by one id.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Block {
    /// The node's own id.
    pub node: NodeId,
    /// The statements in this block, in source order.
    pub statements: Vec<StmtId>,
    /// The block's source range, including its braces.
    pub span: Span,
}

/// An expression node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Expr {
    /// The node's own id.
    pub node: NodeId,
    /// What kind of expression this is.
    pub kind: ExprKind,
    /// The full source range of the expression.
    pub span: Span,
}

/// An attribute, written `@name` or `@name(args)`, attached to a declaration.
///
/// The first set is `@inline`, `@noreturn`, `@deprecated`, and `@align(N)`, and
/// a declaration may carry several.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Attr {
    /// The attribute's name, without the `@`.
    pub name: String,
    /// Where the name is written, for a diagnostic about the attribute itself.
    pub name_span: Span,
    /// The arguments, in the order written; empty for `@name`.
    ///
    /// They are expressions so that `@align(16)` and `@align(alignof(T))` are
    /// one syntax rather than two, and so a diagnostic can point at the
    /// argument that was wrong.
    pub arguments: Vec<ExprId>,
    /// The attribute's source range, including `@` and any argument list.
    pub span: Span,
}

impl Attr {
    /// Creates an attribute.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        name_span: Span,
        arguments: Vec<ExprId>,
        span: Span,
    ) -> Self {
        Attr {
            name: name.into(),
            name_span,
            arguments,
            span,
        }
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
    fn builder_allocates_distinct_ids() {
        let mut ast = Ast::new();
        let a = ast.builder().alloc_expr(
            ExprKind::Unresolved(Unresolved::new("1", span(0, 1))),
            span(0, 1),
        );
        let b = ast.builder().alloc_expr(
            ExprKind::Unresolved(Unresolved::new("2", span(2, 3))),
            span(2, 3),
        );
        assert_ne!(a, b);
        assert_eq!(ast.expr_count(), 2);
        assert_eq!(ast.expr(a).span, span(0, 1));
    }

    #[test]
    fn builder_appends_items_in_order() {
        let mut ast = Ast::new();
        let mut builder = ast.builder();
        let first = builder.alloc_item(
            ItemKind::Unresolved(Unresolved::new("a", span(0, 1))),
            span(0, 1),
        );
        let second = builder.alloc_item(
            ItemKind::Unresolved(Unresolved::new("b", span(2, 3))),
            span(2, 3),
        );
        builder.push_item(first);
        builder.push_item(second);
        // `builder` borrows `ast` mutably; ending its scope here releases it.
        assert_eq!(ast.items, vec![first, second]);
    }

    #[test]
    fn span_of_finds_allocated_nodes() {
        let mut ast = Ast::new();
        let e = ast.builder().alloc_expr(
            ExprKind::Unresolved(Unresolved::new("x", span(5, 6))),
            span(5, 6),
        );
        assert_eq!(ast.span_of(e.as_node()), span(5, 6));
        assert!(ast.span_of(NodeId::from_raw(9999)).is_synthetic());
    }

    #[test]
    fn dummy_ids_are_distinguishable() {
        assert!(ItemId::DUMMY.is_dummy());
        assert!(!ItemId::from_raw(0).is_dummy());
        assert_eq!(ExprId::from_raw(3).as_node(), NodeId::from_raw(3));
    }
}

mod expressions;
mod items;
mod statements;
mod types;

pub use expressions::*;
pub use items::*;
pub use statements::*;
pub use types::*;

impl Ast {
    /// Replaces every top-level `if (cond) @constexpr` with the branch its
    /// condition selects.
    ///
    /// The branches that are dropped are never checked afterwards, which is
    /// what the form is for: code that only exists for some builds may name
    /// what other builds do not have. A condition that is not a constant
    /// leaves the item in place, and the checker reports it.
    pub fn select_conditional_items(&mut self, platform: &dyn Fn(&str) -> Option<bool>) {
        let mut selected = Vec::with_capacity(self.items.len());
        self.flatten_items(&self.items.clone(), &mut selected, platform);
        self.items = selected;
    }

    fn flatten_items(
        &self,
        items: &[ItemId],
        out: &mut Vec<ItemId>,
        platform: &dyn Fn(&str) -> Option<bool>,
    ) {
        for &item in items {
            match &self.item(item).kind {
                crate::node::ItemKind::Conditional(node) => {
                    let branch = match self.conditional_value(node.condition, platform) {
                        Some(true) => &node.then_items,
                        Some(false) => &node.else_items,
                        None => {
                            // Not decided here: keep the node so the checker
                            // can say the condition is not a constant.
                            out.push(item);
                            continue;
                        }
                    };
                    let branch = branch.clone();
                    self.flatten_items(&branch, out, platform);
                }
                _ => out.push(item),
            }
        }
    }

    /// The value of a conditional-item condition, when it is a literal.
    fn conditional_value(
        &self,
        condition: ExprId,
        platform: &dyn Fn(&str) -> Option<bool>,
    ) -> Option<bool> {
        match &self.expr(condition).kind {
            crate::node::ExprKind::BoolLiteral(value) => Some(*value),
            crate::node::ExprKind::IntLiteral(literal) => {
                crate::literal::decode_int_value(&literal.digits).map(|value| value != 0)
            }
            // A compile-time question about the build is answerable here for a
            // platform name, because the platform is this compiler's, not the
            // program's: `if (@meta.defined(linux)) @constexpr` chooses its
            // branch before anything is resolved. Every other question needs
            // the resolution tables, and is left for the checker.
            crate::node::ExprKind::Builtin(query) => platform(&query.argument),
            // Everything else -- a name, a call, a comparison -- is decided by
            // the checker's constant folder, which needs the resolution tables.
            // The nested conditions are left for it.
            _ => None,
        }
    }
}
