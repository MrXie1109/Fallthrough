//! Statements, bindings and the flow of a body.
//!
//! Conditions, `switch`, loops, bindings -- including `auto` and the reference
//! forms it can deduce -- and the checks a `return` or a `raise` has to pass.

use super::*;

impl TypeChecker {
    /// Checks every statement in a block.
    pub(super) fn check_block(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        block: fallc_ast::BlockId,
    ) {
        // An array length is a constant position, and it is resolved before the
        // statement's own expressions are checked -- so a `constexpr` template
        // call in it had not been instantiated yet, and the length looked like a
        // call nothing could evaluate. The lengths are checked first here, which
        // is what records their values; resolving the type then finds them.
        for &stmt in &ast.block(block).statements.clone() {
            self.precheck_constant_lengths(ast, resolutions, types, stmt);
        }
        // A statement after one that always leaves has nothing to run. The
        // first such statement is reported, and only the first: a block that
        // continues for twenty lines needs one remark, not twenty.
        let mut left: Option<fallc_span::Span> = None;
        for &stmt in &ast.block(block).statements {
            if let Some(span) = left {
                self.diagnostics.push(
                    Diagnostic::warning("this statement can never run")
                        .with_code(DiagnosticCode::sema(141))
                        .with_warning("unreachable")
                        .with_span(ast.stmt(stmt).span)
                        .with_label(span, "the block already left here")
                        .with_note(
                            "the statement above always leaves the block, so nothing below it \
                             runs: remove it, or move it in front of the statement that leaves",
                        ),
                );
                left = None;
            }
            self.check_stmt(ast, resolutions, types, stmt);
            if leaves(&ast.stmt(stmt).kind) {
                left = Some(ast.stmt(stmt).span);
            }
        }
    }

    /// Checks the array-length expressions a statement's written types contain.
    ///
    /// Only the lengths: nothing else about the type is decided here, and the
    /// ordinary check of the statement does all the rest. A length that is not
    /// a constant is left for that ordinary check to report, once, in its own
    /// words.
    pub(super) fn precheck_constant_lengths(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        stmt: fallc_ast::StmtId,
    ) {
        let mut positions = Vec::new();
        collect_constant_positions(&ast.stmt(stmt).kind, &mut positions);
        // This pass exists to *record* values, not to report: the statements
        // are checked afterwards and say what is wrong with their own
        // expressions, in their own words. Keeping the diagnostics here
        // reported a condition twice, and reported it before the block's
        // earlier statements had their bindings recorded -- so a condition
        // that named one looked like a use of a name with no type.
        let before = self.diagnostics.len();
        for length in positions {
            let previous = self.expected.take();
            let _ = self.infer_expr(ast, resolutions, types, length);
            self.expected = previous;
        }
        self.diagnostics.truncate(before);
    }

    /// Checks whichever branch an `if` did not take.
    ///
    /// Shared by the three paths through an `if`, so the `else if` chain is
    /// walked the same way however the condition was decided.
    pub(super) fn check_else_branch(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        branch: &Option<fallc_ast::ElseBranch>,
    ) {
        match branch {
            Some(fallc_ast::ElseBranch::Block(block)) => {
                self.check_block(ast, resolutions, types, *block);
            }
            Some(fallc_ast::ElseBranch::If(inner)) => {
                self.check_stmt(ast, resolutions, types, *inner);
            }
            None => {}
        }
    }

    /// Checks an expression used as a condition.
    ///
    /// A number, a `bool`, a character, or a pointer is true when it is
    /// non-zero, which is built in. Anything else needs `__bool__`, exactly as
    /// another language might need a conversion to `bool`: without one the type
    /// has no truth value, and the diagnostic says so rather than guessing.
    pub(super) fn check_condition(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        condition: fallc_ast::ExprId,
    ) {
        let previous = self.expected.take();
        let ty = self.infer_expr(ast, resolutions, types, condition);
        self.expected = previous;
        // `if (x = f())` reads like a comparison and is an assignment: the old
        // value of `x` is overwritten by what is tested, and the test may then
        // be of the *written* value rather than of the expression. Spelled out
        // as a condition it is nearly always a missing `=`.
        if matches!(
            ast.expr(condition).kind,
            fallc_ast::ExprKind::Assign(_)
        ) {
            self.diagnostics.push(
                Diagnostic::warning("this condition assigns")
                    .with_code(DiagnosticCode::sema(145))
                    .with_warning("assignment-in-condition")
                    .with_span(ast.expr(condition).span)
                    .with_note(
                        "`if (x = f())` looks like a comparison and overwrites `x`: assign \
                         first and then test, or write `==` if a comparison was meant",
                    ),
            );
        }
        let Some(ty) = ty else {
            return;
        };
        // A reference is tested by what it refers to: `if (r)` asks whether the
        // value behind `r` is non-zero, not whether the address is. The read
        // through is recorded, because the address is what the expression
        // *is*, and code generation would otherwise compare that.
        let ty = match ty.referent().cloned() {
            Some(referent) => {
                self.deref_reads
                    .insert(ast.expr(condition).span, referent.clone());
                referent
            }
            None => ty,
        };
        if ty.is_truth_testable() {
            return;
        }
        let span = ast.expr(condition).span;
        if let Some(symbol) = self.resolve_nullary_method(
            resolutions,
            &ty,
            fallc_ast::special::SpecialMethod::Bool,
        ) {
            self.bool_methods.insert(span, symbol);
            // Asked for the value's truth, the conversion is a call site: a
            // `__bool__` that may raise raises here.
            if let Some(name) = ty.nominal_name() {
                let qualified = Self::special_method_qualified(
                    name,
                    fallc_ast::special::SpecialMethod::Bool.name(),
                );
                if let Some(signature) = self
                    .definitions_named(resolutions, &qualified)
                    .into_iter()
                    .find_map(|def| def.signature.clone())
                {
                    self.note_throwing_call(span, &signature, ast, None);
                }
            }
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(format!(
                "`{}` cannot be used as a condition",
                ty.spelling()
            ))
            .with_code(DiagnosticCode::sema(67))
            .with_span(span)
            .with_note(
                "a number, `bool`, character, or pointer is tested directly; \
                 any other type needs `__bool__`",
            ),
        );
    }

