//! Compile-time evaluation of functions.
//!
//! A call whose arguments are all known before the program runs is *evaluated*
//! here rather than emitted, and the result is folded into the caller. That is
//! what lets `[square(3)]int` be a legal array length and `if (is_debug())`
//! select a branch at compile time.
//!
//! # Why there is no `constexpr`
//!
//! There is no keyword to opt in. A function is evaluated when it *can* be, and
//! emitted when it cannot -- so a function that is written ordinarily is
//! constant-evaluable exactly when its body consists of things this module
//! knows how to do. Nothing is marked, because the marking would be a promise
//! the compiler already keeps by itself: if the body does something that needs
//! the machine, evaluation simply fails and the call is emitted instead.
//!
//! # What is evaluated
//!
//! Integer arithmetic, comparisons, the conditional and logical operators,
//! local bindings, assignment to them, `if`/`else`, `while`, `return`, and
//! calls to other evaluable functions. Recursion is allowed and bounded.
//!
//! # What is not
//!
//! Anything touching memory the program owns: pointers, arrays, fields,
//! strings, and calls into C. Those produce `None`, which means "not a
//! constant" rather than "wrong" -- the call is then emitted as ordinary code.

use fallc_ast::{Ast, BinaryOp, BlockId, ExprId, ExprKind, StmtId, StmtKind, UnaryOp};
use std::collections::HashMap;

/// How deep a chain of compile-time calls may go before evaluation gives up.
///
/// A recursive function over constants is legitimate -- `factorial(5)` is a
/// constant -- so recursion is not refused outright. A program that does not
/// terminate would otherwise hang the compiler rather than the program, and
/// this is the bound that keeps the compiler's own behaviour finite.
const MAX_DEPTH: u32 = 64;

/// How many statements one evaluation may run, so that a long loop is a
/// failure to evaluate rather than a compiler that never returns.
const MAX_STEPS: u32 = 100_000;

/// A function the evaluator may call.
#[derive(Clone, Debug)]
pub struct Evaluable {
    /// The tree the body's node ids belong to.
    ///
    /// Shared rather than cloned: the table holds one entry per function, and a
    /// copy of the whole tree for each of them made a file with n functions cost
    /// n times its own size in memory -- 600 tiny functions took 850 MB. An
    /// `Rc` is what the table needs: a recursive call borrows the same tree the
    /// evaluator is already running from, so nothing has to give up ownership.
    pub ast: std::rc::Rc<Ast>,
    /// The body to run.
    pub body: BlockId,
    /// The parameter names, in order, which the call's arguments bind to.
    pub params: Vec<String>,
    /// The default of each parameter, in the same order.
    ///
    /// A call may leave a parameter out, and the default is an expression of
    /// this tree evaluated in the call's place. Without them the parameter was
    /// simply unbound, and an `if` whose condition mentioned it took the other
    /// branch -- so `clamp(99)` with `high int = low + 10` folded to 99 instead
    /// of 10. A wrong value rather than a failure to fold.
    pub defaults: Vec<Option<fallc_ast::ExprId>>,
    /// Whether the function is declared `constexpr`.
    ///
    /// This evaluator can run a function that is not marked -- folding is an
    /// optimisation and any function it understands is worth folding. But a
    /// call may only stand where the language *requires* a constant when the
    /// declaration says so, and that decision needs the marker.
    pub is_constexpr: bool,
}

