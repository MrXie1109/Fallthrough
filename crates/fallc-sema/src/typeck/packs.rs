//! Parameter packs: one parameter per element.
//!
//! A function that declares `values ...Ts` takes any number of arguments
//! instead of one. How many is decided by the call, and after that the elements
//! are ordinary parameters: this module expands the declaration and the body of
//! such a function into an ordinary function with one parameter per element, so
//! nothing further along has to know that a pack existed.
//!
//! What is expanded, and into what:
//!
//! * the pack parameter becomes `name$0` … `name$n-1`, one per element;
//! * `f(values...)` becomes `f(name$0, …, name$n-1)`;
//! * `(pattern, ...)` becomes one statement per element, each with the pack
//!   name standing for that element;
//! * `sizeof...(values)` becomes the count, a `usize` literal.
//!
//! The expansion happens on a *copy* of the tree. A template body is one set of
//! expressions shared by every instantiation, and one call's expansion must not
//! be visible to another's.

use super::*;
use fallc_ast::{BlockId, ExprId, StmtId};

/// A function that declared a pack, expanded for one call.
pub(super) struct Expanded {
    /// The copy of the tree the expanded body lives in.
    pub tree: Ast,
    /// The function with one parameter per element.
    pub function: fallc_ast::FunctionItem,
    /// The types of the expanded parameters, in order.
    pub param_types: Vec<Type>,
    /// The spans of the copied names, paired with the names they copy.
    ///
    /// A copy past the first has a span of its own -- the checker files what it
    /// resolves by span, and two repetitions must not share each other's
    /// answers -- so the name it copies is bound to the same definition at the
    /// new span.
    pub name_spans: Vec<(Span, Span)>,
}

/// Expands `function`, whose `pack_index`th parameter is a pack.
///
/// `param_types` is the whole parameter list already expanded: the fixed
/// parameters first, then one type per element. `Err` when the body uses the
/// pack where it cannot be expanded -- as an ordinary value, or in a fold that
/// is not a statement -- and the caller reports the message.
pub(super) fn expand(
    ast: &Ast,
    function: &fallc_ast::FunctionItem,
    pack_index: usize,
    param_types: &[Type],
) -> Result<Expanded, PackProblem> {
    let pack = function
        .params
        .get(pack_index)
        .ok_or(PackProblem::ValueUse)?;
    let element_count = param_types.len().saturating_sub(pack_index);
    let mut expander = Expander {
        source: ast,
        tree: Ast::new(),
        pack: pack.name.clone(),
        count: element_count,
        element: None,
        problem: None,
        name_spans: Vec::new(),
    };
    // The fixed parameters are copied unchanged; the pack becomes one
    // parameter per element, each with its own name and a synthetic span, so
    // that the binding tables can tell them apart.
    let mut params: Vec<fallc_ast::Param> = function.params[..pack_index].to_vec();
    for index in 0..element_count {
        let name = format!("{}${index}", pack.name);
        let name_span = synthetic_span(index);
        params.push(fallc_ast::Param {
            name,
            name_span,
            ty: pack.ty.clone(),
            default: None,
            is_pack: false,
            span: name_span,
        });
    }
    let body = function.body.ok_or(PackProblem::ValueUse)?;
    let body = expander.block(body);
    if let Some(problem) = expander.problem {
        return Err(problem);
    }
    let expanded_function = fallc_ast::FunctionItem {
        abi: function.abi.clone(),
        is_constexpr: function.is_constexpr,
        exported: function.exported,
        throws: function.throws.clone(),
        is_external: function.is_external,
        variadic: function.variadic,
        template_params: function.template_params.clone(),
        name: function.name.clone(),
        overloaded: function.overloaded,
        name_span: function.name_span,
        params,
        return_type: function.return_type.clone(),
        body: Some(body),
    };
    Ok(Expanded {
        tree: expander.tree,
        function: expanded_function,
        param_types: param_types.to_vec(),
        name_spans: expander.name_spans,
    })
}

