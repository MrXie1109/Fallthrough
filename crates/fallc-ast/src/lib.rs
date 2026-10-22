//! The Fallthrough abstract syntax tree.
//!
//! # What is here
//!
//! * [`node`] — the tree itself, and the [`NodeId`] arena that owns it.
//! * [`visit`] — traversal traits and a generic walker.
//! * [`pretty`] — a debug dumper, for tests and `--emit=ast`.
//!
//! # Arena, not boxes
//!
//! Nodes live in a [`node::Ast`] arena and refer to each other by [`NodeId`].
//! This is a deliberate framework decision with three consequences worth
//! knowing before writing passes:
//!
//! 1. Lowering passes can rewrite the tree in place instead of rebuilding it.
//! 2. The tree is trivially cloneable, serialisable, and comparable.
//! 3. There are no `Box<Expr>` cycles, so dropping the AST cannot stack
//!    overflow on deeply nested input — a realistic concern for a language
//!    with `fallthrough`, which invites long chains.
//!
//! The node set mirrors the grammar: [`node::ItemKind`], [`node::StmtKind`], and
//! [`node::ExprKind`] each have a variant per form, and [`node::TypeSyntax`]
//! does too — with `Unresolved` for a type the parser could not recognise, so
//! that recovery can keep the tree.

#![forbid(unsafe_code)]

pub mod literal;
pub mod node;
pub mod pretty;
pub mod special;
pub mod visit;

pub use node::{
    AlignSpec, ArrayType, AssignExpr, Ast, AstBuilder, Attr, BinaryExpr, BinaryOp, BindStmt,
    BindingType,
    Block, BlockId, CallExpr, CastExpr, CharLiteral, CommaExpr, ConditionalExpr, ConstItem,
    ConstantMember,
    DesignatedInit, ElseBranch, ExceptClause, EnumDecl, ExternVariableItem, EnumMember, Expr, ExprId, ExprKind, ExprStmt, FieldDecl,
    FieldExpr,
    FloatLiteral, FoldExpr, FoldKind, ForCStmt, FunctionItem, FunctionTypeSyntax,
    IfStmt, ImportItem,
    ImportedMembers, ImportedName, IncDecExpr, IncDecOp, IndexExpr, Initializer, IntLiteral, Item,
    InterfaceDecl, InterfaceMethod,
    LambdaBody, LambdaExpr, LambdaParam,
    NamedArgument,
    ItemId, ItemKind, LinkItem, LinkKind, LinkLocation, LoopStmt, NamedType, NodeId, PackRef, Param,
    RaiseStmt, ReturnStmt, Stmt, StmtId, StmtKind, SwitchArm, SwitchStmt, TemplateArgumentSyntax,
    PropagateExpr, StaticAssert, TemplateDefault, TemplateParam, TemplateParamKind, Throws, TryStmt,
    TypeDecl,
    TypeDeclKind,
    TraitDecl, TraitParam, TraitRequirement, TraitResult, TraitValue, TypeMember, TypeOfType, TypeSyntax,
    TypedInitExpr,
    UnaryExpr, UnaryOp, Unresolved,
};