/// Evaluates a call, returning its value or `None` when it is not a constant.
///
/// `lookup` resolves a callee by name, so a call to another evaluable function
/// is evaluated too. It returns `None` for anything the caller cannot evaluate,
/// which ends this attempt without a diagnostic.
pub fn eval_call(
    ast: &Ast,
    body: BlockId,
    params: &[String],
    defaults: &[Option<fallc_ast::ExprId>],
    args: &[i64],
    functions: &HashMap<String, Evaluable>,
    constants: &HashMap<String, i64>,
) -> Option<i64> {
    let mut state = State {
        ast,
        scopes: Vec::new(),
        constants,
        functions,
        steps: 0,
        depth: 0,
        raised: false,
    };
    let mut scope = HashMap::new();
    for (name, value) in params.iter().zip(args) {
        scope.insert(name.clone(), *value);
    }
    // A parameter the call left out takes its default, evaluated here with the
    // parameters before it already bound -- which is what `high int = low + 10`
    // means. A parameter with neither is not something this evaluator can
    // answer, so the call is simply not folded.
    for (index, name) in params.iter().enumerate().skip(args.len()) {
        let default = defaults.get(index).copied().flatten()?;
        state.scopes.push(scope);
        let value = state.eval(default)?;
        scope = state.scopes.pop().unwrap_or_default();
        scope.insert(name.clone(), value);
    }
    state.scopes.push(scope);
    let flow = state.run_block(body);
    if state.raised {
        return None;
    }
    match flow {
        Flow::Return(value) => value,
        // Falling off the end of a function that returns nothing is not a value.
        Flow::Normal | Flow::Break | Flow::Continue => None,
    }
}

/// Evaluates one expression with a set of parameter values in scope.
///
/// Used where a function's *annotation* has to be decided at a call site: the
/// condition of `??(cond)` names the callee's parameters, and the call's
/// arguments say what they are. Evaluating it here means the rule is applied by
/// the same evaluator that folds every other constant, rather than by a second
/// one that could disagree.
pub fn eval_expression(
    ast: &Ast,
    expr: ExprId,
    params: &[String],
    args: &[i64],
    functions: &HashMap<String, Evaluable>,
    constants: &HashMap<String, i64>,
) -> Option<i64> {
    let mut state = State {
        ast,
        scopes: Vec::new(),
        constants,
        functions,
        steps: 0,
        depth: 0,
        raised: false,
    };
    let mut scope = HashMap::new();
    for (name, value) in params.iter().zip(args) {
        scope.insert(name.clone(), *value);
    }
    state.scopes.push(scope);
    state.eval(expr)
}

/// What running a statement did.
enum Flow {
    /// Carried on to the next statement.
    Normal,
    /// A `return`, with the value if there was one.
    Return(Option<i64>),
    /// A `break` or `continue`, which the enclosing loop handles.
    Break,
    Continue,
}

struct State<'a> {
    ast: &'a Ast,
    /// The local bindings, innermost last.
    scopes: Vec<HashMap<String, i64>>,
    /// Module constants, by name.
    constants: &'a HashMap<String, i64>,
    /// Every function the evaluator may call, by name.
    functions: &'a HashMap<String, Evaluable>,
    /// Statements run so far, against [`MAX_STEPS`].
    steps: u32,
    /// Call depth, against [`MAX_DEPTH`].
    depth: u32,
    /// Whether a raise was reached, which means there is no value to fold.
    ///
    /// A `raise` and a `??` are control flow, not arithmetic: the rest of the
    /// body does not run, and a function that raised has no answer to give.
    /// Ignoring that folded `spare int = failing()??;` to the value the body
    /// would have returned had the call succeeded -- a value the program never
    /// sees, and the call itself was then dropped as a folded constant.
    raised: bool,
}