/// The elements a name refers to, for the caller to bind in its own tables.
///
/// Every synthetic parameter's name and span, in order. The caller records them
/// as parameters so that a use inside the expanded body has a definition.
pub(super) fn element_bindings(expanded: &Expanded, pack_index: usize) -> Vec<(String, Span)> {
    expanded.function.params[pack_index..]
        .iter()
        .map(|param| (param.name.clone(), param.name_span))
        .collect()
}

/// A unique span for one synthetic parameter.
///
/// The spans are only keys -- the names do not exist in any source file -- but
/// they must differ, because a binding table is keyed by the declaration's
/// span. They are marked synthetic, so a diagnostic that lands on one says the
/// code was generated rather than pointing at a wrong line.
/// The next compiler-generated span, unique across the whole compilation.
///
/// Every expansion used to number its own copies from zero, so two
/// instantiations generated the same `SYNTHETIC` spans and the tables keyed by
/// them shared answers: one call's receiver was lowered with another call's
/// recorded receiver. A process-wide counter makes a generated span mean one
/// node for the whole run.
fn unique_span() -> Span {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    Span::new(fallc_span::FileId::SYNTHETIC, serial, serial.saturating_add(1))
}

fn synthetic_span(index: usize) -> Span {
    // A range of its own, far from the counter the copied nodes use: a copy's
    // span is a key in the checker's tables, and an element's span is another.
    // Sharing one would make an element's name mean whatever definition the
    // copy at the same offset was bound to.
    use std::sync::atomic::{AtomicU32, Ordering};
    let _ = index;
    static NEXT: AtomicU32 = AtomicU32::new(0x4000_0000);
    let lo = NEXT.fetch_add(1, Ordering::Relaxed);
    Span::new(fallc_span::FileId::SYNTHETIC, lo, lo.saturating_add(1))
}

/// Which misuse of a pack was found while expanding.
#[derive(Clone, Copy, Debug)]
pub(super) enum PackProblem {
    /// The pack's name was used as an ordinary value.
    ValueUse,
    /// A comma fold appeared where a value is wanted.
    UnsupportedFold,
}

impl PackProblem {
    /// The diagnostic this problem is reported with.
    pub(super) fn message(self) -> &'static str {
        match self {
            PackProblem::ValueUse => {
                "a parameter pack cannot be used as a value: expand it with `...`"
            }
            PackProblem::UnsupportedFold => {
                "a comma fold must be a statement: `(call(pack), ...);`"
            }
        }
    }
}

/// Copies a tree, expanding one pack as it goes.
struct Expander<'a> {
    /// The tree being copied.
    source: &'a Ast,
    /// The copy being built.
    tree: Ast,
    /// The pack parameter's name.
    pack: String,
    /// How many elements there are.
    count: usize,
    /// Which element the name currently stands for, while one copy is made.
    element: Option<usize>,
    /// The first misuse, if the body cannot be expanded.
    problem: Option<PackProblem>,
    /// The spans of the copied names, and the names they copy.
    name_spans: Vec<(Span, Span)>,
}

