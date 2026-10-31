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
    AbortExpr, AlignSpec, ArrayLiteral, ArrayType, AssignExpr, Ast, AstBuilder, Attr, BinaryExpr,
    BinaryOp, BindStmt, BindingType, Block, BlockId, BuiltinQuery, CallExpr, CastExpr, CharLiteral,
    CommaExpr, ConditionalExpr, ConditionalItems, ConstItem, ConstantMember, DesignatedInit,
    ElseBranch, EnumDecl, EnumMember, ExceptClause, Expr, ExprId, ExprKind, ExprStmt,
    ExternVariableItem, FieldDecl, FieldExpr, FloatLiteral, FoldExpr, FoldKind, ForCStmt,
    FunctionItem, FunctionTypeSyntax, IfStmt, IfValueExpr, ImportItem, ImportedMembers,
    ImportedName, IncDecExpr, IncDecOp, IndexExpr, Initializer, IntLiteral, InterfaceDecl,
    InterfaceMethod, Item, ItemId, ItemKind, LambdaBody, LambdaExpr, LambdaParam, LinkItem,
    LinkKind, LinkLocation, LoopStmt, NamedArgument, NamedType, NodeId, PackRef, Param,
    PropagateExpr, RaiseStmt, RangeExpr, ReturnStmt, StaticAssert, Stmt, StmtId, StmtKind,
    SwitchArm, SwitchStmt, TemplateArgumentSyntax, TemplateDefault, TemplateParam,
    TemplateParamKind, Throws, TraitDecl, TraitParam, TraitRequirement, TraitResult, TraitValue,
    TryStmt, TypeDecl, TypeDeclKind, TypeMember, TypeOfType, TypeSyntax, TypedInitExpr, UnaryExpr,
    UnaryOp, Unresolved, YieldExpr,
};