    /// Checks one statement.
    ///
    /// Each statement form has its own rule: an expression statement discards
    /// its value, a binding's initialiser must match its declared type, a
    /// condition must be truthy or `bool`, a `return` must match the function's
    /// declared return type, a `case` label must be constant, and a `raise`'s
    /// operands must be string-like (a character array, a character pointer, or
    /// `std.String`).
    pub(super) fn check_stmt(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        stmt: fallc_ast::StmtId,
    ) {
        use fallc_ast::StmtKind;
        match &ast.stmt(stmt).kind {
            StmtKind::Expr(node) => {
                // An expression statement's value is discarded, so its type is
                // not required to be anything. A call marked `@must_use` says
                // that discarding it is a mistake.
                let _ = self.infer_expr(ast, resolutions, types, node.expr);
                let call_span = ast.expr(node.expr).span;
                if self.must_use_calls.contains(&call_span) {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            "the result of this `@must_use` call is discarded",
                        )
                        .with_code(DiagnosticCode::sema(191))
                        .with_warning("unused-result")
                        .with_span(call_span)
                        .with_note(
                            "use the value, or bind it to `_` to say the result is \
                             deliberately dropped",
                        ),
                    );
                }
            }
            StmtKind::Bind(node) => self.check_bind(ast, resolutions, types, node),
            StmtKind::Return(node) => self.check_return(ast, resolutions, types, stmt, node),
            StmtKind::Raise(node) => {
                // A raise that is not caught by a `try` in the same body leaves
                // it, which is what makes a lambda body that raises a throwing
                // function.
                if let Some((baseline, raised)) = self.lambda_raise_stack.last_mut()
                    && self.protected_depth == *baseline
                {
                    *raised = true;
                }
                // Every operand is a C string: the type's name and the
                // message. The type is what `except` shows and what a caller
                // compares against, so it is part of the program's behaviour
                // rather than a tag the compiler chooses.
                for expr in [node.exception_type, node.message].into_iter().flatten() {
                    let found = self.infer_expr(ast, resolutions, types, expr);
                    if let Some(found) = found
                        && !found.is_string_like()
                    {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "a `raise` operand must be a string, but this is `{}`",
                                found.spelling()
                            ))
                            .with_code(DiagnosticCode::sema(82))
                            .with_span(ast.expr(expr).span),
                        );
                    }
                }
                if node.is_reraise() {
                    // `raise;` passes on what is being handled, so it belongs
                    // to the handler that is holding it.
                    if self.except_depth == 0 {
                        self.diagnostics.push(
                            Diagnostic::error("`raise;` outside an `except` block")
                                .with_code(DiagnosticCode::sema(82))
                                .with_span(ast.stmt(stmt).span)
                                .with_note(
                                    "a bare `raise;` re-raises the exception being handled",
                                ),
                        );
                    }
                } else if node.exception_type.is_none() {
                    self.diagnostics.push(
                        Diagnostic::error("`raise` needs a type, or nothing at all to re-raise")
                            .with_code(DiagnosticCode::sema(82))
                            .with_span(ast.stmt(stmt).span),
                    );
                }
                // A raise that nothing in this body catches is what makes an
                // `except(auto)` function a throwing one. The call records in
                // `note_throwing_call` cover a call that may raise; a `raise`
                // written here is the other way a body produces one, and it
                // was the one the deduction did not see -- every
                // `except(auto)` function that raised in its own body was
                // treated as one that cannot.
                if self.throws_auto && self.protected_depth == 0 {
                    self.auto_seen_raise = true;
                }
                self.require_throw_allowed(ast.stmt(stmt).span, "`raise`");
            }
            StmtKind::If(node) => {
                // # The two forms, and how they differ
                //
                // `if constexpr (c)` requires `c` to be a constant, and the
                // branch it does **not** take is never checked -- so that branch
                // may name things this instantiation does not have, which is
                // what makes the form a way to write code that only exists for
                // some arguments.
                //
                // A plain `if` checks *both* branches, whatever its condition
                // is. A constant one is still decided here, and only the branch
                // taken is emitted -- but the other was checked, so a mistake in
                // it is reported. That is the deliberate difference: writing
                // `if` says both branches are meant to compile.
                let constant = self.const_eval_int(node.condition);

                // `if (auto) @constexpr { a } else { b }` asks which branch
                // this build can compile. The `then` block is checked on
                // trial; if that reported anything it is rolled back and the
                // `else` block is the one that counts, which is why a probe
                // needs no separate syntax for "the fallback".
                if node.probes_compilable {
                    let saved_types = types.clone();
                    // The trial is checked into a bag of its own. A diagnostic
                    // bag keeps one copy of each distinct report, so a mistake
                    // that had already been reported somewhere else did not
                    // lengthen the shared bag -- and the probe, which asked
                    // whether anything was added, read that as "the block
                    // compiles". Whether the trial compiles is not a fact about
                    // what else was reported, so the trial records separately.
                    let mut trial = fallc_diag::DiagnosticBag::new();
                    std::mem::swap(&mut self.diagnostics, &mut trial);
                    self.check_block(ast, resolutions, types, node.then_block);
                    std::mem::swap(&mut self.diagnostics, &mut trial);
                    // A warning is not a failure to compile: an unused binding
                    // in the probed block reported the whole branch as
                    // impossible, which made a probe containing any `??` call
                    // take its `else` every time.
                    let failed = trial
                        .iter()
                        .any(|diagnostic| diagnostic.severity.is_fatal());
                    // Code generation emits the branch the probe chose, so the
                    // answer is recorded where the other compile-time
                    // decisions are.
                    self.compile_time_ifs
                        .insert(ast.expr(node.condition).span, !failed);
                    if !failed {
                        // The trial compiled, so it is the live branch and what
                        // it reported about itself is the program's to keep.
                        for diagnostic in trial.iter() {
                            self.diagnostics.push(diagnostic.clone());
                        }
                        return;
                    }
                    // The block does not compile, so the `else` side is what
                    // this program has: the trial's reports are discarded and
                    // its types rolled back before the other branch is checked.
                    *types = saved_types;
                    self.check_else_branch(ast, resolutions, types, &node.else_branch);
                    return;
                }

                if node.is_constexpr {
                    // The condition is typed *before* it is required to be a
                    // constant, because a query such as `except(..)` becomes
                    // one only when it is typed: asking for the value first
                    // refused the very thing this form exists for.
                    //
                    // Typing it here is not a second look. It is the one look
                    // the condition gets, and it is what folds the queries in
                    // it, records the calls they resolve, and reports what is
                    // wrong with them.
                    let previous = self.expected.take();
                    let _ = self.infer_expr(ast, resolutions, types, node.condition);
                    self.expected = previous;
                    // The condition must be a constant *by the language's rule*,
                    // which means a call in it has to be to a `constexpr`
                    // function -- not merely a call this compiler could fold.
                    self.require_constant(
                        node.condition,
                        ast.expr(node.condition).span,
                        "the condition of `if constexpr`",
                        75,
                    );
                    // The value is looked up by the condition's *span* as
                    // well as by expression id: an instantiated body comes from
                    // another tree, and the id-keyed tables were built for the
                    // tree of the unit being checked. The span is the one key
                    // that means the same thing in both.
                    let taken = self.const_eval_int(node.condition).or_else(|| {
                        self.constant_values
                            .get(&ast.expr(node.condition).span)
                            .copied()
                    });
                    let Some(taken) = taken else {
                        // Checked as an ordinary `if`, so the rest of the
                        // function is still typed and the errors in it are
                        // reported alongside this one.
                        self.check_block(ast, resolutions, types, node.then_block);
                        self.check_else_branch(ast, resolutions, types, &node.else_branch);
                        return;
                    };
                    self.compile_time_ifs.insert(ast.expr(node.condition).span, taken != 0);
                    // Only the branch taken is checked. The other is not merely
                    // skipped in the output -- it is never looked at.
                    if taken != 0 {
                        self.check_block(ast, resolutions, types, node.then_block);
                    } else {
                        self.check_else_branch(ast, resolutions, types, &node.else_branch);
                    }
                    return;
                }

                self.check_condition(ast, resolutions, types, node.condition);
                // A condition written as a literal is almost never what the
                // author meant: `if (ture)` and `if (false)` both look like
                // logic and are none. Only a *written* literal counts -- a
                // `const` used as a condition is a deliberate switch.
                if let Some(value) = literal_condition(ast, node.condition) {
                    self.diagnostics.push(
                        Diagnostic::warning(if value {
                            "this condition is always true"
                        } else {
                            "this condition is always false"
                        })
                        .with_code(DiagnosticCode::sema(144))
                        .with_warning("constant-condition")
                        .with_span(ast.expr(node.condition).span)
                        .with_note(
                            "a condition written as a literal is a leftover: name the constant \
                             (`DEBUG const bool = true;`), or write `if constexpr` if the \
                             branch is chosen while compiling",
                        ),
                    );
                }
                self.check_block(ast, resolutions, types, node.then_block);
                self.check_else_branch(ast, resolutions, types, &node.else_branch);
                // A plain `if` whose condition happens to be constant is still
                // *decided* here, so that only the live branch is emitted. Both
                // were checked above, so nothing is hidden -- only the dead code
                // is dropped.
                if let Some(taken) = constant {
                    self.compile_time_ifs.insert(ast.expr(node.condition).span, taken != 0);
                }
            }
            StmtKind::While(node) => {
                self.check_condition(ast, resolutions, types, node.condition);
                // `while (true)` is how an infinite loop is written and means
                // what it says; `while (false)` is a loop that never runs, which
                // is what a mistake looks like.
                if literal_condition(ast, node.condition) == Some(false) {
                    self.diagnostics.push(
                        Diagnostic::warning("this loop can never run")
                            .with_code(DiagnosticCode::sema(144))
                            .with_warning("constant-condition")
                            .with_span(ast.expr(node.condition).span)
                            .with_note(
                                "the condition is false before the first test, so the body and \
                                 the `else` clause are the only code that can run",
                            ),
                    );
                }
                self.check_block(ast, resolutions, types, node.body);
                if let Some(block) = node.else_block {
                    self.check_block(ast, resolutions, types, block);
                }
            }
            StmtKind::ForC(node) => {
                if let Some(init) = node.init {
                    self.check_stmt(ast, resolutions, types, init);
                }
                if let Some(condition) = node.condition {
                    self.check_condition(ast, resolutions, types, condition);
                }
                if let Some(update) = node.update {
                    let _ = self.infer_expr(ast, resolutions, types, update);
                }
                self.check_block(ast, resolutions, types, node.body);
                if let Some(block) = node.else_block {
                    self.check_block(ast, resolutions, types, block);
                }
            }
            StmtKind::Switch(node) => {
                let _ = self.infer_expr(ast, resolutions, types, node.scrutinee);
                // A value may be claimed by one arm only: two arms with the same
                // label would make the switch depend on which one came first,
                // and the generated switch is not defined that way.
                let mut claimed: std::collections::HashMap<i64, Span> =
                    std::collections::HashMap::new();
                for arm in &node.arms {
                    if let Some(labels) = &arm.labels {
                        for &label in labels {
                            let _ = self.infer_expr(ast, resolutions, types, label);
                            // A `case` label is compared at run time by the
                            // generated switch, so it has to be a value the
                            // compiler knows: a label computed when the program
                            // runs would have nothing to compare against.
                            if self.require_constant(
                                label,
                                ast.expr(label).span,
                                "a `case` label",
                                161,
                            ) && let Some(value) = self.const_eval_int(label)
                            {
                                if let Some(first) = claimed.get(&value) {
                                    self.diagnostics.push(
                                        Diagnostic::error(format!(
                                            "`{value}` is already a label of this `switch`"
                                        ))
                                        .with_code(DiagnosticCode::sema(162))
                                        .with_span(ast.expr(label).span)
                                        .with_label(*first, "the first one is here"),
                                    );
                                } else {
                                    claimed.insert(value, ast.expr(label).span);
                                }
                            }
                        }
                    }
                    for &inner in &arm.body {
                        self.check_stmt(ast, resolutions, types, inner);
                    }
                }
            }
            StmtKind::Try(node) => {
                // Only the body is protected, and only when there is an
                // `except` to catch with: a `finally` runs on the way out and
                // does not stop anything, so a call there still needs its own
                // protection.
                // The filter expressions are written in the enclosing scope:
                // `except ("KeyError") { .. }` names an exception, it does not
                // run inside the handler.
                let mut catches_all = false;
                for clause in &node.except_clauses {
                    for &name in &clause.names {
                        let _ = self.infer_expr(ast, resolutions, types, name);
                    }
                    catches_all |= clause.names.is_empty();
                }
                // A handler with no bare clause lets the exceptions it does not
                // name through: a raise whose name is not listed leaves the
                // function, which means the function has to be able to raise.
                // Names are values, not types, so there is no way to prove a
                // list complete.
                if !node.except_clauses.is_empty()
                    && !catches_all
                    && !self.throws_allowed.last().copied().unwrap_or(false)
                {
                    let span = node
                        .except_clauses
                        .iter()
                        .flat_map(|clause| clause.names.first())
                        .next()
                        .map_or_else(|| ast.block(node.body).span, |expr| ast.expr(*expr).span);
                    self.diagnostics.push(
                        Diagnostic::error(
                            "an `except` that names exceptions does not catch the others",
                        )
                        .with_code(DiagnosticCode::sema(192))
                        .with_span(span)
                        .with_note(
                            "an exception whose name is not listed leaves the function, so \
                             the function must be able to raise: write `fn .. except`, or \
                             end the `try` with a bare `except { .. }`",
                        ),
                    );
                }
                // A named handler still protects the call syntactically: the
                // exceptions it does not name leave the function, which the
                // rule above has already required it to be able to do.
                let protects = !node.except_clauses.is_empty();
                if protects {
                    self.protected_depth += 1;
                }
                self.check_block(ast, resolutions, types, node.body);
                if protects {
                    self.protected_depth -= 1;
                }
                for clause in &node.except_clauses {
                    self.except_depth += 1;
                    self.check_block(ast, resolutions, types, clause.block);
                    self.except_depth -= 1;
                }
                for block in [node.else_block, node.finally_block]
                    .into_iter()
                    .flatten()
                {
                    self.check_block(ast, resolutions, types, block);
                }
            }
            StmtKind::StaticAssert(node) => {
                self.check_static_assert(ast, resolutions, types, node);
            }
            StmtKind::Block(block) => self.check_block(ast, resolutions, types, *block),
            // The block is checked in a scope of its own, exactly as a bare
            // block is; where and when it runs is code generation's business.
            StmtKind::Defer(block) => self.check_block(ast, resolutions, types, *block),
            StmtKind::Fallthrough | StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Unresolved(_) => {}
        }
    }

    /// Deduces a binding's type from its initialiser, matching the written type
    /// against the initialiser's, with `auto` as the wildcard.
    ///
    /// # The rules
    ///
    /// A bare `auto` takes the initialiser's **value** type: a reference reads
    /// as the value it names, so `x auto = *it` is a copy and not an alias.
    ///
    /// A modifier is matched too, one indirection at a time, which is what makes
    /// `q *auto = &v` a `*int` rather than a `**int`: the `*` in the type is
    /// paired with the pointer the initialiser already has, and `auto` takes
    /// what is left -- `int`. `&auto` pairs with a place, a `[N]auto` with an
    /// array of that length, and `const auto` with the same value under a
    /// qualifier.
    ///
    /// Three cases have nothing to deduce from, and each says so in its own
    /// words: no initialiser, a designated initialiser (which needs a type to
    /// be interpreted at all), and an initialiser that was already reported
    /// (an unknown name, a call nothing matches) -- the last is silent, because
    /// the mistake has a message already and "cannot deduce a type" would name
    /// the consequence rather than the cause.
    pub(super) fn deduce_auto_binding(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &fallc_ast::BindStmt,
    ) -> Option<Type> {
        let written = match &node.ty {
            fallc_ast::BindingType::Written(written) => written,
            fallc_ast::BindingType::Auto(_) => {
                // A bare `auto` is a value, so the initialiser is typed the way
                // any value is: a reference reads through it. Whether that read
                // is a *load* or the first half of a move is decided later, by
                // the constructor the value's type offers: `b auto = a;` has to
                // copy a `String`'s bytes, not share its buffer.
                let found = self.auto_initialiser(ast, resolutions, types, node, None, false)?;
                return Some(found.referent().cloned().unwrap_or(found));
            }
        };
        let auto_span = auto_span_in(written);
        // A modifier of the written type may want the *address* of the
        // initialiser -- `&auto` binds a reference to it -- and then the
        // initialiser's own reference must be kept rather than read through.
        let wants_address = matches!(
            written,
            fallc_ast::TypeSyntax::Reference(_)
                | fallc_ast::TypeSyntax::ConstReference(_)
                | fallc_ast::TypeSyntax::RvalueReference(_)
                | fallc_ast::TypeSyntax::ConstRvalueReference(_)
                | fallc_ast::TypeSyntax::Pointer(_)
                | fallc_ast::TypeSyntax::PointerToConst(_)
                | fallc_ast::TypeSyntax::ConstPointer(_)
        );
        let found = self.auto_initialiser(ast, resolutions, types, node, Some(auto_span), wants_address)?;
        let init_span = match &node.init {
            Some(fallc_ast::Initializer::Expr(expr)) => ast.expr(*expr).span,
            _ => node.name_span,
        };
        self.deduce_from(written, &found, auto_span, init_span)
    }

    /// Types a binding's initialiser for a deduction, reporting the three
    /// "nothing to deduce from" failures.
    ///
    /// The answer is the initialiser's type as it stands -- a reference included
    /// -- because the modifiers are matched against it; the *value* type is
    /// taken by the `auto` wildcard itself, where a reference reads through.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn auto_initialiser(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &fallc_ast::BindStmt,
        auto_span: Option<Span>,
        wants_address: bool,
    ) -> Option<Type> {
        match &node.init {
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "cannot deduce a type for `{}`: `auto` needs an initialiser",
                        node.name
                    ))
                    .with_code(DiagnosticCode::sema(20))
                    .with_span(auto_span.unwrap_or(node.name_span))
                    .with_note("write a type instead, or initialise the variable"),
                );
                None
            }
            Some(fallc_ast::Initializer::Designated(designated)) => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "cannot deduce a type for `{}` from a designated initialiser",
                        node.name
                    ))
                    .with_code(DiagnosticCode::sema(20))
                    .with_span(designated.span)
                    .with_note(
                        "`auto` needs an initialiser with a type of its own; write the type \
                         explicitly",
                    ),
                );
                None
            }
            Some(fallc_ast::Initializer::Expr(expr)) => {
                // The initialiser is typed before the binding is recorded. With
                // no expectation an `x auto = 7` is an `int`; when the written
                // type asks for an address, the expectation says so, and a
                // reference initialiser stays a reference instead of being read
                // as a value.
                // The ambient expectation is *kept* when the deduction wants a
                // value: a literal takes the surrounding context's type, which
                // is what makes `fn f() u8 { x auto = 7; }` bind a `u8`. When an
                // address is wanted, the expectation says so instead.
                let previous = wants_address.then(|| {
                    self.expected.replace(Type::Pointer {
                        pointee: Box::new(Type::unspecified("", ast.expr(*expr).span)),
                        pointee_is_const: false,
                        pointer_is_const: false,
                    })
                });
                let inferred = self.infer_expr(ast, resolutions, types, *expr);
                if let Some(previous) = previous {
                    self.expected = previous;
                }
                inferred
            }
        }
    }

    /// Matches a written type against the type an initialiser has.
    ///
    /// Every constructor in the written type takes one level off the type
    /// found, and `auto` takes what is left. A mismatch is reported against the
    /// `auto`, because that is the part the author expected to be deduced.
    pub(super) fn deduce_from(
        &mut self,
        written: &fallc_ast::TypeSyntax,
        found: &Type,
        span: Span,
        init_span: Span,
    ) -> Option<Type> {
        use fallc_ast::TypeSyntax;
        let deduced = match written {
            // `type(auto)` keeps the initialiser's own type, reference and
            // all: that is the whole difference from `auto`, which reads
            // through a reference to the value it names.
            TypeSyntax::DecltypeAuto(_) => found.clone(),
            // A written function type has no wildcard for `auto` to fill: the
            // modifier is matched one indirection at a time from the outside,
            // and a function type is not an indirection.
            TypeSyntax::Function(_) => return None,
            // The wildcard: the *value* the initialiser names, so a reference
            // reads through. Recorded, because the expression's own type is
            // still the reference and code generation has to read it.
            TypeSyntax::Auto(_) => match found.referent().cloned() {
                Some(referent) => {
                    // The initialiser names storage, and a value is what is
                    // wanted here, so the read is recorded where generation
                    // looks for it -- at the initialiser's own expression.
                    self.deref_reads.insert(init_span, referent.clone());
                    referent
                }
                None => found.clone(),
            },
            TypeSyntax::Const(inner) => {
                Type::Const(Box::new(self.deduce_from(inner, found, span, init_span)?))
            }
            TypeSyntax::Volatile(inner) => {
                Type::Volatile(Box::new(self.deduce_from(inner, found, span, init_span)?))
            }
            TypeSyntax::Reference(inner) | TypeSyntax::ConstReference(inner) => {
                let referent = found.referent().cloned().unwrap_or_else(|| found.clone());
                Type::Reference {
                    referent: Box::new(self.deduce_from(inner, &referent, span, init_span)?),
                    referent_is_const: matches!(written, TypeSyntax::ConstReference(_)),
                    is_rvalue: false,
                }
            }
            TypeSyntax::RvalueReference(inner) | TypeSyntax::ConstRvalueReference(inner) => {
                let referent = found.referent().cloned().unwrap_or_else(|| found.clone());
                Type::Reference {
                    referent: Box::new(self.deduce_from(inner, &referent, span, init_span)?),
                    referent_is_const: matches!(written, TypeSyntax::ConstRvalueReference(_)),
                    is_rvalue: true,
                }
            }
            TypeSyntax::Pointer(inner)
            | TypeSyntax::PointerToConst(inner)
            | TypeSyntax::ConstPointer(inner) => {
                // A pointer is paired with the pointer or reference the
                // initialiser has; a value would need an address taken, which
                // the language does not do outside a parameter.
                let target = found
                    .referent()
                    .cloned()
                    .or_else(|| found.pointee().cloned())
                    .unwrap_or_else(|| found.clone());
                Type::Pointer {
                    pointee: Box::new(self.deduce_from(inner, &target, span, init_span)?),
                    pointee_is_const: matches!(written, TypeSyntax::PointerToConst(_)),
                    pointer_is_const: matches!(written, TypeSyntax::ConstPointer(_)),
                }
            }
            TypeSyntax::Array(array) => {
                let Some(element) = self.array_element_of(found) else {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`[N]auto` cannot be deduced from `{}`, which is not an array",
                            found.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(20))
                        .with_span(span)
                        .with_note("the initialiser of an array binding is an array"),
                    );
                    return None;
                };
                let Some(len) = self.const_eval_int(array.len).and_then(|len| u64::try_from(len).ok())
                else {
                    self.diagnostics.push(
                        Diagnostic::error("an array length must be a constant".to_owned())
                            .with_code(DiagnosticCode::sema(40))
                            .with_span(span),
                    );
                    return None;
                };
                Type::Array {
                    element: Box::new(self.deduce_from(&array.element, &element, span, init_span)?),
                    len,
                }
            }
            // A written name next to an `auto` is not a deduction: the hole is
            // what is being filled.
            TypeSyntax::Named(_) | TypeSyntax::TypeOf(_) | TypeSyntax::Member(_)
            | TypeSyntax::UnsizedArray(_) | TypeSyntax::Unresolved(_) => found.clone(),
        };
        Some(deduced)
    }

    /// Returns the length expression and element type of an array or a pointer
    /// to one, for an `[]auto` deduction.
    pub(super) fn array_element_of(&self, found: &Type) -> Option<Type> {
        match found {
            Type::Array { element, .. } => Some(element.as_ref().clone()),
            _ => None,
        }
    }

    /// Checks a variable binding.
    ///
    /// # The rules
    ///
    /// A written type is used as-is. `auto` takes the initialiser's type, so it
    /// is an error when there is no initialiser, or when the initialiser has no
    /// type of its own — which is exactly the case for a designated
    /// initialiser, since `.{ .. }` needs a type to be interpreted as anything.
    ///
    /// Both failures are reported with the same code, because they are the same
    /// failure: `auto` with nothing to deduce from.
    pub(super) fn check_bind(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &fallc_ast::BindStmt,
    ) {
        // The initialiser is checked before the binding is recorded, so that a
        // literal can adopt the type the binding requires.
        let declared = match &node.ty {
            // `auto`, bare or under a modifier -- `&auto`, `*auto`, `const
            // auto` -- is a hole in a type, filled from the initialiser. The
            // modifier is matched against the initialiser's type one
            // indirection at a time, which is why `p *auto = &v` is a `*int`.
            fallc_ast::BindingType::Written(written) if mentions_auto(written) => {
                let Some(deduced) = self.deduce_auto_binding(ast, resolutions, types, node) else {
                    return;
                };
                deduced
            }
            fallc_ast::BindingType::Written(written) => {
                self.prepare_type_of(written, ast, resolutions, types);
                self.resolve_written_type(written)
            }
            fallc_ast::BindingType::Auto(_) => {
                let Some(deduced) = self.deduce_auto_binding(ast, resolutions, types, node) else {
                    return;
                };
                deduced
            }
        };

        // A `const` binding must be initialised: there is no way to give it a
        // value afterwards, so an uninitialised one could never be read.
        if node.is_const() && node.init.is_none() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`const` binding `{}` must be initialised",
                    node.name
                ))
                .with_code(DiagnosticCode::sema(41))
                .with_span(node.name_span),
            );
            return;
        }

        // The attributes a binding may carry. The rest of the table belongs to
        // module-level declarations; `@export` is refused here because a local
        // has no name another module could use.
        let constexpr = node.attrs.iter().any(|attr| attr.name == "constexpr");
        for attr in &node.attrs {
            match attr.name.as_str() {
                "inline" | "deprecated" | "align" | "constexpr" | "static"
                | "thread_local" | "maybe_unused" | "uninitialized" => {}
                "export" => self.diagnostics.push(
                    Diagnostic::error("`@export` applies to a module-level declaration, not a local")
                        .with_code(DiagnosticCode::sema(189))
                        .with_span(attr.span)
                        .with_note("a local name belongs to one call frame"),
                ),
                other => self.unknown_attribute(other, attr.span),
            }
        }
        // `@constexpr` promises the value is known before the program runs, so
        // it needs an initialiser and that initialiser must be a constant.
        if constexpr {
            if node.init.is_none() {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`@constexpr` binding `{}` must be initialised",
                        node.name
                    ))
                    .with_code(DiagnosticCode::sema(41))
                    .with_span(node.name_span)
                    .with_note("there is no later assignment that could give it a value"),
                );
                return;
            }
            if let Some(initializer) = &node.init
                && !self.initializer_is_constant(ast, initializer)
            {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "the value of `@constexpr` binding `{}` is not a constant expression",
                        node.name
                    ))
                    .with_code(DiagnosticCode::sema(68))
                    .with_span(match initializer {
                        fallc_ast::Initializer::Expr(expr) => ast.expr(*expr).span,
                        fallc_ast::Initializer::Designated(init) => init.span,
                    })
                    .with_note(
                        "`@constexpr` says the value is known before the program runs; drop \
                         the attribute for a value computed at run time",
                    ),
                );
            }
        }

        self.check_binding_alignment(ast, node, &declared);

        // A static binding outlives the call it is declared in, exactly as a
        // module-level variable outlives `main`, so its initial value is part
        // of the program image too.
        if node.is_static
            && let Some(initializer) = &node.init
            && !self.initializer_is_constant(ast, initializer)
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "the value of `static` binding `{}` is not a constant expression",
                    node.name
                ))
                .with_code(DiagnosticCode::sema(68))
                .with_span(match initializer {
                    fallc_ast::Initializer::Expr(expr) => ast.expr(*expr).span,
                    fallc_ast::Initializer::Designated(init) => init.span,
                })
                .with_note(
                    "a `static` binding has one object for the whole run, so its initial value \
                     has to be known before the run starts",
                ),
            );
        }

        // A value of an incomplete type has no layout to reserve.
        self.require_complete(&declared, node.name_span, "a variable");
        // Record the type so later passes can read it.
        self.binding_types.insert(node.name_span, declared.clone());
        // A binding with no initialiser is *constructed* when its type has a
        // constructor that takes nothing else. Doing nothing would leave the
        // storage as zero bytes, which is the empty value of some types and a
        // state their own `__init__` never produces for others.
        if node.init.is_none()
            && !node.is_uninitialized
            && !declared.is_indirection()
            && !declared.is_unspecified()
            && let Some(symbol) = self.default_constructor_symbol(&declared, resolutions)
        {
            self.init_calls.insert(node.name_span, symbol);
        }
        // Only a `const` binding becomes non-assignable. Recording every
        // binding here made ordinary variables immutable.
        if node.is_const() || constexpr {
            self.const_bindings.insert(node.name_span);
        }
        // A written type is enforced against the initialiser. `auto` needs no
        // check: its declared type *is* the initialiser's type.
        match (&node.ty, &node.init) {
            (fallc_ast::BindingType::Written(_), Some(fallc_ast::Initializer::Expr(expr))) => {
                // `T x = expr;` calls a constructor when the type has one that
                // accepts `expr`, which is what makes `String s = "hi";` work:
                // the initialiser is a C string, not a `String`.
                //
                // An initialiser that is *already* the binding's type is the
                // value, not the constructor's argument -- `Array<int> a =
                // Array<int>();` builds one array and initialises `a` with it,
                // and `Array<int> b = a;` copies. Trying the constructor first
                // sent both to the copy constructor, which for the first meant
                // passing a temporary where the callee wanted the receiver.
                //
                // A constructor is what `T x = y;` means, even when `y` is
                // already a `T`: `String b = a;` has to copy the bytes, and a
                // plain store would leave the two sharing a buffer. That is the
                // contract a type's `__init__(this &T, other T)` states.
                //
                // An initialiser that is itself a construction -- `Array<int>()
                // = ..` -- is the value and not an argument, and is dealt with
                // inside `init_overload_for`, which must not let the copy
                // constructor claim a temporary the caller just made.
                //
                // # `??` on the initialiser
                //
                // A construction written as an initialiser has no call to put
                // `??` on, so the marker goes on the initialiser itself:
                // `copy String = this??;` passes on a raise from the
                // constructor, which is the only thing there that can raise. It
                // is not `??` on `this`, which cannot raise anything.
                let mut argument = *expr;
                let mut propagates = false;
                if let fallc_ast::ExprKind::Propagate(inner) = &ast.expr(*expr).kind {
                    argument = inner.value;
                    propagates = true;
                    self.require_throw_allowed(ast.expr(*expr).span, "`??`");
                }
                if propagates {
                    self.propagating_depth += 1;
                }
                // A reference or a pointer names an address, so there is
                // nothing to construct: the initialiser must already be that
                // address, and `check_binding_value` decides whether it is.
                // Trying the constructor first found `Array`'s copy
                // constructor for `r &Array<int> = *q;` and emitted a
                // construction into an eight-byte slot -- a stack overwrite
                // whose only symptom was a segfault later.
                let constructed = if declared.is_indirection() {
                    None
                } else {
                    self.init_overload_for(&declared, argument, ast, resolutions, types)
                };
                if propagates {
                    self.propagating_depth -= 1;
                }
                if constructed.is_none() {
                    let _ = self.check_binding_value(ast, resolutions, types, *expr, &declared);
                }
            }
            // An `auto` binding is constructed exactly as a written one is.
            // Its type came from the initialiser, but a value of that type is
            // not the bytes of the initialiser: `b auto = a;` has to call
            // `String`'s copy constructor, or the two would share one buffer and
            // free it twice. Only the *type* was deduced; the construction is
            // the same question as before.
            (fallc_ast::BindingType::Auto(_), Some(fallc_ast::Initializer::Expr(expr))) => {
                let mut argument = *expr;
                let mut propagates = false;
                if let fallc_ast::ExprKind::Propagate(inner) = &ast.expr(*expr).kind {
                    argument = inner.value;
                    propagates = true;
                }
                // The constructor search re-infers the argument. `x auto =
                // make()??;` puts the marker on the initialiser, so the inner
                // call is inferred again here without it -- and, before this,
                // "may raise and nothing handles it" was reported for a call the
                // `??` had already protected. The marker covers this second
                // inference too.
                if propagates {
                    self.propagating_depth += 1;
                }
                let constructed = if declared.is_indirection() {
                    None
                } else {
                    self.init_overload_for(&declared, argument, ast, resolutions, types)
                };
                if propagates {
                    self.propagating_depth -= 1;
                }
                if constructed.is_none() {
                    // With no constructor, the initialiser's value is stored as
                    // it stands -- so a reference has to be read through, which
                    // is what makes `x auto = *it;` a copy.
                    let init_span = ast.expr(argument).span;
                    let referent = match types.get(argument) {
                        Some(ty) => ty.referent().cloned(),
                        None => None,
                    };
                    if let Some(referent) = referent {
                        self.deref_reads.insert(init_span, referent);
                    }
                }
            }
            // A designated initialiser is checked against the type it
            // initialises, which is why it cannot be used with `auto`: the
            // elements' types come from the target's fields.
            (
                fallc_ast::BindingType::Written(_),
                Some(fallc_ast::Initializer::Designated(init)),
            ) => {
                self.check_designated_init(ast, resolutions, types, init, &declared);
            }
            _ => {}
        }
    }

    /// Checks a binding's initialiser against the type it declares.
    ///
    /// The inference is the ordinary one, but the *verdict* is not: a reference
    /// parameter accepts the place a value lives in, and a binding is not a
    /// parameter. `r &int = 5;` would otherwise make a reference out of a
    /// number, which is not a reference to anything -- so the comparison is
    /// made with that rule switched off.
    ///
    /// Only the comparison is affected: a method call inside the initialiser
    /// still takes its receiver's address the way it always does.
    pub(super) fn check_binding_value(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: fallc_ast::ExprId,
        declared: &Type,
    ) -> bool {
        let previous = self.expected.replace(declared.clone());
        let inferred = self.infer_expr(ast, resolutions, types, expr);
        self.expected = previous;
        let Some(found) = inferred else {
            return false;
        };
        let was_parameter = self.in_parameter_position;
        self.in_parameter_position = false;
        let mut compatible = self.is_compatible(&found, declared);
        self.in_parameter_position = was_parameter;
        if !compatible && super::methods::integer_promotes(&found, declared) {
            self.adopted_types
                .insert(ast.expr(expr).span, declared.clone());
            compatible = true;
        }
        if !compatible {
            compatible = self.try_implicit_conversion(
                resolutions, ast, types, expr, declared,
            );
        }
        if !compatible {
            self.report_type_mismatch(ast.expr(expr).span, &found, declared);
            return false;
        }
        // A pointer-typed binding wants the *value* the initialiser has, and an
        // expression that names storage reads as that value -- `x *Node = *p;`
        // is the node, not the address of the slot it sits in. Code generation
        // decides whether to read from the same expectation, and a pointer is
        // an address *type*: it suppressed the read and stored the address.
        if declared.is_pointer() && found.is_reference() {
            self.deref_reads.insert(ast.expr(expr).span, declared.clone());
        } else {
            // A value wants the referent: `a int = r;` is the value `r` names,
            // not the address it holds.
            self.note_reference_read(ast, expr, &found, declared);
        }
        true
    }

    /// Checks a `return` against the enclosing function's declared type.
    pub(super) fn check_return(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        stmt: fallc_ast::StmtId,
        node: &fallc_ast::ReturnStmt,
    ) {
        // An `auto` return is deduced here, by the first `return` that has a
        // value: its type, once checked, becomes the function's. Later returns
        // are checked against it, so they have to agree -- the rule `?:` uses
        // for its two branches.
        if let Some(Some((span, keeps_reference))) = self.pending_auto_returns.last().copied()
            && let Some(value) = node.value
        {
            let previous = self.expected.take();
            let found = self.infer_expr(ast, resolutions, types, value);
            self.expected = previous;
            if let Some(found) = found {
                // `auto` is the *value* the expression names, so a reference
                // reads through and the read is recorded for code generation;
                // `type(auto)` is the expression's own type, reference kept.
                let deduced = if keeps_reference {
                    found
                } else {
                    match found.referent().cloned() {
                        Some(referent) => {
                            self.deref_reads
                                .insert(ast.expr(value).span, referent.clone());
                            referent
                        }
                        None => found,
                    }
                };
                if let Some((_, expected)) = self.return_types.last_mut() {
                    *expected = deduced.clone();
                }
                self.type_of_types.insert(span, deduced);
                self.pending_auto_returns.pop();
                self.pending_auto_returns.push(None);
            }
        }
        let Some((_, expected)) = self.return_types.last().cloned() else {
            // A `return` outside a function cannot happen: the parser only
            // produces statements inside a function body.
            return;
        };
        match node.value {
            Some(value) => {
                // `return expr;` is an initialisation position, exactly as `T x
                // = expr;` and an argument are: when the value is not already
                // the return type, the type's `__init__`/`__im_init__` may make
                // it. Without this, a type whose `__im_init__` converts from a
                // literal -- `return 300.0;` in a function returning `Celsius`
                // -- was a type mismatch even though `Celsius c = 300.0;`
                // worked.
                let constructed = if expected.is_indirection()
                    || expected.is_unspecified()
                {
                    None
                } else {
                    self.init_overload_for(&expected, value, ast, resolutions, types)
                };
                if constructed.is_none() {
                    let _ = self.check_expr(ast, resolutions, types, value, &expected);
                }
                // A pointer-typed return wants the value, exactly as a
                // pointer-typed binding does: `return *p;` in a function
                // returning `*T` is the pointee, and code generation read it as
                // the address because a pointer looks like an address.
                if expected.is_pointer()
                    && types.get(value).is_some_and(Type::is_reference)
                {
                    self.deref_reads.insert(ast.expr(value).span, expected.clone());
                }
            }
            None => {
                // A bare `return;` in a function that declares a return type.
                if expected.is_numeric() {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "expected a value of type `{}` in this `return`",
                            expected.spelling()
                        ))
                        .with_code(DiagnosticCode::sema(11))
                        .with_span(ast.stmt(stmt).span),
                    );
                }
            }
        }
    }

    /// Records a call that may raise, and reports one that nobody guards.
    ///
    /// A call may raise when its callee says so with `??`. What makes it safe is
    /// being inside a `try` body with an `except`, or carrying a `??` of its own
    /// -- and the second is only available in a function that may raise.
    pub(super) fn note_throwing_call(
        &mut self,
        span: Span,
        signature: &crate::FunctionSignature,
        ast: &Ast,
        node: Option<&fallc_ast::CallExpr>,
    ) {
        if !signature.throws {
            return;
        }
        // An `except(auto)` callee said nothing; what its body turned out to do
        // is the answer, and a body that cannot raise makes this call ordinary.
        if self.auto_throws_is_false(signature) {
            return;
        }
        // A probe asks what the expression *would* do, and nothing is emitted
        // for it: the answer is the point, not the call.
        if self.probe_depth > 0 {
            self.probe_saw_throw = true;
            return;
        }
        // The result is a tagged union whatever the condition says, so the call
        // is recorded either way; what the condition can lift is the *duty* to
        // protect it.
        self.throwing_calls.insert(span);
        // A `try` that encloses this call is what `??` re-raises into: the
        // marker means `try { call() } except { raise; }`, so its handler is
        // the innermost `except` above it, not the caller of the function.
        // This is checked before `propagating_depth`, because being inside a
        // `try` is what decides where the re-raise lands.
        if self.protected_depth > 0 {
            return;
        }
        if self.propagating_depth > 0 {
            // No handler here: the exception leaves this function.
            self.propagating_calls.insert(span);
            // An `except(auto)` function has no claim of its own yet, so the
            // fact that something can leave it is recorded and becomes the
            // answer.
            if self.throws_auto {
                self.auto_seen_raise = true;
            }
            return;
        }
        // `??(cond)` claims the function raises only when `cond` holds. The
        // condition names the callee's parameters, and the call's arguments are
        // what they are, so it can be *decided* here when those arguments are
        // constants -- `f(-1)` may raise, `f(3)` may not.
        if self.throwing_condition_is_false(signature, ast, node) {
            return;
        }
        self.report_unhandled_raise(span, &signature.written_name);
    }

    /// Records that the operation at `span` may raise, for a callee that has no
    /// signature to consult.
    ///
    /// The rules are [`Self::note_throwing_call`]'s without the `??(cond)`
    /// question. Two callees have no signature to consult: a virtual call of an
    /// interface method, and a call of a lambda. `message` is the diagnostic's
    /// text, and it is the only thing the two do not share.
    pub(super) fn note_raising_site(&mut self, span: Span, message: &str) {
        if self.probe_depth > 0 {
            self.probe_saw_throw = true;
            return;
        }
        self.throwing_calls.insert(span);
        if self.protected_depth > 0 {
            return;
        }
        if self.propagating_depth > 0 {
            self.propagating_calls.insert(span);
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(message.to_owned())
                .with_code(DiagnosticCode::sema(83))
                .with_span(span)
                .with_note(
                    "put it in a `try` with an `except`, write `??` to pass this one on, or \
                     wrap the whole region in `try { .. } except { raise; }`",
                ),
        );
    }

    /// Reports a call that may raise where nothing handles it.
    fn report_unhandled_raise(&mut self, span: Span, label: &str) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "a call to `{label}` may raise, and nothing here handles it"
            ))
            .with_code(DiagnosticCode::sema(83))
            .with_span(span)
            .with_note(
                "put it in a `try` with an `except`, write `??` to pass this one on, or wrap \
                 the whole region in `try { .. } except { raise; }`",
            ),
        );
    }

    /// Returns `true` when a `??(cond)` callee provably cannot raise here.
    ///
    /// Only a condition in *this* tree can be looked at, and only constant
    /// arguments can decide it: `f(n < 0)` with a literal argument is a fact,
    /// and with a variable it is not. Everything undecided is treated as able
    /// to raise, which is the safe direction -- a wrong answer the other way
    /// would let an exception escape through a function that denied raising.
    pub(super) fn throwing_condition_is_false(
        &mut self,
        signature: &crate::FunctionSignature,
        ast: &Ast,
        node: Option<&fallc_ast::CallExpr>,
    ) -> bool {
        let Some(condition) = signature.throws_condition else {
            return false;
        };
        if condition.as_raw() as usize >= ast.expr_count() {
            // The condition belongs to another tree, which this caller cannot
            // read: treat the callee as able to raise.
            return false;
        }
        // The condition was checked where it was written -- its `except(..)`
        // queries became constants there -- so this only has to read the answer.
        // A condition that mentions parameters is folded with their values when
        // the arguments are constant.
        let Some(node) = node else {
            // A call written without argument syntax -- a constructor invoked
            // by an initialiser, say -- has no arguments to fold the claim's
            // parameters with, so the callee is taken at its word.
            return false;
        };
        if !node.args.is_empty() {
            let mut values = Vec::with_capacity(node.args.len());
            for argument in &node.args {
                let Some(value) = self.const_eval_int(*argument) else {
                    return false;
                };
                values.push(value);
            }
            let functions = self.evaluable_functions.clone();
            let constants = self.constants_by_name.clone();
            if let Some(answered) = crate::interp::eval_expression(
                ast,
                condition,
                &signature.param_names,
                &values,
                &functions,
                &constants,
            ) {
                return answered == 0;
            }
        }
        self.const_eval_int(condition) == Some(0)
    }

    /// Reports unless raising is allowed in the function being checked.
    pub(super) fn require_throw_allowed(&mut self, span: Span, what: &str) {
        if self.throws_allowed.last().copied().unwrap_or(false) {
            return;
        }
        // A `try` that encloses the raise is what takes it: the exception
        // never leaves the function, so the function does not have to say it
        // may raise. `??` is the same case -- it re-raises at the marker, and
        // an enclosing `try` catches the re-raise.
        if self.protected_depth > 0 {
            return;
        }
        self.diagnostics.push(
            Diagnostic::error(format!(
                "{what} is only allowed in a function marked `except`"
            ))
            .with_code(DiagnosticCode::sema(82))
            .with_span(span)
            .with_note("a function that can raise says so after its return type"),
        );
    }

}

impl TypeChecker {
    /// Whether an `except(auto)` callee turned out not to raise.
    ///
    /// A call whose callee cannot be seen at all is treated as raising, which
    /// is the safe direction: the caller must handle a failure it cannot rule
    /// out.
    fn auto_throws_is_false(&self, signature: &crate::FunctionSignature) -> bool {
        if !self.except_auto(&signature.written_name) {
            return false;
        }
        !self
            .throwing_functions
            .contains(&signature.written_name)
            && !self
                .throwing_functions
                .contains(signature.written_name.rsplit('.').next().unwrap_or(""))
    }

    /// Whether a declaration was written `except(auto)`.
    fn except_auto(&self, name: &str) -> bool {
        let bare = name.rsplit('.').next().unwrap_or(name);
        self.except_auto_functions.iter().any(|declared| {
            declared == name || declared.rsplit('.').next().unwrap_or(declared) == bare
        })
    }
}