impl State<'_> {
    /// Runs a block, stopping early when a statement returns or breaks.
    fn run_block(&mut self, block: BlockId) -> Flow {
        for &stmt in &self.ast.block(block).statements {
            match self.run_stmt(stmt) {
                Flow::Normal => {}
                other => return other,
            }
        }
        Flow::Normal
    }

    fn run_stmt(&mut self, stmt: StmtId) -> Flow {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Flow::Return(None);
        }
        match &self.ast.stmt(stmt).kind {
            StmtKind::Expr(node) => {
                // Evaluated for its effect on the locals, if any. A statement
                // this evaluator cannot run has an effect it cannot reproduce
                // -- `C.printf(..)` writes a line -- so the whole body stops
                // being foldable. Ignoring the failure folded the *rest* of the
                // body as if the statement were not there, and the value that
                // came out was returned in place of a call that should have run.
                if self.eval(node.expr).is_none() {
                    self.raised = true;
                }
                Flow::Normal
            }
            StmtKind::Bind(node) => {
                // Only a written value initialiser is understood. A designated
                // initialiser builds a struct, which this evaluator has no
                // representation for.
                let value = match &node.init {
                    Some(fallc_ast::Initializer::Expr(expr)) => self.eval(*expr),
                    _ => None,
                };
                self.declare(&node.name, value);
                Flow::Normal
            }
            StmtKind::If(node) => {
                // A condition this evaluator cannot answer stops the
                // evaluation: taking the `else` branch instead folded
                // `if (x > 0) { return 1; } return 2;` to 2 for a module
                // variable it does not model, and a wrong value is worse than
                // no fold.
                let Some(condition) = self.eval(node.condition) else {
                    return Flow::Return(None);
                };
                let taken = condition != 0;
                if taken {
                    return self.run_block(node.then_block);
                }
                match &node.else_branch {
                    Some(fallc_ast::ElseBranch::Block(block)) => self.run_block(*block),
                    Some(fallc_ast::ElseBranch::If(inner)) => self.run_stmt(*inner),
                    None => Flow::Normal,
                }
            }
            StmtKind::While(node) => {
                // Whether the loop was left by `break`, which is what decides
                // whether the `else` clause runs. Getting this wrong is not a
                // failure to evaluate but a *wrong value*: `count_down(3)`
                // returned 3 instead of 103.
                let mut broke = false;
                loop {
                    self.steps += 1;
                    if self.steps > MAX_STEPS {
                        return Flow::Return(None);
                    }
                    match self.eval(node.condition) {
                        Some(value) if value != 0 => {}
                        Some(_) => break,
                        // The same rule as `if`: an unanswerable condition
                        // stops the evaluation rather than pretending the loop
                        // ended, which folded a loop body away and returned
                        // whatever came after it.
                        None => return Flow::Return(None),
                    }
                    match self.run_block(node.body) {
                        Flow::Break => {
                            broke = true;
                            break;
                        }
                        // A `return` leaves the function, so the loop's `else`
                        // never runs and neither does anything after it.
                        Flow::Return(value) => return Flow::Return(value),
                        Flow::Normal | Flow::Continue => {}
                    }
                }
                // The `else` belongs to the loop finishing on its own, which is
                // how `while (..) { .. } else { .. }` is specified.
                if !broke && let Some(block) = node.else_block {
                    return self.run_block(block);
                }
                Flow::Normal
            }
            StmtKind::Return(node) => match node.value {
                Some(value) => Flow::Return(self.eval(value)),
                None => Flow::Return(None),
            },
            StmtKind::Break => Flow::Break,
            StmtKind::Continue => Flow::Continue,
            // Anything else -- a `switch`, a `for`, a `try` -- is not something
            // this evaluator runs. Ending the evaluation here reports "not a
            // constant" rather than guessing at a value.
            _ => Flow::Return(None),
        }
    }

    /// Evaluates an expression, or `None` when it is not a constant.
    fn eval(&mut self, expr: ExprId) -> Option<i64> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return None;
        }
        match &self.ast.expr(expr).kind {
            ExprKind::IntLiteral(literal) => fallc_ast::literal::decode_int_value(&literal.digits),
            ExprKind::BoolLiteral(value) => Some(i64::from(*value)),
            ExprKind::CharLiteral(literal) => {
                fallc_ast::literal::decode_char_code_point(&literal.text).map(i64::from)
            }
            ExprKind::Name(name) => self.lookup_value(name),
            // `??` is the raise itself, seen at the call: whatever the callee
            // returned, the exception it carried is what leaves the function.
            ExprKind::Propagate(_) => {
                self.raised = true;
                None
            }
            // Assignment, which is how a loop counts. Without it `i += 1` did
            // nothing, so every `while` ran until the step bound and the
            // function was reported as unevaluable.
            ExprKind::Assign(assign) => self.eval_assign(assign),
            ExprKind::Unary(unary) => {
                let operand = self.eval(unary.operand)?;
                match unary.op {
                    UnaryOp::Neg => operand.checked_neg(),
                    UnaryOp::Plus => Some(operand),
                    UnaryOp::Not => Some(i64::from(operand == 0)),
                    UnaryOp::BitNot => Some(!operand),
                    // `*p` and `&x` need storage, which is not modelled.
                    _ => None,
                }
            }
            ExprKind::Binary(binary) => self.eval_binary(binary),
            ExprKind::Conditional(node) => {
                if self.eval(node.condition)? != 0 {
                    self.eval(node.then_value)
                } else {
                    self.eval(node.else_value)
                }
            }
            ExprKind::Call(call) => self.eval_call(call),
            ExprKind::Cast(cast) => {
                // Only the integer-to-integer cases are meaningful here; the
                // value itself is unchanged, and narrowing is applied by
                // truncation the way the machine would do it.
                let value = self.eval(cast.operand)?;
                match &cast.ty {
                    fallc_ast::TypeSyntax::Named(named) => {
                        match fallc_sema_type_bits(&named.path) {
                            Some(bits) if bits < 64 => {
                                let mask = (1i64 << bits) - 1;
                                Some(value & mask)
                            }
                            _ => Some(value),
                        }
                    }
                    _ => Some(value),
                }
            }
            // A parenthesised expression is the same node kind, so nothing to
            // do; anything else -- an index, a field, a literal construction --
            // needs storage this evaluator does not have.
            _ => None,
        }
    }

    /// Assigns to a local, returning the assigned value.
    ///
    /// Only a plain name is a valid target here: a field, an index, and a
    /// dereference all need storage this evaluator does not model, and `None`
    /// says so rather than writing somewhere wrong.
    fn eval_assign(&mut self, assign: &fallc_ast::AssignExpr) -> Option<i64> {
        let ExprKind::Name(name) = &self.ast.expr(assign.target).kind else {
            return None;
        };
        let name = name.clone();
        // A compound assignment reads the target first, so `i += 1` is
        // `i = i + 1` and the old value has to be found before the new one is
        // written.
        let value = match assign.op {
            None => self.eval(assign.value)?,
            Some(op) => {
                let current = self.lookup_value(&name)?;
                let rhs = self.eval(assign.value)?;
                apply_binary(op, current, rhs)?
            }
        };
        // Only an existing local may be written. Declaring on assignment would
        // turn a typo into a new variable, and a name that is not a local here
        // is a global or a constant, which this evaluator does not write.
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(&name) {
                *slot = value;
                return Some(value);
            }
        }
        None
    }

    fn eval_binary(&mut self, binary: &fallc_ast::BinaryExpr) -> Option<i64> {
        let lhs = self.eval(binary.lhs)?;
        // Short-circuit before evaluating the right operand, so the operand the
        // answer does not need is never run -- it may divide by zero, or not be
        // a constant at all.
        match binary.op {
            BinaryOp::LogicalAnd => {
                return if lhs == 0 {
                    Some(0)
                } else {
                    Some(i64::from(self.eval(binary.rhs)? != 0))
                };
            }
            BinaryOp::LogicalOr => {
                return if lhs != 0 {
                    Some(1)
                } else {
                    Some(i64::from(self.eval(binary.rhs)? != 0))
                };
            }
            _ => {}
        }
        let rhs = self.eval(binary.rhs)?;
        apply_binary(binary.op, lhs, rhs)
    }

    fn eval_call(&mut self, call: &fallc_ast::CallExpr) -> Option<i64> {
        // Only a plain name is callable here: a method needs a receiver, and a
        // receiver is storage.
        let ExprKind::Name(name) = &self.ast.expr(call.callee).kind else {
            return None;
        };
        // The arguments are evaluated in the caller's scope, before the callee's
        // parameters shadow anything.
        let mut args = Vec::with_capacity(call.args.len());
        for &arg in &call.args {
            args.push(self.eval(arg)?);
        }
        // A recursive call is what the depth bound is for: evaluation stops
        // rather than following the recursion forever.
        if self.depth + 1 > MAX_DEPTH {
            return None;
        }
        let callee = self.functions.get(name)?.clone();
        let mut nested = State {
            ast: &callee.ast,
            scopes: Vec::new(),
            constants: self.constants,
            functions: self.functions,
            steps: 0,
            depth: self.depth + 1,
            raised: false,
        };
        let mut scope = HashMap::new();
        for (name, value) in callee.params.iter().zip(&args) {
            scope.insert(name.clone(), *value);
        }
        nested.scopes.push(scope);
        let flow = nested.run_block(callee.body);
        // A callee that raised has no value, and neither does its caller.
        if nested.raised {
            self.raised = true;
            return None;
        }
        match flow {
            Flow::Return(value) => value,
            _ => None,
        }
    }

    /// Declares a local, shadowing any outer binding of the same name.
    fn declare(&mut self, name: &str, value: Option<i64>) {
        if let Some(scope) = self.scopes.last_mut() {
            // A binding that could not be evaluated is still *declared*, so that
            // a later use reports "not a constant" rather than reading an outer
            // variable of the same name -- which would be a wrong value rather
            // than no value.
            match value {
                Some(value) => {
                    scope.insert(name.to_owned(), value);
                }
                None => {
                    scope.remove(name);
                }
            }
        }
    }

    fn lookup_value(&mut self, name: &str) -> Option<i64> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(*value);
            }
        }
        self.constants.get(name).copied()
    }
}