impl Expander<'_> {
    /// The name one element is bound to.
    fn element_name(&self, index: usize) -> String {
        format!("{}${index}", self.pack)
    }

    fn expr(&mut self, id: ExprId) -> ExprId {
        let node = self.source.expr(id);
        let span = node.span;
        let kind = match &node.kind {
            fallc_ast::ExprKind::Name(name) if *name == self.pack => match self.element {
                Some(index) => {
                    // The element's own name, at the element's own span: the
                    // definition the checker needs is keyed by it.
                    let element = self.element_name(index);
                    return self
                        .tree
                        .builder()
                        .alloc_expr(fallc_ast::ExprKind::Name(element), synthetic_span(index));
                }
                None => {
                    self.problem.get_or_insert(PackProblem::ValueUse);
                    fallc_ast::ExprKind::Name(name.clone())
                }
            },
            fallc_ast::ExprKind::SizeofPack(reference) if reference.pack == self.pack => {
                fallc_ast::ExprKind::IntLiteral(fallc_ast::IntLiteral {
                    digits: self.count.to_string(),
                    suffix: Some("usize".to_owned()),
                })
            }
            fallc_ast::ExprKind::PackExpand(inner) => {
                // A nested expansion has no meaning: the elements were already
                // expanded into the argument list they belong to.
                self.problem.get_or_insert(PackProblem::ValueUse);
                fallc_ast::ExprKind::PackExpand(self.expr(*inner))
            }
            fallc_ast::ExprKind::CommaFold { pattern } => {
                self.problem.get_or_insert(PackProblem::UnsupportedFold);
                fallc_ast::ExprKind::CommaFold {
                    pattern: self.expr(*pattern),
                }
            }
            fallc_ast::ExprKind::Comma(node) => fallc_ast::ExprKind::Comma(
                fallc_ast::CommaExpr {
                    lhs: self.expr(node.lhs),
                    rhs: self.expr(node.rhs),
                },
            ),
            fallc_ast::ExprKind::Binary(node) => fallc_ast::ExprKind::Binary(
                fallc_ast::BinaryExpr {
                    op: node.op,
                    lhs: self.expr(node.lhs),
                    rhs: self.expr(node.rhs),
                },
            ),
            fallc_ast::ExprKind::Unary(node) => fallc_ast::ExprKind::Unary(
                fallc_ast::UnaryExpr {
                    op: node.op,
                    operand: self.expr(node.operand),
                },
            ),
            fallc_ast::ExprKind::Assign(node) => fallc_ast::ExprKind::Assign(
                fallc_ast::AssignExpr {
                    target: self.expr(node.target),
                    op: node.op,
                    value: self.expr(node.value),
                },
            ),
            fallc_ast::ExprKind::Call(call) => {
                let mut args = Vec::with_capacity(call.args.len());
                for argument in &call.args {
                    match &self.source.expr(*argument).kind {
                        fallc_ast::ExprKind::PackExpand(inner)
                            if is_this_pack(self.source, *inner, &self.pack) =>
                        {
                            for index in 0..self.count {
                                let previous = self.element.replace(index);
                                let cloned = self.expr(*inner);
                                self.element = previous;
                                args.push(cloned);
                            }
                        }
                        _ => args.push(self.expr(*argument)),
                    }
                }
                fallc_ast::ExprKind::Call(fallc_ast::CallExpr {
                    callee: self.expr(call.callee),
                    args,
                    named: call
                        .named
                        .iter()
                        .map(|named| fallc_ast::NamedArgument {
                            name: named.name.clone(),
                            name_span: named.name_span,
                            value: self.expr(named.value),
                            span: named.span,
                        })
                        .collect(),
                    template_args: call.template_args.clone(),
                    explicit_template_arguments: call.explicit_template_arguments,
                })
            }
            fallc_ast::ExprKind::TypedInit(init) => fallc_ast::ExprKind::TypedInit(
                fallc_ast::TypedInitExpr {
                    type_name: init.type_name.clone(),
                    type_arguments: init.type_arguments.clone(),
                    type_span: init.type_span,
                    init: self.designated(&init.init),
                },
            ),
            fallc_ast::ExprKind::NestedInit(init) => {
                fallc_ast::ExprKind::NestedInit(self.designated(init))
            }
            fallc_ast::ExprKind::Field(field) => fallc_ast::ExprKind::Field(
                fallc_ast::FieldExpr {
                    object: self.expr(field.object),
                    field: field.field.clone(),
                    through_pointer: field.through_pointer,
                },
            ),
            fallc_ast::ExprKind::Index(index) => fallc_ast::ExprKind::Index(
                fallc_ast::IndexExpr {
                    object: self.expr(index.object),
                    index: self.expr(index.index),
                },
            ),
            fallc_ast::ExprKind::IncDec(incdec) => fallc_ast::ExprKind::IncDec(
                fallc_ast::IncDecExpr {
                    op: incdec.op,
                    postfix: incdec.postfix,
                    operand: self.expr(incdec.operand),
                },
            ),
            fallc_ast::ExprKind::Cast(cast) => fallc_ast::ExprKind::Cast(
                fallc_ast::CastExpr {
                    operand: self.expr(cast.operand),
                    ty: cast.ty.clone(),
                },
            ),
            fallc_ast::ExprKind::Conditional(node) => {
                fallc_ast::ExprKind::Conditional(fallc_ast::ConditionalExpr {
                    condition: self.expr(node.condition),
                    then_value: self.expr(node.then_value),
                    else_value: self.expr(node.else_value),
                })
            }
            fallc_ast::ExprKind::Propagate(node) => fallc_ast::ExprKind::Propagate(
                fallc_ast::PropagateExpr {
                    value: self.expr(node.value),
                },
            ),
            fallc_ast::ExprKind::Abort(node) => fallc_ast::ExprKind::Abort(
                fallc_ast::AbortExpr {
                    value: self.expr(node.value),
                },
            ),
            fallc_ast::ExprKind::TypeOf(inner) => {
                fallc_ast::ExprKind::TypeOf(self.expr(*inner))
            }
            fallc_ast::ExprKind::ThrowQuery(inner) => {
                fallc_ast::ExprKind::ThrowQuery(self.expr(*inner))
            }
            fallc_ast::ExprKind::Fold(fold) => fallc_ast::ExprKind::Fold(
                fallc_ast::FoldExpr {
                    pack: fold.pack.clone(),
                    op: fold.op,
                    kind: fold.kind,
                    init: fold.init.map(|init| self.expr(init)),
                },
            ),
            fallc_ast::ExprKind::Lambda(lambda) => {
                let body = match lambda.body {
                    fallc_ast::LambdaBody::Expression(inner) => {
                        fallc_ast::LambdaBody::Expression(self.expr(inner))
                    }
                    fallc_ast::LambdaBody::Block(block) => {
                        fallc_ast::LambdaBody::Block(self.block(block))
                    }
                };
                fallc_ast::ExprKind::Lambda(fallc_ast::LambdaExpr {
                    parameters: lambda.parameters.clone(),
                    body,
                    span: lambda.span,
                })
            }
            // Leaves that carry no expression children.
            fallc_ast::ExprKind::IntLiteral(_)
            | fallc_ast::ExprKind::CharLiteral(_)
            | fallc_ast::ExprKind::FloatLiteral(_)
            | fallc_ast::ExprKind::ByteStringLiteral(_)
            | fallc_ast::ExprKind::StringLiteral(_)
            | fallc_ast::ExprKind::RawStringLiteral(_)
            | fallc_ast::ExprKind::BoolLiteral(_)
            | fallc_ast::ExprKind::NullPointer
            // Another pack's count: copied as written.
            | fallc_ast::ExprKind::SizeofPack(_)
            | fallc_ast::ExprKind::TypeLiteral(_)
            | fallc_ast::ExprKind::Sizeof(_)
            | fallc_ast::ExprKind::Alignof(_)
            | fallc_ast::ExprKind::Unresolved(_) => node.kind.clone(),
            // An ordinary name: a new span, so two repetitions do not share
            // each other's record, but the same definition behind it.
            fallc_ast::ExprKind::Name(name) => {
                let new_span = self.copy_span(span);
                self.name_spans.push((new_span, span));
                return self
                    .tree
                    .builder()
                    .alloc_expr(fallc_ast::ExprKind::Name(name.clone()), new_span);
            }
        };
        let span = self.copy_span(span);
        self.tree.builder().alloc_expr(kind, span)
    }

    /// The span a copy gets.
    ///
    /// The first repetition keeps the written span; every later one is
    /// compiler-generated and unique.
    fn copy_span(&mut self, span: Span) -> Span {
        match self.element {
            Some(0) | None => span,
            Some(_) => {
                unique_span()
            }
        }
    }

    /// Copies a designated initialiser, expanding its elements.
    fn designated(&mut self, init: &fallc_ast::DesignatedInit) -> fallc_ast::DesignatedInit {
        // A nested initialiser is an expression and carries an id; a top-level
        // one does not, and its id is a dummy that stays as it is.
        let dummy = init.expr == ExprId::DUMMY;
        let expr = if dummy { ExprId::DUMMY } else { self.expr(init.expr) };
        fallc_ast::DesignatedInit {
            expr,
            text: init.text.clone(),
            elements: init.elements.iter().map(|id| self.expr(*id)).collect(),
            named: init
                .named
                .iter()
                .map(|named| fallc_ast::NamedArgument {
                    name: named.name.clone(),
                    name_span: named.name_span,
                    value: self.expr(named.value),
                    span: named.span,
                })
                .collect(),
            span: init.span,
        }
    }

    /// Copies one statement, which may become several when a fold expands.
    fn stmt(&mut self, id: StmtId) -> Vec<StmtId> {
        let node = self.source.stmt(id);
        let span = node.span;
        let kind = match &node.kind {
            fallc_ast::StmtKind::Defer(block) => {
                fallc_ast::StmtKind::Defer(self.block(*block))
            }
            fallc_ast::StmtKind::Block(block) => {
                fallc_ast::StmtKind::Block(self.block(*block))
            }
            fallc_ast::StmtKind::Bind(bind) => fallc_ast::StmtKind::Bind(
                fallc_ast::BindStmt {
                    name: bind.name.clone(),
                    name_span: bind.name_span,
                    ty: bind.ty.clone(),
                    init: bind.init.as_ref().map(|init| match init {
                        fallc_ast::Initializer::Expr(expr) => {
                            fallc_ast::Initializer::Expr(self.expr(*expr))
                        }
                        fallc_ast::Initializer::Designated(init) => {
                            fallc_ast::Initializer::Designated(self.designated(init))
                        }
                    }),
                    is_static: bind.is_static,
                    is_thread_local: bind.is_thread_local,
                    align: bind.align.as_ref().map(|align| fallc_ast::AlignSpec {
                        alignment: self.expr(align.alignment),
                        span: align.span,
                    }),
                    exported: bind.exported,
                    is_uninitialized: bind.is_uninitialized,
                    attrs: bind.attrs.clone(),
                },
            ),
            fallc_ast::StmtKind::If(node) => fallc_ast::StmtKind::If(fallc_ast::IfStmt {
                is_constexpr: node.is_constexpr,
                condition: self.expr(node.condition),
                condition_attrs: node.condition_attrs.clone(),
                then_block: self.block(node.then_block),
                else_branch: match &node.else_branch {
                    Some(fallc_ast::ElseBranch::Block(block)) => {
                        Some(fallc_ast::ElseBranch::Block(self.block(*block)))
                    }
                    Some(fallc_ast::ElseBranch::If(statement)) => {
                        let mut cloned = self.stmt(*statement);
                        // An `else if` is exactly one statement; anything else
                        // has no representation here.
                        match cloned.pop() {
                            Some(first) => Some(fallc_ast::ElseBranch::If(first)),
                            None => {
                                self.problem.get_or_insert(PackProblem::ValueUse);
                                return Vec::new();
                            }
                        }
                    }
                    None => None,
                },
                else_attrs: node.else_attrs.clone(),
            }),
            fallc_ast::StmtKind::While(node) => fallc_ast::StmtKind::While(
                fallc_ast::LoopStmt {
                    condition: self.expr(node.condition),
                    body: self.block(node.body),
                    else_block: node.else_block.map(|block| self.block(block)),
                },
            ),
            fallc_ast::StmtKind::ForC(node) => fallc_ast::StmtKind::ForC(
                fallc_ast::ForCStmt {
                    init: match &node.init {
                        Some(init) => {
                            let mut cloned = self.stmt(*init);
                            match cloned.pop() {
                                Some(first) => Some(first),
                                None => {
                                    self.problem.get_or_insert(PackProblem::ValueUse);
                                    return Vec::new();
                                }
                            }
                        }
                        None => None,
                    },
                    condition: node.condition.map(|condition| self.expr(condition)),
                    update: node.update.map(|update| self.expr(update)),
                    body: self.block(node.body),
                    else_block: node.else_block.map(|block| self.block(block)),
                },
            ),
            fallc_ast::StmtKind::Switch(node) => fallc_ast::StmtKind::Switch(
                fallc_ast::SwitchStmt {
                    scrutinee: self.expr(node.scrutinee),
                    arms: node
                        .arms
                        .iter()
                        .map(|arm| fallc_ast::SwitchArm {
                            labels: arm
                                .labels
                                .as_ref()
                                .map(|labels| labels.iter().map(|id| self.expr(*id)).collect()),
                            body: self.stmts(&arm.body),
                            span: arm.span,
                            attrs: arm.attrs.clone(),
                        })
                        .collect(),
                },
            ),
            fallc_ast::StmtKind::Fallthrough => fallc_ast::StmtKind::Fallthrough,
            fallc_ast::StmtKind::Break => fallc_ast::StmtKind::Break,
            fallc_ast::StmtKind::Continue => fallc_ast::StmtKind::Continue,
            fallc_ast::StmtKind::Return(node) => fallc_ast::StmtKind::Return(
                fallc_ast::ReturnStmt {
                    value: node.value.map(|value| self.expr(value)),
                },
            ),
            fallc_ast::StmtKind::Raise(node) => fallc_ast::StmtKind::Raise(
                fallc_ast::RaiseStmt {
                    exception_type: node.exception_type.map(|id| self.expr(id)),
                    message: node.message.map(|id| self.expr(id)),
                },
            ),
            fallc_ast::StmtKind::Try(node) => fallc_ast::StmtKind::Try(fallc_ast::TryStmt {
                body: self.block(node.body),
                except_clauses: node
                    .except_clauses
                    .iter()
                    .map(|clause| fallc_ast::ExceptClause {
                        names: clause.names.iter().map(|expr| self.expr(*expr)).collect(),
                        block: self.block(clause.block),
                        span: clause.span,
                    })
                    .collect(),
                else_block: node.else_block.map(|block| self.block(block)),
                finally_block: node.finally_block.map(|block| self.block(block)),
            }),
            fallc_ast::StmtKind::Expr(expr) => {
                // A comma fold is a statement: it repeats its call for every
                // element. Anything else is a plain expression statement.
                if let fallc_ast::ExprKind::CommaFold { pattern } =
                    &self.source.expr(expr.expr).kind
                {
                    let mut statements = Vec::with_capacity(self.count);
                    for index in 0..self.count {
                        let previous = self.element.replace(index);
                        let cloned = self.expr(*pattern);
                        self.element = previous;
                        let statement_span = if index == 0 {
                            span
                        } else {
                            unique_span()
                        };
                        statements.push(
                            self.tree
                                .builder()
                                .alloc_stmt(fallc_ast::StmtKind::Expr(
                                    fallc_ast::ExprStmt { expr: cloned },
                                ), statement_span),
                        );
                    }
                    return statements;
                }
                fallc_ast::StmtKind::Expr(fallc_ast::ExprStmt {
                    expr: self.expr(expr.expr),
                })
            }
            fallc_ast::StmtKind::StaticAssert(assert) => {
                fallc_ast::StmtKind::StaticAssert(fallc_ast::StaticAssert {
                    condition: self.expr(assert.condition),
                    message: assert.message.clone(),
                    span: assert.span,
                })
            }
            fallc_ast::StmtKind::Unresolved(unresolved) => {
                fallc_ast::StmtKind::Unresolved(unresolved.clone())
            }
        };
        let span = self.copy_span(span);
        vec![self.tree.builder().alloc_stmt(kind, span)]
    }

    fn stmts(&mut self, ids: &[StmtId]) -> Vec<StmtId> {
        let mut cloned = Vec::with_capacity(ids.len());
        for id in ids {
            cloned.extend(self.stmt(*id));
        }
        cloned
    }

    fn block(&mut self, id: BlockId) -> BlockId {
        let block = self.source.block(id);
        let span = block.span;
        let statements = self.stmts(&block.statements);
        self.tree.builder().alloc_block(statements, span)
    }
}

/// Whether an expression is a use of the named pack.
fn is_this_pack(ast: &Ast, id: ExprId, pack: &str) -> bool {
    match &ast.expr(id).kind {
        fallc_ast::ExprKind::Name(name) => name == pack,
        _ => false,
    }
}