/// The width in bits of a primitive integer type name, if it is one.
///
/// Used to truncate a cast at compile time the way the machine would. A name
/// that is not a primitive integer leaves the value alone, which is right for a
/// cast between two widths the compiler does not model here.
fn fallc_sema_type_bits(name: &str) -> Option<u32> {
    Some(match name {
        "c8" | "char" | "i8" | "u8" | "byte" => 8,
        "c16" | "i16" | "u16" => 16,
        "c32" | "i32" | "u32" | "int" => 32,
        "i64" | "u64" | "usize" | "isize" => 64,
        _ => return None,
    })
}

/// Applies a binary operator to two known values.
///
/// Shared by the expression form and the compound-assignment form, so `a + b`
/// and `a += b` cannot disagree about what `+` means.
fn apply_binary(op: BinaryOp, lhs: i64, rhs: i64) -> Option<i64> {
    match op {
        BinaryOp::Add => lhs.checked_add(rhs),
        BinaryOp::Sub => lhs.checked_sub(rhs),
        BinaryOp::Mul => lhs.checked_mul(rhs),
        // A division by zero is not a constant: the program would have trapped,
        // and inventing a value would be worse than not folding.
        BinaryOp::Div => lhs.checked_div(rhs),
        BinaryOp::Rem => lhs.checked_rem(rhs),
        BinaryOp::Shl => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shl(s)),
        BinaryOp::Shr => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shr(s)),
        BinaryOp::BitAnd => Some(lhs & rhs),
        BinaryOp::BitOr => Some(lhs | rhs),
        BinaryOp::BitXor => Some(lhs ^ rhs),
        BinaryOp::Eq => Some(i64::from(lhs == rhs)),
        BinaryOp::Ne => Some(i64::from(lhs != rhs)),
        BinaryOp::Lt => Some(i64::from(lhs < rhs)),
        BinaryOp::Le => Some(i64::from(lhs <= rhs)),
        BinaryOp::Gt => Some(i64::from(lhs > rhs)),
        BinaryOp::Ge => Some(i64::from(lhs >= rhs)),
        _ => None,
    }
}
