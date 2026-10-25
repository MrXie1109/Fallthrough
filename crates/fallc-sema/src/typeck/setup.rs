//! Building a checker, and the module-level pass.
//!
//! This is what runs once per file: the constructor, the entry point that walks
//! the items, the warnings that need a whole body to decide, and the tables of
//! declared types, templates and traits that everything else consults.

use super::*;

impl TypeChecker {
    /// Records a module's name, so that a qualified call can be told from a
    /// method call.
    pub fn add_module(&mut self, name: impl Into<String>) {
        self.modules.insert(name.into());
    }

    /// Creates a type checker for a target with this pointer width, in bits.
    ///
    /// The width is a parameter rather than something set afterwards because
    /// `sizeof` of a pointer -- and of every type that contains one -- is that
    /// width. A checker without it answers "not a constant" for any such type,
    /// which is a fact about the checker rather than about the program: it made
    /// `sizeof(T)` of a declared type fail inside every module, because only the
    /// file being compiled had its checker told. Making it required means a new
    /// caller cannot forget it.
    #[must_use]
    pub fn new(pointer_bits: u32) -> Self {
        TypeChecker {
            diagnostics: DiagnosticBag::new(),
            declarations_only: false,
            expected: None,
            in_trait_requirement: false,
            binding_saved_substitution: Vec::new(),
            pending_auto_returns: Vec::new(),
            external_variables: std::collections::HashSet::new(),
            imported_variables: std::collections::HashMap::new(),
            imported_type_names: std::collections::HashSet::new(),
            imported_field_alignments: std::collections::HashMap::new(),
            type_environments: Vec::new(),
            environment_conflicts: std::collections::HashSet::new(),
            local_type_names: std::collections::HashSet::new(),
            local_families: std::collections::HashSet::new(),
            external_global_refs: std::collections::HashMap::new(),
            binding_types: BindingTypes::new(),
            binding_alignments: std::collections::HashMap::new(),
            deprecated_functions: std::collections::HashMap::new(),
            conversion_methods: std::collections::HashMap::new(),
            implicit_conversions: std::collections::HashMap::new(),
            type_of_types: std::collections::HashMap::new(),
            index_methods: std::collections::HashMap::new(),
            call_methods: std::collections::HashMap::new(),
            text_calls: std::collections::HashMap::new(),
            call_symbols: std::collections::HashMap::new(),
            must_use_functions: std::collections::HashSet::new(),
            must_use_calls: std::collections::HashSet::new(),
            templated_methods: std::collections::HashSet::new(),
            lambda_raise_stack: Vec::new(),
            closures: std::collections::HashMap::new(),
            type_specialisations: std::collections::HashMap::new(),
            imported_specialisations: std::collections::HashMap::new(),
            imported_family_constants: std::collections::HashMap::new(),
            interfaces: std::collections::HashMap::new(),
            expected_types: std::collections::HashMap::new(),
            disabled_functions: std::collections::HashSet::new(),
            setup_span: None,
            closure_calls: std::collections::HashMap::new(),
            interface_conversions: std::collections::HashMap::new(),
            interface_calls: std::collections::HashMap::new(),
            lambda_calls: std::collections::HashMap::new(),
            lambda_values: std::collections::HashMap::new(),
            lambda_instances: std::collections::HashMap::new(),
            constructor_calls: std::collections::HashMap::new(),
            constructor_types: std::collections::HashMap::new(),
            instantiation_list: Vec::new(),
            constant_values: std::collections::HashMap::new(),
            by_address_arguments: std::collections::HashSet::new(),
            operator_methods: std::collections::HashMap::new(),
            adopted_types: std::collections::HashMap::new(),
            literal_constructions: std::collections::HashMap::new(),
            literal_construction_types: std::collections::HashMap::new(),
            bool_methods: std::collections::HashMap::new(),
            unary_methods: std::collections::HashMap::new(),
            in_place_methods: std::collections::HashMap::new(),
            in_place_by_address: std::collections::HashMap::new(),
            init_calls: std::collections::HashMap::new(),
            method_calls: std::collections::HashMap::new(),
            throwing_calls: std::collections::HashSet::new(),
            propagating_calls: std::collections::HashSet::new(),
            throws_allowed: Vec::new(),
            protected_depth: 0,
            except_depth: 0,
            propagating_depth: 0,
            probe_depth: 0,
            probe_saw_throw: false,
            assign_methods: std::collections::HashMap::new(),
            deref_reads: std::collections::HashMap::new(),
            pointer_targets: std::collections::HashMap::new(),
            in_place_operations: std::collections::HashMap::new(),
            fallback_resolutions: None,
            modules: std::collections::HashSet::new(),
            nominal_types: std::collections::HashMap::new(),
            auto_base: None,
            resolving_probe: false,
            in_parameter_position: true,
            substitution: None,
            type_templates: std::collections::HashMap::new(),
            type_template_arguments: std::collections::HashMap::new(),
            template_definitions: std::collections::HashMap::new(),
            trait_definitions: std::collections::HashMap::new(),
            module_trees: Vec::new(),
            pending_method_instantiations: Vec::new(),
            instantiation_constants: std::collections::HashMap::new(),
            constants_by_name: std::collections::HashMap::new(),
            compile_time_ifs: std::collections::HashMap::new(),
            pointer_bits,
            aliases: std::collections::HashMap::new(),
            enum_bases: std::collections::HashMap::new(),
            evaluable_functions: std::collections::HashMap::new(),
            constant_types_by_name: std::collections::HashMap::new(),
            template_value_types: std::collections::HashMap::new(),
            instantiations: std::collections::HashMap::new(),
            instantiated_returns: std::collections::HashMap::new(),
            resolving_nested_type: false,
            const_bindings: std::collections::HashSet::new(),
            default_values: std::collections::HashMap::new(),
            default_arguments: std::collections::HashMap::new(),
            default_expressions: std::collections::HashMap::new(),
            const_exprs: Vec::new(),
            constants: std::collections::HashMap::new(),
            const_expr_spans: Vec::new(),
            literal_suffixes: Vec::new(),
            const_index_tree: None,
            return_types: Vec::new(),
        }
    }

    /// Runs type checking over `ast`.
    /// Records constants an import brought in, so `C.EOF` has a value.
    pub fn add_imported_constants(
        &mut self,
        constants: std::collections::HashMap<String, i64>,
    ) {
        self.constants_by_name.extend(constants);
    }

    /// Records the `extern` variables imported modules exported.
    ///
    /// Keyed by the qualified name a program writes -- `C.stdin` -- because that
    /// is what a field access on a module means, and what a bare imported name
    /// resolves to through its definition.
    pub fn add_imported_variables(&mut self, variables: std::collections::HashMap<String, Type>) {
        self.imported_variables.extend(variables);
    }

    /// Sets whether this check is declaration-only.
    ///
    /// A declaration-only check resolves types and signatures, folds constants
    /// and defaults, and evaluates `@enable_if`; it does not enter function
    /// bodies or `__setup__` blocks. The module's object file holds the checked
    /// implementation, built when that object was compiled.
    pub fn set_declarations_only(&mut self, declarations_only: bool) {
        self.declarations_only = declarations_only;
    }

    pub fn check(&mut self, ast: &Ast, resolutions: &Resolutions) -> TypeCheckResult {
        let mut types = TypeTable::new();
        // This file's definitions are the fallback for every name a template
        // body's own module table cannot answer: the body was written against
        // its module, but the types it is instantiated with are this file's.
        self.fallback_resolutions = Some(resolutions.clone());
        // Array lengths are folded before checking, because the checker needs a
        // length to build the array type and the length expression has no type
        // context of its own.
        self.index_constants(ast);
        // Declared types are collected before anything is checked, so that a
        // function may use a type declared after it. Declarations are
        // order-independent in every language that has them, and collecting
        // first is also what lets one type refer to another.
        self.collect_type_declarations(ast);
        // Every function with a body is a candidate for compile-time
        // evaluation. Collected before any body is checked, so that a constant
        // may call a function declared later in the file.
        //
        // One tree is shared by every entry: cloning it per function made each
        // entry carry a copy of the whole file, which is quadratic in the number
        // of functions and was the reason a file of 600 tiny functions needed
        // most of a gigabyte.
        let evaluable_ast = std::rc::Rc::new(ast.clone());
        for &item in &ast.items {
            if let fallc_ast::ItemKind::Function(function) = &ast.item(item).kind {
                // A template is not one function but a family, and which member
                // a call means is decided during checking -- so it is not
                // evaluable here. A method needs a receiver, which is storage.
                if !function.template_params.is_empty() || function.body.is_none() || function.has_pack() {
                    continue;
                }
                if function
                    .params
                    .first()
                    .is_some_and(|param| param.name == "this")
                {
                    continue;
                }
                let Some(body) = function.body else {
                    continue;
                };
                self.evaluable_functions.insert(
                    function.name.clone(),
                    crate::interp::Evaluable {
                        ast: std::rc::Rc::clone(&evaluable_ast),
                        body,
                        params: function.params.iter().map(|p| p.name.clone()).collect(),
                        defaults: function.params.iter().map(|p| p.default).collect(),
                        is_constexpr: function.is_constexpr,
                    },
                );
            }
        }
        // A declaration from another module may be deprecated. The attribute
        // is part of what it publishes, so a use here says so as well -- under
        // the qualified name and under the last segment, which are the two
        // spellings a call site writes.
        for def in &resolutions.defs {
            let Some(message) = def
                .signature
                .as_ref()
                .and_then(|signature| signature.deprecated.clone())
            else {
                continue;
            };
            let entry = if message.is_empty() {
                None
            } else {
                Some(message)
            };
            self.deprecated_functions
                .entry(def.name.clone())
                .or_insert_with(|| entry.clone());
            if let Some((_, last)) = def.name.rsplit_once('.') {
                self.deprecated_functions
                    .entry(last.to_owned())
                    .or_insert_with(|| entry.clone());
            }
        }
        self.check_items(ast, resolutions, &mut types);
        // A parameter default is an expression in this tree, and a call from
        // another module has no way to read it. What *can* cross is the value,
        // so the ones the compiler can work out are folded here -- after the
        // items, because a default may name a constant or an enumerator that
        // is declared later in the file.
        self.fold_default_values(ast, resolutions, &mut types);
        // An interface is satisfied structurally, which needs the resolution
        // table: the expectations were recorded while checking and are settled
        // here, in one pass.
        self.resolve_interface_conversions(resolutions, &types);
        // A templated type's destructor is instantiated for every member that
        // exists, before the methods are checked.
        self.instantiate_member_destructors(resolutions);
        // A templated type's methods are checked now, once every member that
        // was asked for is known. They could not be checked while the call that
        // asked for them was being resolved: the call is resolved *by* them.
        self.check_pending_method_instantiations(resolutions, &mut types);
        // Reported last, so that it is a remark about a program that is
        // otherwise understood: a binding nothing reads is not an error, and a
        // program with errors has more urgent things to say.
        self.warn_unused_bindings(ast, resolutions);
        self.warn_use_after_move(ast);
        let declarations = self.nominal_types.values().cloned().collect();
        // The tables this module's own templates were written against. Taken
        // here, before anything is moved out, so that a body instantiated in
        // another file resolves its names in this module.
        let environment = TypeEnvironment {
            module: String::new(),
            nominal_types: self.nominal_types.clone(),
            type_templates: self.type_templates.clone(),
            aliases: self.aliases.clone(),
            enum_bases: self.enum_bases.clone(),
            type_specialisations: self.type_specialisations.clone(),
            interfaces: self.interfaces.clone(),
            modules: self.modules.clone(),
        };
        TypeCheckResult {
            environment,
            type_of_types: std::mem::take(&mut self.type_of_types),
            conversions: {
                // Explicit `as` conversions and implicit `__im_as__` ones are
                // the same call as far as code generation is concerned; both
                // are keyed by the source expression's span.
                let mut conversions = std::mem::take(&mut self.conversion_methods);
                conversions.extend(std::mem::take(&mut self.implicit_conversions));
                conversions
            },
            index_methods: std::mem::take(&mut self.index_methods),
            call_methods: std::mem::take(&mut self.call_methods),
            text_calls: std::mem::take(&mut self.text_calls),
            call_symbols: std::mem::take(&mut self.call_symbols),
            templated_methods: std::mem::take(&mut self.templated_methods),
            disabled_functions: std::mem::take(&mut self.disabled_functions),
            closure_calls: std::mem::take(&mut self.closure_calls),
            interface_conversions: std::mem::take(&mut self.interface_conversions),
            interface_calls: std::mem::take(&mut self.interface_calls),
            lambda_calls: std::mem::take(&mut self.lambda_calls),
            lambda_values: std::mem::take(&mut self.lambda_values),
            constructor_calls: std::mem::take(&mut self.constructor_calls),
            constructor_types: std::mem::take(&mut self.constructor_types),
            instantiations: std::mem::take(&mut self.instantiation_list)
                .into_iter()
                .map(|instantiation| (instantiation.key.clone(), instantiation))
                .collect(),
            constant_values: std::mem::take(&mut self.constant_values),
            default_values: std::mem::take(&mut self.default_values),
            default_arguments: std::mem::take(&mut self.default_arguments),
            default_expressions: std::mem::take(&mut self.default_expressions),
            compile_time_ifs: std::mem::take(&mut self.compile_time_ifs),
            aliases: self.aliases.clone(),
            constants_by_name: std::mem::take(&mut self.constants_by_name),
            constant_types_by_name: std::mem::take(&mut self.constant_types_by_name),
            binding_alignments: std::mem::take(&mut self.binding_alignments),
            by_address_arguments: std::mem::take(&mut self.by_address_arguments),
            operator_methods: std::mem::take(&mut self.operator_methods),
            adopted_types: std::mem::take(&mut self.adopted_types),
            literal_constructions: std::mem::take(&mut self.literal_constructions),
            literal_construction_types: std::mem::take(&mut self.literal_construction_types),
            bool_methods: std::mem::take(&mut self.bool_methods),
            unary_methods: std::mem::take(&mut self.unary_methods),
            in_place_methods: std::mem::take(&mut self.in_place_methods),
            in_place_by_address: std::mem::take(&mut self.in_place_by_address),
            init_calls: std::mem::take(&mut self.init_calls),
            method_calls: std::mem::take(&mut self.method_calls),
            throwing_calls: std::mem::take(&mut self.throwing_calls),
            propagating_calls: std::mem::take(&mut self.propagating_calls),
            assign_methods: std::mem::take(&mut self.assign_methods),
            deref_reads: std::mem::take(&mut self.deref_reads),
            pointer_targets: std::mem::take(&mut self.pointer_targets),
            in_place_operations: std::mem::take(&mut self.in_place_operations),
            types,
            types_declarations: declarations,
            binding_types: std::mem::take(&mut self.binding_types),
            external_global_refs: std::mem::take(&mut self.external_global_refs),
            diagnostics: std::mem::take(&mut self.diagnostics).into_vec(),
        }
    }

    /// Warns when a value is read after it was moved from.
    ///
    /// A moved-from value is not broken -- it is empty, and reading it is legal
    /// -- which is exactly why the mistake is quiet: the string that should
    /// hold text prints nothing, and the array that should have elements reports
    /// zero. The pass is deliberately *local*: it follows one block's
    /// statements in order, and a move inside a nested block does not count as a
    /// move afterwards, because the block may not run. That misses cases and
    /// invents none, which is the right direction for a warning.
    pub(super) fn warn_use_after_move(&mut self, ast: &Ast) {
        for &item in &ast.items {
            let fallc_ast::ItemKind::Function(function) = &ast.item(item).kind else {
                continue;
            };
            let Some(body) = function.body else {
                continue;
            };
            let mut moved = std::collections::HashSet::new();
            self.scan_block_for_moves(ast, body, &mut moved);
        }
    }

    /// Walks one block's statements, following which names are empty.
    pub(super) fn scan_block_for_moves(
        &mut self,
        ast: &Ast,
        block: fallc_ast::BlockId,
        moved: &mut std::collections::HashSet<String>,
    ) {
        for &stmt in &ast.block(block).statements.clone() {
            self.scan_stmt_for_moves(ast, stmt, moved);
        }
    }

    /// Walks one statement, warning about reads of names it already moved.
    pub(super) fn scan_stmt_for_moves(
        &mut self,
        ast: &Ast,
        stmt: fallc_ast::StmtId,
        moved: &mut std::collections::HashSet<String>,
    ) {
        let node = ast.stmt(stmt);
        match &node.kind {
            // A block owns its own moves: whether it runs is not known here, so
            // what it empties stays emptied inside it and nowhere else.
            fallc_ast::StmtKind::Block(inner) => {
                let mut scoped = moved.clone();
                self.scan_block_for_moves(ast, *inner, &mut scoped);
            }
            fallc_ast::StmtKind::If(node) => {
                self.scan_expr_for_moves(ast, node.condition, moved);
                let mut then_scope = moved.clone();
                self.scan_block_for_moves(ast, node.then_block, &mut then_scope);
                if let Some(else_branch) = &node.else_branch {
                    let mut else_scope = moved.clone();
                    match else_branch {
                        fallc_ast::ElseBranch::Block(block) => {
                            self.scan_block_for_moves(ast, *block, &mut else_scope);
                        }
                        fallc_ast::ElseBranch::If(nested) => {
                            self.scan_stmt_for_moves(ast, *nested, &mut else_scope);
                        }
                    }
                }
            }
            fallc_ast::StmtKind::While(node) => {
                self.scan_expr_for_moves(ast, node.condition, moved);
                let mut scoped = moved.clone();
                self.scan_block_for_moves(ast, node.body, &mut scoped);
                if let Some(block) = node.else_block {
                    let mut scoped = moved.clone();
                    self.scan_block_for_moves(ast, block, &mut scoped);
                }
            }
            fallc_ast::StmtKind::ForC(node) => {
                let mut scoped = moved.clone();
                if let Some(init) = node.init {
                    self.scan_stmt_for_moves(ast, init, &mut scoped);
                }
                if let Some(condition) = node.condition {
                    self.scan_expr_for_moves(ast, condition, &mut scoped);
                }
                if let Some(update) = node.update {
                    self.scan_expr_for_moves(ast, update, &mut scoped);
                }
                self.scan_block_for_moves(ast, node.body, &mut scoped);
                if let Some(block) = node.else_block {
                    self.scan_block_for_moves(ast, block, &mut scoped);
                }
            }
            fallc_ast::StmtKind::Switch(node) => {
                self.scan_expr_for_moves(ast, node.scrutinee, moved);
                for arm in &node.arms {
                    let mut scoped = moved.clone();
                    for &label in arm.labels.iter().flatten() {
                        self.scan_expr_for_moves(ast, label, &mut scoped);
                    }
                    for &inner in &arm.body {
                        self.scan_stmt_for_moves(ast, inner, &mut scoped);
                    }
                }
            }
            fallc_ast::StmtKind::Try(node) => {
                let mut body_scope = moved.clone();
                self.scan_block_for_moves(ast, node.body, &mut body_scope);
                for clause in &node.except_clauses {
                    let mut scoped = moved.clone();
                    self.scan_block_for_moves(ast, clause.block, &mut scoped);
                }
                for block in [node.else_block, node.finally_block]
                    .into_iter()
                    .flatten()
                {
                    let mut scoped = moved.clone();
                    self.scan_block_for_moves(ast, block, &mut scoped);
                }
            }
            // A declaration starts a new value, so a name that was empty is not
            // any more -- and its initialiser is read in the old world.
            fallc_ast::StmtKind::Bind(node) => {
                if let Some(init) = &node.init {
                    match init {
                        fallc_ast::Initializer::Expr(expr) => {
                            self.scan_expr_for_moves(ast, *expr, moved);
                        }
                        fallc_ast::Initializer::Designated(designated) => {
                            for &element in &designated.elements {
                                self.scan_expr_for_moves(ast, element, moved);
                            }
                        }
                    }
                }
                moved.remove(&node.name);
            }
            // Everything else holds expressions and no blocks.
            _ => {
                let mut scan = MoveScan::new();
                fallc_ast::visit::walk_stmt(ast, stmt, &mut scan);
                let scan = scan.finish();
                self.apply_move_scan(ast, scan, moved);
            }
        }
    }

    /// Walks one expression, applying the same rule.
    pub(super) fn scan_expr_for_moves(
        &mut self,
        ast: &Ast,
        expr: fallc_ast::ExprId,
        moved: &mut std::collections::HashSet<String>,
    ) {
        let mut scan = MoveScan::new();
        fallc_ast::visit::walk_expr(ast, expr, &mut scan);
        let scan = scan.finish();
        self.apply_move_scan(ast, scan, moved);
    }

    /// Reports the reads a scan found of names that are already empty, then
    /// applies what the scan moved and what it wrote.
    pub(super) fn apply_move_scan(
        &mut self,
        ast: &Ast,
        scan: (Vec<fallc_ast::ExprId>, Vec<fallc_ast::ExprId>, Vec<String>),
        moved: &mut std::collections::HashSet<String>,
    ) {
        let (reads, targets, moves) = scan;
        let mut warned = std::collections::HashSet::new();
        for id in reads {
            // `a = ...` writes `a`, which a moved-from name may do: assignment
            // gives it a value again rather than reading the old one.
            if targets.contains(&id) {
                continue;
            }
            let Some(name) = name_expression(ast, id) else {
                continue;
            };
            if !moved.contains(&name) || !warned.insert(name.clone()) {
                continue;
            }
            self.diagnostics.push(
                Diagnostic::warning(format!("`{name}` is read after it was moved from"))
                    .with_code(DiagnosticCode::sema(148))
                    .with_warning("use-after-move")
                    .with_span(ast.expr(id).span)
                    .with_note(
                        "a moved-from value is empty rather than broken: the string it held is \
                         now in the other value, and reading this one gives nothing",
                    ),
            );
        }
        for id in targets {
            if let Some(name) = name_expression(ast, id) {
                moved.remove(&name);
            }
        }
        for name in moves {
            moved.insert(name);
        }
    }

    /// Warns about local bindings that nothing reads.
    ///
    /// A local that no occurrence resolves to is never read: the resolver has
    /// recorded every name together with the definition it means, so the
    /// question is a lookup rather than a second traversal. Parameters are left
    /// alone -- a signature is not a mistake -- and a name beginning with `_`
    /// says the same thing the warning would, which is what the convention is
    /// for.
    pub(super) fn warn_unused_bindings(&mut self, ast: &Ast, resolutions: &Resolutions) {
        // A parameter is not always the author's choice: an `extern` function
        // has no body to read its parameters in, and a trait's parameters are
        // the type variables its requirements are written about.
        let mut required = std::collections::HashSet::new();
        for &item in &ast.items {
            match &ast.item(item).kind {
                fallc_ast::ItemKind::Function(function) if function.body.is_none() => {
                    for param in &function.params {
                        required.insert(param.name_span);
                    }
                }
                fallc_ast::ItemKind::Trait(declaration) => {
                    for param in &declaration.params {
                        required.insert(param.name_span);
                    }
                }
                _ => {}
            }
        }
        // `@maybe_unused` is the author saying the value is deliberately not
        // read, the same statement `_` makes in the name. Attribute spans are
        // declaration spans, so the names are collected first and the warning
        // loop compares by span.
        let mut waived = std::collections::HashSet::new();
        for raw in 0..ast.stmt_count() {
            let id = fallc_ast::StmtId::from_raw(raw as u32);
            if let fallc_ast::StmtKind::Bind(bind) = &ast.stmt(id).kind
                && bind
                    .attrs
                    .iter()
                    .any(|attr| attr.name == "maybe_unused")
            {
                waived.insert(bind.name_span);
            }
        }
        for definition in &resolutions.defs {
            let (group, code) = match definition.kind {
                crate::resolve::DefKind::Local => ("unused", 140),
                // A parameter that is never read is a smaller mistake than a
                // local one -- a signature is not always ours to change -- so it
                // is its own group and can be turned off on its own.
                crate::resolve::DefKind::Param => ("unused-parameter", 142),
                _ => continue,
            };
            // `this` is a receiver rather than a parameter anybody chose, and a
            // name beginning with `_` says the same thing the warning would.
            if definition.name == "this"
                || definition.name.starts_with('_')
                || waived.contains(&definition.name_span)
            {
                continue;
            }
            // A module-level variable is not a local that went unread: it may
            // be read by another module or by nothing at all, which is the
            // author's business rather than a slip of the pen. The root scope
            // is where those live, and its id is zero.
            if definition.scope.as_raw() == 0 {
                continue;
            }
            if required.contains(&definition.name_span) {
                continue;
            }
            if resolutions.is_referenced(definition.id) {
                continue;
            }
            // A binding whose type has a destructor is doing its job by
            // existing: `guard Lock = Lock(&m);` is never read, and the unlock
            // is the whole point of the line. Warning about it asked callers to
            // write a name they would then be told is unused for another reason.
            if self.drops_on_death(resolutions, definition) {
                continue;
            }
            let note = if code == 140 {
                "a value nothing reads is either left over or was meant to be used: remove it, \
                 or write `_` in front of the name to say it is deliberate"
            } else {
                "a signature is not always the author's to change: write `_` in front of the \
                 name to say the parameter is deliberately unused"
            };
            self.diagnostics.push(
                Diagnostic::warning(format!("`{}` is never read", definition.name))
                    .with_code(DiagnosticCode::sema(code))
                    .with_warning(group)
                    .with_span(definition.name_span)
                    .with_note(note),
            );
        }
    }

    /// Whether the binding's type runs a destructor, which reads the value for
    /// the author.
    fn drops_on_death(
        &mut self,
        resolutions: &Resolutions,
        definition: &crate::resolve::Def,
    ) -> bool {
        let Some(ty) = self.binding_types.get(definition.name_span).cloned() else {
            return false;
        };
        let Some(name) = ty.nominal_name().map(str::to_owned) else {
            return false;
        };
        let bare = name.rsplit_once('.').map_or(name.as_str(), |(_, last)| last);
        let qualified = format!("{}{bare}", fallc_ast::special::SpecialMethod::Drop.name());
        self.definitions_named(resolutions, &qualified)
            .iter()
            .any(|candidate| {
                candidate
                    .signature
                    .as_ref()
                    .is_some_and(|signature| signature.param_types.len() == 1)
            })
    }

    /// Indexes `ast`'s expressions for folding, and returns the previous index.
    ///
    /// Used before checking a body that came from another module: the ids in it
    /// are that tree's, and reading them against this unit's index found the
    /// wrong expression or none at all -- which is how a family method's
    /// `@enable_if` stopped being a constant once it was declared in a library.
    /// Nothing is done, and `None` is returned, when the index is already the
    /// tree's own.
    pub(super) fn adopt_const_index(&mut self, ast: &Ast) -> Option<ConstIndex> {
        if self.const_index_tree == Some(ast as *const Ast as usize) {
            return None;
        }
        let previous = ConstIndex {
            exprs: std::mem::take(&mut self.const_exprs),
            spans: std::mem::take(&mut self.const_expr_spans),
            suffixes: std::mem::take(&mut self.literal_suffixes),
            tree: self.const_index_tree.take(),
        };
        self.index_constants(ast);
        Some(previous)
    }

    /// Puts the index [`Self::adopt_const_index`] set aside back.
    pub(super) fn restore_const_index(&mut self, previous: Option<ConstIndex>) {
        let Some(previous) = previous else {
            return;
        };
        self.const_exprs = previous.exprs;
        self.const_expr_spans = previous.spans;
        self.literal_suffixes = previous.suffixes;
        self.const_index_tree = previous.tree;
    }

    /// Folds the value of every parameter default the compiler can work out.
    ///
    /// Recorded by the expression's span, which is how the exporter finds it:
    /// the signature it publishes carries the written default, which is a node
    /// of this tree, and the value beside it, which crosses to any module.
    pub(super) fn fold_default_values(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) {
        for &item in &ast.items {
            let fallc_ast::ItemKind::Function(function) = &ast.item(item).kind else {
                continue;
            };
            // A default of a template or of a family's method may name the
            // template's own parameters, which have no meaning here; only its
            // value can travel, and the folder finds it when it can.
            let generic = !function.template_params.is_empty() || function.has_pack()
                || function
                    .params
                    .first()
                    .is_some_and(|parameter| self.mentions_template_parameter_syntax(&parameter.ty));
            for (index, parameter) in function.params.iter().enumerate() {
                let Some(default) = parameter.default else {
                    continue;
                };
                if let Some(value) = self.const_eval_int(default) {
                    self.default_values
                        .insert(ast.expr(default).span, value);
                }
                // A default that is not a constant is an expression of this
                // module's tree, and a call from another module lowers it
                // *here* -- so it is checked here, with the parameters before
                // it in scope, and its types are recorded with this module's
                // table.
                if generic {
                    continue;
                }
                let declared = self.resolve_written_type(&parameter.ty);
                if declared.is_unspecified() {
                    continue;
                }
                let saved_bindings = std::mem::take(&mut self.binding_types);
                for earlier in function.params.iter().take(index) {
                    let ty = self.resolve_written_type(&earlier.ty);
                    self.binding_types.insert(earlier.name_span, ty);
                }
                let previous = self.expected.replace(declared);
                let _ = self.infer_expr(ast, resolutions, types, default);
                self.expected = previous;
                self.binding_types = saved_bindings;
            }
        }
    }

    /// Records every expression in a form the constant folder can read.
    ///
    /// Done as a pre-pass because folding happens while resolving types, which
    /// is before ordinary checking visits those expressions.
    pub(super) fn index_constants(&mut self, ast: &Ast) {
        self.const_index_tree = Some(ast as *const Ast as usize);
        let count = ast.expr_count();
        self.const_exprs = vec![ConstExpr::Unknown; count];
        self.const_expr_spans = vec![fallc_span::Span::SYNTHETIC; count];
        self.literal_suffixes = vec![None; count];

        for index in 0..count {
            let id = ExprId::from_raw(u32::try_from(index).expect("expression count fits in u32"));
            let node = ast.expr(id);
            self.const_expr_spans[index] = node.span;
            self.const_exprs[index] = match &node.kind {
                ExprKind::IntLiteral(literal) => {
                    self.literal_suffixes[index] = literal.suffix.clone();
                    fallc_ast::literal::decode_int_value(&literal.digits)
                        .map_or(ConstExpr::Unknown, ConstExpr::Int)
                }
                // A character literal is a constant too, which is what makes
                // `const ZERO char = '\0';` a constant rather than a value
                // that could only be known at run time.
                ExprKind::CharLiteral(literal) => {
                    fallc_ast::literal::decode_char_code_point(&literal.text)
                        .map_or(ConstExpr::Unknown, |code| ConstExpr::Int(i64::from(code)))
                }
                ExprKind::Unary(unary) => match unary.op {
                    UnaryOp::Neg => ConstExpr::Neg(unary.operand),
                    UnaryOp::BitNot => ConstExpr::Not(unary.operand),
                    UnaryOp::Not => ConstExpr::LogicalNot(unary.operand),
                    _ => ConstExpr::Unknown,
                },
                // A boolean is a constant like any other: `@enable_if(false)`
                // asks to remove a declaration, and the answer is known.
                ExprKind::BoolLiteral(value) => ConstExpr::Int(i64::from(*value)),
                ExprKind::Binary(binary) => match binary.op {
                    // The logical operators are folded lazily, so they get a
                    // node of their own rather than the eager arithmetic one.
                    BinaryOp::LogicalAnd => ConstExpr::Logical {
                        is_and: true,
                        lhs: binary.lhs,
                        rhs: binary.rhs,
                    },
                    BinaryOp::LogicalOr => ConstExpr::Logical {
                        is_and: false,
                        lhs: binary.lhs,
                        rhs: binary.rhs,
                    },
                    // A comparison yields 0 or 1, which is what makes a
                    // compile-time `if` decidable.
                    op @ (BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge) => ConstExpr::Compare {
                        op,
                        lhs: binary.lhs,
                        rhs: binary.rhs,
                    },
                    op => ConstExpr::Binary {
                        op,
                        lhs: binary.lhs,
                        rhs: binary.rhs,
                    },
                },
                ExprKind::Sizeof(ty) => ConstExpr::Sizeof(ty.clone()),
                ExprKind::Alignof(ty) => ConstExpr::Alignof(ty.clone()),
                ExprKind::SizeofPack(pack) => ConstExpr::SizeofPack(pack.clone()),
                ExprKind::Fold(fold) => ConstExpr::Fold(fold.clone()),
                ExprKind::Conditional(node) => ConstExpr::Conditional {
                    condition: node.condition,
                    then_value: node.then_value,
                    else_value: node.else_value,
                },
                // A name may stand for a constant. Whether it does is only
                // known once the definitions are in hand, so it is recorded and
                // looked up when the value is wanted.
                ExprKind::Name(name) => ConstExpr::Named {
                    name: name.clone(),
                    span: node.span,
                },
                // A call may be a constant, if the function it names can be
                // evaluated and its arguments are constants too. Whether that
                // holds is only known once the definitions are in hand.
                // A call with named arguments is not folded: the evaluator
                // binds arguments by position, so `.c = 30` would be filled
                // from the default after it instead. Folding is an
                // optimisation, and an optimisation that changes the answer is
                // worse than none.
                ExprKind::Call(call) => match &ast.expr(call.callee).kind {
                    ExprKind::Name(callee) if call.named.is_empty() => ConstExpr::Call {
                        callee: callee.clone(),
                        args: call.args.clone(),
                    },
                    _ => ConstExpr::Unknown,
                },
                _ => ConstExpr::Unknown,
            };
        }
    }

    /// Folds an enum's members into constants.
    ///
    /// # The value of a member
    ///
    /// `GREEN = 5` says the value; a member with no `=` is one more than the
    /// one before it, and the first is zero. The base type is what the members
    /// *are* -- an `enum Color int` has members of type `int` -- which is what
    /// makes an enum usable where an integer is: `switch (c)` and
    /// `c == Color.RED` need no conversion, and turning one into a plain
    /// integer for arithmetic needs `as`.
    ///
    /// The members are recorded by their qualified name, `Color.RED`, because
    /// that is how the language spells one -- there is no `::`.
    pub(super) fn check_enum(&mut self, declaration: &fallc_ast::EnumDecl) {
        let base = match &declaration.base {
            Some(written) => self.resolve_written_type(written),
            // The base type defaults to `int`, as it does in C.
            None => Type::INT,
        };
        if !base.is_integer() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "an enum's base type must be an integer, but `{}` was given",
                    base.spelling()
                ))
                .with_code(DiagnosticCode::sema(76))
                .with_span(declaration.name_span),
            );
            return;
        }
        self.enum_bases
            .insert(declaration.name.clone(), base.clone());
        // The type is declared for the checker's own lookups, so that a member
        // resolves to it and `Color` may be named as a type.
        self.nominal_types
            .entry(declaration.name.clone())
            .or_insert_with(|| crate::types::NominalType {
                template_arguments: None,
                name: declaration.name.clone(),
                fields: Vec::new(),
                mutable_fields: Vec::new(),
                field_alignments: Vec::new(),
                is_union: false,
                is_incomplete: false,
                union_storage: None,
            });

        let mut next: i64 = 0;
        let mut seen: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for member in &declaration.members {
            let value = match member.value {
                // A written value is a *constant expression*, by the same rule
                // every other constant position uses.
                Some(expr) => {
                    if !self.require_constant(
                        expr,
                        self.const_span(expr),
                        &format!("the value of `{}.{}`", declaration.name, member.name),
                        77,
                    ) {
                        return;
                    }
                    match self.const_eval_int(expr) {
                        Some(value) => value,
                        None => return,
                    }
                }
                None => next,
            };
            // A duplicate is worth reporting: `Color.RED` would otherwise mean
            // whichever one happened to be recorded last.
            if let Some(previous) = seen.insert(member.name.clone(), value) {
                let _ = previous;
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}.{}` is declared more than once",
                        declaration.name, member.name
                    ))
                    .with_code(DiagnosticCode::sema(78))
                    .with_span(member.name_span),
                );
                return;
            }
            self.constants_by_name.insert(
                format!("{}.{}", declaration.name, member.name),
                value,
            );
            // The next member continues from this one, whether the value was
            // written or implied.
            next = value.saturating_add(1);
        }
    }

    /// Records every declared type, before any function body is checked.
    pub(super) fn collect_type_declarations(&mut self, ast: &Ast) {
        // Which type names this file declares itself, so that an imported
        // family of the same name cannot take the name away: a `type Box` in a
        // file that imported `std` is *that* program's `Box`.
        for &item in &ast.items {
            match &ast.item(item).kind {
                fallc_ast::ItemKind::TypeDeclaration(declaration) => {
                    self.local_type_names.insert(declaration.name.clone());
                    if !declaration.template_params.is_empty() {
                        self.local_families.insert(declaration.name.clone());
                    }
                }
                fallc_ast::ItemKind::EnumDeclaration(declaration) => {
                    self.local_type_names.insert(declaration.name.clone());
                }
                fallc_ast::ItemKind::Interface(interface) => {
                    self.local_type_names.insert(interface.name.clone());
                    if !interface.template_params.is_empty() {
                        self.local_families.insert(interface.name.clone());
                    }
                }
                _ => {}
            }
        }
        // An interface's value is a heap copy of a concrete value plus a table
        // of the methods that act on it: two pointers, and nothing else. It is
        // registered as a type so that `IIterator<int>` names that pair.
        for &item in &ast.items {
            let fallc_ast::ItemKind::Interface(interface) = &ast.item(item).kind else {
                continue;
            };
            self.register_interface(interface);
        }
        // A templated type declares no type of its own -- its fields name the
        // very parameters being declared -- so it is remembered and each
        // `Array<int>` is collected in its place, on first use.
        for &item in &ast.items {
            let fallc_ast::ItemKind::TypeDeclaration(declaration) = &ast.item(item).kind else {
                continue;
            };
            // A specialisation is not the family and not a type of its own: it
            // replaces the member its pattern matches. It is remembered here so
            // that the first `Box<*int>` finds it.
            if !declaration.pattern.is_empty() {
                self.type_specialisations
                    .entry(declaration.name.clone())
                    .or_default()
                    .push(Specialisation {
                        params: declaration.template_params.clone(),
                        pattern: declaration.pattern.clone(),
                        declaration: declaration.clone(),
                        span: ast.item(item).span,
                    });
                continue;
            }
            if !declaration.template_params.is_empty() {
                self.type_templates
                    .insert(declaration.name.clone(), declaration.clone());
                continue;
            }
        }
        // Imported types first, so that a local declaration of the same name
        // replaces one rather than being shadowed by it, and so that a type
        // from another module resolves exactly like one declared here.
        for &item in &ast.items {
            // An enum declares a type too -- `Color` may be named wherever a
            // type is -- so it is registered here with no fields. Its members
            // are constants and are folded when the item is checked.
            if let fallc_ast::ItemKind::EnumDeclaration(declaration) = &ast.item(item).kind {
                if declaration.template_params.is_empty() {
                    // An enum *is* its base type, seen through a name that
                    // gives the members meaning. `Color` and `int` are the same
                    // type, which is what lets a `Color` be compared with an
                    // `int`, used as a `switch` scrutinee, and converted with
                    // `as` -- all without a conversion rule of its own.
                    let base = match &declaration.base {
                        Some(written) => self.resolve_written_type(written),
                        None => Type::INT,
                    };
                    self.aliases.insert(declaration.name.clone(), base);
                }
                continue;
            }
            let fallc_ast::ItemKind::TypeDeclaration(declaration) = &ast.item(item).kind else {
                continue;
            };
            if !declaration.template_params.is_empty() || !declaration.pattern.is_empty() {
                continue;
            }
            // An alias introduces no new type: `type Id = usize;` makes `Id`
            // *be* `usize`, so every use of one is a use of the other and
            // nothing is registered under the new name. That is what makes a
            // value of one acceptable where the other is wanted, with no
            // conversion.
            if let fallc_ast::TypeDeclKind::Alias(target) = &declaration.kind {
                let resolved = self.resolve_written_type(target);
                self.aliases.insert(declaration.name.clone(), resolved);
                continue;
            }
            // `type Foo;` declares the name and nothing else. The type is
            // incomplete until a definition completes it, and only a pointer or
            // a reference to it may be written. A declaration that follows a
            // definition changes nothing: the type is already complete.
            if declaration.is_forward {
                self.nominal_types
                    .entry(declaration.name.clone())
                    .or_insert_with(|| crate::types::NominalType {
                        template_arguments: None,
                        name: declaration.name.clone(),
                        fields: Vec::new(),
                        mutable_fields: Vec::new(),
                        field_alignments: Vec::new(),
                        is_union: declaration.kind == fallc_ast::TypeDeclKind::Union,
                        is_incomplete: true,
                        union_storage: None,
                    });
                continue;
            }
            // The name goes in before the fields, so that a field may name
            // the type being declared: `type Node { next *Node; }` is a link,
            // and a type could not mention itself at all while the entry
            // appeared only after its fields were resolved. What the
            // self-reference records is the *name* -- the identity -- and the
            // fields are filled in at the end of this loop.
            self.nominal_types.insert(
                declaration.name.clone(),
                crate::types::NominalType {
                    template_arguments: None,
                    name: declaration.name.clone(),
                    fields: Vec::new(),
                    mutable_fields: Vec::new(),
                    field_alignments: Vec::new(),
                    is_union: declaration.kind == fallc_ast::TypeDeclKind::Union,
                    // The fields are filled in at the end of this loop; until
                    // then the type is being defined, not incomplete.
                    is_incomplete: false,
                    union_storage: None,
                },
            );
            let mut fields = Vec::with_capacity(declaration.fields.len());
            let mut mutable_fields = Vec::new();
            for field in &declaration.fields {
                let ty = self.resolve_written_type(&field.ty);
                // The resolved type is published under the written form's span,
                // because an importer cannot resolve one whose parts need
                // folding -- `[32]byte` has a length that is an expression in
                // this tree, and the importing file has no tree to fold it in.
                self.type_of_types.insert(field.ty.span(), ty.clone());
                // A type cannot contain itself by value: the layout would be
                // its own size, which is not a number. A pointer or a
                // reference is a different matter -- that is what a link is.
                let contains_itself = matches!(
                    ty.unqualified(),
                    Type::Nominal(inner) if inner.name == declaration.name
                );
                if contains_itself {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{}` contains itself by value, so it has no size",
                            declaration.name
                        ))
                        .with_code(DiagnosticCode::sema(85))
                        .with_span(field.ty.span())
                        .with_note(
                            "hold it behind a pointer or a reference: `*Node`, not `Node`",
                        ),
                    );
                }
                fields.push((field.name.clone(), ty));
                if field.mutable {
                    mutable_fields.push(field.name.clone());
                }
            }
            // Read after the fields, because whether an alignment is weaker
            // than the field's own is a question about the resolved type.
            let field_alignments =
                self.collect_field_alignments(Some(ast), &fields, &declaration.fields);
            let is_union = declaration.kind == fallc_ast::TypeDeclKind::Union;
            // The member a union's storage is represented by: the largest, so
            // that every other member still fits in the space. Measured here
            // because only the checker knows a field's size.
            let union_storage = if is_union {
                let mut largest: Option<(usize, i64)> = None;
                for (index, (_, field_ty)) in fields.iter().enumerate() {
                    let size = self.size_of(field_ty).unwrap_or(0);
                    if largest.is_none_or(|(_, best)| size > best) {
                        largest = Some((index, size));
                    }
                }
                largest.map(|(index, _)| index)
            } else {
                None
            };
            self.nominal_types.insert(
                declaration.name.clone(),
                crate::types::NominalType {
                    template_arguments: None,
                    name: declaration.name.clone(),
                    fields,
                    mutable_fields,
                    field_alignments,
                    is_union,
                    is_incomplete: false,
                    union_storage,
                },
            );
            self.register_nested_types(&declaration.name.clone(), declaration);
        }
    }

    /// Records where a templated function's body is.
    ///
    /// The driver hands these over, because a template declared in one module
    /// is instantiated from another and only the driver sees every module's
    /// tree.
    /// Records an imported module's tree, so its templates can be checked.
    pub fn add_module_tree(&mut self, ast: Ast) {
        self.module_trees.push(ast);
    }

    pub fn add_template_definition(
        &mut self,
        name: impl Into<String>,
        body: TemplateBody,
    ) {
        // The tree and the module's own resolutions travel with the function.
        //
        // The tree, because a node id is an index into one tree: the same
        // number names different functions in different translation units, and
        // looking one up by number found `String`'s `push` when `Array`'s
        // `length` was wanted.
        //
        // The resolutions, because a template's body names things its own
        // module imported -- `panic.panic` is a call into a module the
        // instantiating file may never have heard of.
        self.template_definitions.insert(name.into(), body);
    }

    /// Files a trait under a name a program may write for it.
    ///
    /// Called once per name the language makes reachable -- the bare one and
    /// the module-qualified ones -- because a query is written with whichever
    /// of them is in scope.
    pub fn add_trait_definition(&mut self, name: impl Into<String>, body: TraitBody) {
        self.trait_definitions.insert(name.into(), body);
    }

    /// The trait a written name refers to, if this file can see one.
    pub(super) fn trait_body_for(&self, name: &str) -> Option<TraitBody> {
        if let Some(body) = self.trait_definitions.get(name) {
            return Some(body.clone());
        }
        // A trait named with the module it came from is the same trait as the
        // bare name, exactly as `std.String` and `String` are the same type: the
        // qualification says where this file learned the name, not which trait
        // it is. Compared on the last segment for that reason.
        let bare = name.rsplit_once('.').map_or(name, |(_, last)| last);
        let mut candidates: Vec<&String> = self
            .trait_definitions
            .keys()
            .filter(|existing| existing.rsplit_once('.').map_or(existing.as_str(), |(_, last)| last) == bare)
            .collect();
        candidates.sort();
        let name = (*candidates.first()?).clone();
        self.trait_definitions.get(&name).cloned()
    }

    /// Asks a trait whether the supplied types model it.
    ///
    /// The requirements are *checked*, not assumed: each is an expression
    /// written against the trait's parameters, and it holds when it compiles
    /// with the parameters standing for the arguments. Nothing is emitted and
    /// no diagnostic survives -- a requirement that fails is reported by the
    /// caller as one failure of the whole query, because that is what a reader
    /// can act on.
    pub(super) fn trait_models(&mut self, name: &str, arguments: &[Type], span: Span) -> TraitOutcome {
        let _ = span;
        let Some(body) = self.trait_body_for(name) else {
            return TraitOutcome::Unknown;
        };
        let variables = body.declaration.type_variables();
        if variables.len() != arguments.len() {
            return TraitOutcome::Arity {
                name: body.declaration.name.clone(),
                expected: variables.len(),
                found: arguments.len(),
            };
        }

        // The substitution this question is asked under: the caller's, extended
        // with the types it is asked about. The caller's has the *same*
        // variables bound when a requirement asks about another trait with its
        // own parameter (`InputIterator(it) -> true`), and keeping that binding
        // is the point -- extending a fresh one with an unresolved `Iter`
        // overwrote the type the question was about.
        // The caller's bindings are kept only when they say something about
        // *this* question's variables. They usually do not: this template is
        // asked from inside another one, and `Iter` means a different type in
        // each -- which is how `InputIterator(it) -> true` came to ask about
        // `it` itself. They do when the question is a trait asking about its own
        // parameter, and that is the case that must not lose its type.
        let outer = self.substitution.clone();
        let mut bindings = Vec::with_capacity(variables.len());
        for (variable, argument) in variables.iter().zip(arguments) {
            let resolved = match (argument.is_unspecified(), &outer) {
                (true, Some(outer)) => outer.type_of(variable).cloned().unwrap_or_else(|| argument.clone()),
                _ => argument.clone(),
            };
            bindings.push((variable.clone(), resolved));
        }
        self.trait_models_bound(&body, &bindings)
    }

    /// Asks a trait body whether it holds with its variables bound as given.
    ///
    /// The variables are the body's type variables -- the subject's type and an
    /// interface's own parameters -- and the types are what the question is
    /// asked about: the concrete type being boxed, and the arguments the
    /// interface was written with. Boxing answers the same question a query
    /// does, so it goes through the same door rather than a second one.
    pub(super) fn trait_models_bound(
        &mut self,
        body: &TraitBody,
        bindings: &[(String, Type)],
    ) -> TraitOutcome {
        // Everything the requirements are checked under is saved, because a
        // trait may be asked about a type while another one is being asked
        // about -- `InputIterator(it) -> true` inside a trait body is exactly
        // that -- and the inner question binds type variables of its own.
        let saved_substitution = self.substitution.clone();
        let saved_probe = std::mem::replace(&mut self.resolving_probe, true);
        let saved_diagnostics = self.diagnostics.len();

        let saved_binding_saved = self.binding_saved_substitution.clone();
        let mut extended = crate::template::Substitution::default();
        for (variable, argument) in bindings {
            extended.bind_type(variable.clone(), argument.clone());
        }
        self.substitution = Some(extended);

        let outcome = self
            .check_trait_requirements(body)
            .unwrap_or(TraitOutcome::Modelled);

        self.diagnostics.truncate(saved_diagnostics);
        self.substitution = saved_substitution;
        self.binding_saved_substitution = saved_binding_saved;
        self.resolving_probe = saved_probe;
        outcome
    }

    /// Checks one trait body's requirements, in source order.
    ///
    /// The substitution is the caller's, and the body's own parameters are bound
    /// here for the duration of the check, so a requirement sees places of the
    /// types the parameters declare. A requirement that asks about another trait
    /// -- `InputIterator(it) -> true` -- is checked by this same function, under
    /// a substitution of its own.
    ///
    /// # A requirement the compiler can evaluate has to hold
    ///
    /// A requirement is normally "this operation must compile", because an
    /// operation such as `it != it` has no value before the program runs. A
    /// requirement written as a question does have one: `InputIterator(it)` is
    /// folded to `bool` where it stands, and `-> true` says the answer has to be
    /// yes.
    pub(super) fn check_trait_requirements(&mut self, body: &TraitBody) -> Option<TraitOutcome> {
        let saved_params = self.bind_trait_params(body);

        let outcome = (|| {

            for requirement in &body.declaration.requirements {
                let diagnostics_before = self.diagnostics.len();
                let mut requirement_types = crate::types::TypeTable::new();
                let saved_requirement = std::mem::replace(&mut self.in_trait_requirement, true);
                // A requirement is checked in a table of its own and *read* in
                // the same one -- `found` above -- because the ids it uses are
                // its tree's, and one tree's ids are not another's. Keeping the
                // table around meant a requirement checked later read whatever
                // this unit had at those numbers: an `it` of the wrong type,
                // and a `++it` that "cannot be applied" to it.
                let found = self.infer_expr(
                    &body.tree,
                    &body.resolutions,
                    &mut requirement_types,
                    requirement.expr,
                );
                self.in_trait_requirement = saved_requirement;
                // The type the *requirement's* table gave, which `found` was
                // computed from; see the note above.
                // A requirement says something about the expression's result:
                // that it exists and is not `void`, that it has a type, or that
                // it has a value. A requirement that says nothing only has to
                // compile.
                //
                // The value is read from the *span*-keyed table rather than
                // from the constant folder, because the requirement is written
                // in the trait's tree: an expression id is an index into one
                // tree, and asking the folder about another tree's id answers a
                // question about whatever this unit has at that number. What is
                // in that table is what folded where it stood.
                let folded = self
                    .constant_values
                    .get(&body.tree.expr(requirement.expr).span)
                    .copied();
                let holds = match (&requirement.result, found.as_ref()) {
                    (None, Some(_)) => true,
                    (None, None) => false,
                    (Some(fallc_ast::TraitResult::NotVoid(_)), Some(ty)) => !ty.is_void(),
                    (Some(fallc_ast::TraitResult::NotVoid(_)), None) => false,
                    (Some(fallc_ast::TraitResult::Exact(written)), Some(ty)) => {
                        // The written type is resolved under the question's
                        // substitution: a restriction of a generic interface
                        // says `-> T`, and `T` is the type the box holds, not
                        // a name that means nothing here.
                        let substitution = self.substitution.clone().unwrap_or_default();
                        let expected = self.resolve_template_type(written, &substitution);
                        self.is_compatible(ty, &expected)
                    }
                    (Some(fallc_ast::TraitResult::Exact(_)), None) => false,
                    // `-> true` asserts the folded value: the expression is a
                    // question the author knows the answer to.
                    (Some(fallc_ast::TraitResult::Value(wanted)), Some(_)) => {
                        folded == Some(wanted.as_int())
                    }
                    (Some(fallc_ast::TraitResult::Value(_)), None) => false,
                };
                // A requirement holds when the operation *compiles*, so a
                // reported error is a failure even if the expression still has
                // a type. `it.push(0)` where the method takes a `c8` is exactly
                // that: the call yields an `int`, and the argument is wrong.
                // The diagnostics of the question are dropped when it is
                // answered, so this is the only place their meaning survives.
                let reported = self
                    .diagnostics
                    .iter()
                    .skip(diagnostics_before)
                    .find(|diagnostic| diagnostic.severity.is_fatal())
                    .map(|diagnostic| diagnostic.message.clone());
                let holds = holds && reported.is_none();
                if !holds {
                    // The requirement's own diagnostic says *why* -- "cannot
                    // find `begin` in this scope" -- and it is worth keeping as
                    // a note rather than throwing it away. A requirement about a
                    // value has no diagnostic of its own, so it says so in its
                    // place.
                    let why = match (&requirement.result, folded) {
                        (Some(fallc_ast::TraitResult::Value(wanted)), Some(value)) => {
                            Some(format!("the value is `{value}`, not `{}`", wanted.as_int()))
                        }
                        (Some(fallc_ast::TraitResult::Value(_)), None) => {
                            Some("the expression has no value at compile time".to_owned())
                        }
                        _ => reported,
                    };
                    return Some(TraitOutcome::NotModelled {
                        requirement: requirement.text.clone(),
                        why,
                    });
                }
            }
            None
        })();

        self.unbind_trait_params(saved_params);
        outcome
    }

    /// Binds a trait body's parameters as places of the substituted types.
    ///
    /// Returns what was there before, for [`Self::unbind_trait_params`]: a
    /// trait is checked on demand, so a parameter has no type outside a query.
    pub(super) fn bind_trait_params(&mut self, body: &TraitBody) -> Vec<(Span, Option<crate::types::Type>)> {
        let saved: Vec<(Span, Option<crate::types::Type>)> = body
            .declaration
            .params
            .iter()
            .map(|param| (param.name_span, self.binding_types.get(param.name_span).cloned()))
            .collect();
        // The type variable binding: a trait's parameter is written against a
        // variable (`it Iter`), and the substitution holds the type the trait
        // was asked about. Without it, `InputIterator(it) -> true` -- a question
        // written about the same parameter -- resolved `Iter` as an unknown name
        // and answered "no" however the type stood in the hierarchy.
        let variables = body.declaration.type_variables();
        let parameters: Vec<fallc_ast::TemplateParam> = variables
            .iter()
            .map(|variable| fallc_ast::TemplateParam {
                kind: fallc_ast::TemplateParamKind::Type,
                name: variable.clone(),
                name_span: fallc_span::Span::SYNTHETIC,
                is_pack: false,
                default: None,
                span: fallc_span::Span::SYNTHETIC,
            })
            .collect();
        let arguments = crate::template::Arguments(
            variables
                .iter()
                .map(|variable| match self.substitution.as_ref().and_then(|sub| sub.type_of(variable)) {
                    Some(ty) => crate::template::TemplateArgument::Type(ty.clone()),
                    None => crate::template::TemplateArgument::Type(Type::unspecified(
                        variable.clone(),
                        fallc_span::Span::SYNTHETIC,
                    )),
                })
                .collect(),
        );
        let trait_substitution = crate::template::Substitution::new(&parameters, &arguments);
        let saved_substitution = std::mem::replace(&mut self.substitution, trait_substitution);
        for param in &body.declaration.params {
            let ty = match self.substitution.clone() {
                Some(substitution) => self.resolve_template_type(&param.ty, &substitution),
                None => self.resolve_type_syntax(&param.ty, false),
            };
            self.binding_types.insert(param.name_span, ty);
        }
        self.binding_saved_substitution.push(saved_substitution);
        saved
    }

    /// Puts a trait body's parameters back the way they were.
    pub(super) fn unbind_trait_params(&mut self, saved: Vec<(Span, Option<crate::types::Type>)>) {
        self.substitution = self.binding_saved_substitution.pop().unwrap_or(None);
        for (param, previous) in saved {
            match previous {
                Some(ty) => {
                    self.binding_types.insert(param, ty);
                }
                None => {
                    self.binding_types.remove(param);
                }
            }
        }
    }

    /// Returns the type a name was declared with, if it is a name the checker
    /// has a binding for.
    ///
    /// Used by `type(expr)` inside a trait requirement, where `it` is a place
    /// whose *declared* type is the type variable's argument. Reading the name
    /// through the ordinary expression path answers with a reference to the
    /// place, which is right for a value and wrong for a question.
    pub(super) fn declared_type_of_name(
        &self,
        ast: &Ast,
        expr: fallc_ast::ExprId,
    ) -> Option<crate::types::Type> {
        let fallc_ast::ExprKind::Name(_) = &ast.expr(expr).kind else {
            return None;
        };
        let span = ast.expr(expr).span;
        self.binding_types
            .iter()
            .find(|(declared, _)| *declared == span)
            .map(|(_, ty)| ty.clone())
            .filter(|ty| !ty.is_unspecified())
    }

    /// The type `type(expr)` names, as a requirement's argument.
    ///
    /// A name is taken at its *declared* type rather than at the type its use
    /// would have: `type(it)` is the type of `it`, and `it` used as a value
    /// would read as `&Iter` -- the place, not what it holds. Reading it as a
    /// place also went through the checker's binding table with the name `it`
    /// itself as the type, which made an enclosing question answer "no".
    fn type_named_by_expression(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        expr: fallc_ast::ExprId,
    ) -> Option<crate::types::Type> {
        match self.declared_type_of_name(ast, expr) {
            Some(ty) => Some(ty),
            None => {
                // A name that is not a binding is a *type*: `type(it)` where
                // `it` is a type variable, or `type(IntIter)`. It is resolved
                // as a type, not as the value the name would be if it were
                // bound -- looking one up among the bindings found the query's
                // own argument and read `Iter` as `it`.
                self.type_of_expression(ast, resolutions, types, expr, true)
            }
        }
    }

    /// Types a trait query, `InputIterator(IntIter)`, as a compile-time `bool`.
    ///
    /// Returns `None` when the callee is not a trait, which leaves the call to
    /// the ordinary rules. A callee that *is* a trait is a question rather than
    /// a call: the arguments are **types** to ask about, and nothing is
    /// evaluated.
    ///
    /// A value is not a type, so `InputIterator(it)` is an error; to ask about
    /// a value's type, write `InputIterator(type(it))`. That is the same
    /// `type(expr)` the declaration grammar uses, and it is what makes a type
    /// with no spellable name -- an iterator that is a struct -- askable at
    /// all.
    pub(super) fn trait_query(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        call: &fallc_ast::CallExpr,
        span: Span,
    ) -> Option<Type> {
        let whole = match &ast.expr(call.callee).kind {
            fallc_ast::ExprKind::Name(name) => name.clone(),
            fallc_ast::ExprKind::Field(field) => field.field.clone(),
            _ => return None,
        };
        let trait_name = whole
            .rsplit_once('.')
            .map_or(whole.as_str(), |(_, last)| last)
            .to_owned();
        // Not a query at all: a call to something the compiler does not own as
        // a trait either. Asking the argument parser would report a value where
        // a type belongs for every ordinary call that reached here.
        if crate::resolve::builtin_type_trait_arity(&trait_name).is_none()
            && self.trait_body_for(&whole).is_none()
            && self.find_type_family_trait(&trait_name).is_none()
        {
            return None;
        }

        let mut arguments = Vec::new();
        for &argument in &call.args {
            // A type, and only a type. Two spellings name one: a written type
            // (`Input(IntIter)`) and `type(expr)`, which asks the type of a
            // value. Anything else is a value where a type belongs, and a
            // value's type is not what the author wrote.
            let ty = match &ast.expr(argument).kind {
                fallc_ast::ExprKind::TypeOf(inner) => {
                    self.type_named_by_expression(ast, resolutions, types, *inner)
                }
                // A written type -- `Array<int>` in `Iterable(Array<int>)`. The
                // parser reads it with the type grammar, because a template
                // instance is not an expression at all.
                fallc_ast::ExprKind::TypeLiteral(written) => match &**written {
                    // `Iterable(type(it.begin()))`: the type grammar reads the
                    // `type(..)` as a type, and it asks the same question the
                    // expression form does.
                    fallc_ast::TypeSyntax::TypeOf(inner) => {
                        self.type_named_by_expression(ast, resolutions, types, inner.operand)
                    }
                    _ => {
                        // Resolved under the question's substitution, because a
                        // requirement's argument may name a type variable:
                        // `Iterable(Iter)` asks about the type the trait was
                        // asked about, and `Iter` is the name it is written
                        // with.
                        let saved = std::mem::replace(&mut self.resolving_probe, true);
                        let substitution = self.substitution.clone().unwrap_or_default();
                        let resolved = self.resolve_template_type(written, &substitution);
                        self.resolving_probe = saved;
                        (!resolved.is_unspecified()).then_some(resolved)
                    }
                },
                fallc_ast::ExprKind::Name(_) | fallc_ast::ExprKind::Field(_) => {
                    let path = match &ast.expr(argument).kind {
                        fallc_ast::ExprKind::Name(name) => name.clone(),
                        fallc_ast::ExprKind::Field(field) => field.field.clone(),
                        _ => unreachable!("the match above is one of these two"),
                    };
                    let syntax = fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                        path: path.clone(),
                        arguments: Vec::new(),
                        is_expansion: false,
                        span: ast.expr(argument).span,
                    });
                    // A name that is one of the trait's own type variables
                    // stands for the argument the trait was asked about, so
                    // `InputIterator(it) -> true` asks about that type rather
                    // than about a name that is not a type at all.
                    // Only a *type variable the trait declares* stands for the
                    // argument: a requirement that names a type the program
                    // wrote (`Below(Array<int>.Iterator)`) is resolved as
                    // usual, and a name that is a variable's *value* (`it`) is
                    // not a type at all.
                    // A name is read as a type variable when a trait's body
                    // declares one by that name -- the current trait's, or the
                    // one being asked about -- and the substitution has a type
                    // for it. Nothing else is substituted: a name the program
                    // wrote is resolved as a type, and a name that is a
                    // parameter's *value* (`it`) is not a type at all.
                    let declared = self
                        .trait_body_for(&trait_name)
                        .map(|body| body.declaration.type_variables().contains(&path))
                        .unwrap_or(false)
                        || self.substitution.as_ref().is_some_and(|substitution| {
                            substitution.type_of(&path).is_some()
                        });
                    let substituted = declared
                        .then(|| {
                            self.substitution
                                .as_ref()
                                .and_then(|substitution| substitution.type_of(&path))
                                .cloned()
                        })
                        .flatten();
                    Some(substituted.unwrap_or_else(|| {
                        let saved = std::mem::replace(&mut self.resolving_probe, true);
                        let resolved = self.resolve_type_syntax(&syntax, false);
                        self.resolving_probe = saved;
                        resolved
                    }))
                }
                _ => None,
            };
            let Some(ty) = ty else {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{trait_name}` asks about a type, but this argument is a value"
                    ))
                    .with_code(DiagnosticCode::sema(130))
                    .with_span(ast.expr(argument).span)
                    .with_note(format!(
                        "a qualified name such as `IntIter`, or `type(expr)` for the type of a \
                         value: `{trait_name}(type(it))`"
                    )),
                );
                self.constant_values.insert(span, 0);
                return Some(Type::BOOL);
            };
            arguments.push(ty);
        }

        // A built-in type trait is a question the compiler answers: there is
        // no declaration to look up, and the answer is a fact about the types
        // the query was given. This is what `@enable_if(IsInteger(T))` needs --
        // a way to write two overloads that differ only in which kinds of type
        // they take, the way `std::enable_if` constrains a C++ template.
        if let Some(answer) =
            self.builtin_type_trait(&trait_name, &arguments, resolutions, span)
        {
            self.constant_values.insert(span, i64::from(answer));
            return Some(Type::BOOL);
        }
        // A standard-library trait is a type family: the query names the
        // family, the arguments are the types it is asked about, and the
        // answer is the family member's `VALUE` constant. Template
        // specialisation -- full or partial -- decides which body answers.
        if let Some(answer) = self.type_family_trait(&trait_name, &arguments, span) {
            self.constant_values.insert(span, i64::from(answer));
            return Some(Type::BOOL);
        }
        let body = self.trait_body_for(&whole)?;
        if body.declaration.is_forward {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "trait `{}` is incomplete, so it cannot be asked about a type yet",
                    body.declaration.name
                ))
                .with_code(DiagnosticCode::sema(168))
                .with_span(span)
                .with_note(
                    "a forward declaration names the trait; its requirements come with the \
                     definition",
                ),
            );
            return Some(Type::BOOL);
        }
        let trait_name = body.declaration.name.clone();
        let modelled = match self.trait_models(&trait_name, &arguments, span) {
            TraitOutcome::Modelled => true,
            TraitOutcome::Unknown => {
                self.diagnostics.push(
                    Diagnostic::error(format!("cannot find a trait named `{trait_name}`"))
                        .with_code(DiagnosticCode::sema(131))
                        .with_span(span),
                );
                false
            }
            TraitOutcome::Arity {
                expected, found, ..
            } => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{trait_name}` is about {expected} type(s), but {found} were given"
                    ))
                    .with_code(DiagnosticCode::sema(132))
                    .with_span(span)
                    .with_note(
                        "a trait's type variables are the types of its parameters, in the \
                         order they appear",
                    ),
                );
                false
            }
            // A query is a question, so "no" is an answer rather than a
            // mistake: `if constexpr (Input(x))` asks precisely so that it can
            // do something else when the answer is no. How *why* is reported is
            // the caller's business, and a caller that requires the trait --
            // a template constraint, or the range loop -- is where the
            // requirement text and the failing operation belong.
            TraitOutcome::NotModelled { .. } => false,
        };
        // A query is a fact about types, so its value is known before the
        // program runs: `if constexpr (Input(x))` needs a constant, and code
        // generation emits this one rather than a call nothing defines.
        self.constant_values.insert(span, i64::from(modelled));
        Some(Type::BOOL)
    }

    /// Finds a type family that is also a trait.
    ///
    /// A trait family is a type template whose body declares a constant member
    /// named `VALUE`: the query asks about the type, and the constant is the
    /// answer. The name may be written with the module it came from
    /// (`std.IsInteger`), so the last segment is tried as well.
    ///
    /// Returns the key the family is registered under and its declaration. The
    /// key is what the specialisation tables are filed under.
    fn find_type_family_trait(&self, name: &str) -> Option<(String, fallc_ast::TypeDecl)> {
        if let Some(template) = self.type_templates.get(name)
            && self.family_declares_value(name, template)
        {
            return Some((name.to_owned(), template.clone()));
        }
        // Only a qualified spelling can name a family that is filed under a
        // bare name; an unqualified one has already been looked up above, so
        // an ordinary call does not pay for a scan of every family.
        let (_, bare) = name.rsplit_once('.')?;
        let mut keys: Vec<&String> = self
            .type_templates
            .keys()
            .filter(|key| {
                key.rsplit_once('.').map_or(key.as_str(), |(_, last)| last) == bare
            })
            .collect();
        keys.sort();
        for key in keys {
            let template = &self.type_templates[key];
            if self.family_declares_value(key, template) {
                return Some((key.clone(), template.clone()));
            }
        }
        None
    }

    /// Whether a family declares the constant a trait query reads.
    ///
    /// A family declared here says so in its own body. One that came from
    /// another module has no initialisers left -- they were folded where they
    /// were written -- so the answer is read from the values that crossed with
    /// it, either the generic body's or a specialisation's.
    fn family_declares_value(&self, key: &str, template: &fallc_ast::TypeDecl) -> bool {
        if template
            .constants
            .iter()
            .any(|constant| constant.name == "VALUE")
        {
            return true;
        }
        if self
            .imported_family_constants
            .get(key)
            .is_some_and(|constants| constants.contains_key("VALUE"))
        {
            return true;
        }
        self.imported_specialisations.get(key).is_some_and(|specs| {
            specs
                .iter()
                .any(|spec| spec.constants.iter().any(|(name, _)| name == "VALUE"))
        })
    }

    /// Answers a query whose callee is a standard-library trait family.
    ///
    /// The arguments are the types the query is about; the family is
    /// instantiated with them, the specialisation machinery chooses the body --
    /// including a full or partial specialisation -- and the value of that
    /// body's `VALUE` constant is the answer. `None` means the name is not a
    /// trait family, which sends the query on to the declared traits.
    fn type_family_trait(&mut self, name: &str, arguments: &[Type], span: Span) -> Option<bool> {
        let (key, template) = self.find_type_family_trait(name)?;
        let fixed = template
            .template_params
            .iter()
            .take_while(|param| !param.is_pack)
            .count();
        let has_pack = fixed < template.template_params.len();
        let wrong = if has_pack {
            arguments.len() < fixed
        } else {
            arguments.len() != fixed
        };
        if wrong {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{name}` is about {fixed} type(s), but {} were given",
                    arguments.len()
                ))
                .with_code(DiagnosticCode::sema(132))
                .with_span(span)
                .with_note("a type-trait family takes types, not values"),
            );
            return Some(false);
        }
        // The specialisations may be filed under the family's declared name
        // rather than the spelling this query used, so pick the key that has
        // them.
        let specialisation_key = if self.type_specialisations.contains_key(&key)
            || self.imported_specialisations.contains_key(&key)
        {
            key.clone()
        } else if self.type_specialisations.contains_key(&template.name)
            || self.imported_specialisations.contains_key(&template.name)
        {
            template.name.clone()
        } else {
            key
        };
        let arguments = crate::template::Arguments(
            arguments
                .iter()
                .cloned()
                .map(crate::template::TemplateArgument::Type)
                .collect(),
        );
        match self.family_member_constant(
            &specialisation_key,
            &template,
            &arguments,
            "VALUE",
            span,
        ) {
            Some(value) => Some(value != 0),
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{name}` has no `VALUE` constant for these types, so the query \
                         cannot be answered"
                    ))
                    .with_code(DiagnosticCode::sema(68))
                    .with_span(span)
                    .with_note(
                        "the family's body declares `VALUE const bool = ...;`, and the \
                         specialisation the arguments match must fold to a value",
                    ),
                );
                Some(false)
            }
        }
    }

    /// Answers one of the compiler's own type traits.
    ///
    /// # The traits
    ///
    /// `IsArray` takes one type and asks whether it is a fixed-size array;
    /// `IsSame` takes two and asks whether they are the same type;
    /// `MoveCanRaise` takes one and asks whether moving a value of it can
    /// raise. They are written like any other trait query and fold to a
    /// constant, so `@enable_if(!MoveCanRaise(T))` picks an overload.
    ///
    /// The category predicates -- `IsInteger`, `IsFloating`, `IsNumeric`,
    /// `IsPointer`, `IsReference`, `IsVoid`, `IsBool`, `IsCharacter` -- are
    /// *not* here: they are standard-library type families
    /// (`std.type_traits`), answered by template specialisation like any other
    /// family. What remains built in is what a family cannot express: a pattern
    /// cannot wildcard an array's length (`IsArray`), cannot require two
    /// argument types to be equal (`IsSame`), and cannot read the `except`
    /// marker of the move constructor a container would call
    /// (`MoveCanRaise`).
    ///
    /// Answers `None` for a name the compiler does not own, which sends the
    /// query on to the declared traits.
    fn builtin_type_trait(
        &mut self,
        name: &str,
        arguments: &[Type],
        resolutions: &Resolutions,
        span: Span,
    ) -> Option<bool> {
        // No type predicate is the compiler's any more. `IsArray`, `IsSame` and
        // `MoveCanRaise` are `std.type_traits` families now: the array pattern
        // `[]T` leaves the length open, `IsSame<A, A>` binds one parameter in
        // two positions, and the move question is the `except` query on a move
        // construction. Keeping the call path (rather than deleting it) is what
        // makes the built-in set a table entry away if a future predicate needs
        // the compiler's representation.
        let _ = (name, arguments, resolutions, span);
        None
    }

    /// Whether a declaration's `@enable_if` condition holds for a member.
    ///
    /// Asked while overloads are being compared, because a family may declare
    /// two methods with the same name and parameters and tell them apart only by
    /// the direction their member has: the `__call__` that writes and the one
    /// that reads. Without asking, both fit and the disabled one is chosen --
    /// and then reported, which is a call that should simply not exist.
    pub(super) fn signature_enabled(
        &mut self,
        signature: &FunctionSignature,
        substitution: &Option<crate::template::Substitution>,
    ) -> bool {
        let symbol = crate::template_symbol_of(&signature.written_name, &signature.param_types);
        let Some(body) = self
            .template_body_for(&symbol)
            .or_else(|| self.template_body_for(&signature.bound_as))
        else {
            return true;
        };
        if !body.attrs.iter().any(|attr| attr.name == "enable_if") {
            return true;
        }
        let saved = match substitution {
            Some(substitution) => self.substitution.replace(substitution.clone()),
            None => self.substitution.take(),
        };
        let saved_index = self.adopt_const_index(&body.tree);
        let pushed = body.environment.clone();
        if let Some(environment) = pushed.clone() {
            self.type_environments.push(environment);
        }
        // The value parameters of the family and of the method, so that a name
        // the condition uses has the type its declaration gave it: `Mode` is an
        // `OpenMode`, and `OpenMode.READ` is the constant it names.
        let saved_values = std::mem::take(&mut self.template_value_types);
        for parameter in signature.template_params.iter().chain(
            match substitution {
                Some(substitution) => substitution.params(),
                None => &[],
            }.iter(),
        ) {
            if let fallc_ast::TemplateParamKind::Value(ty) = &parameter.kind {
                let ty = match substitution {
                    Some(substitution) => self.resolve_template_type(ty, substitution),
                    None => self.resolve_written_type(ty),
                };
                self.template_value_types.insert(parameter.name.clone(), ty);
            }
        }
        // The body's own constants, so that an enum member it names -- 
        // `OpenMode.READ` -- is a value here as it is there.
        let saved_constants = std::mem::replace(
            &mut self.instantiation_constants,
            body.constants.clone(),
        );
        // Asking is a question, not a report: a condition that cannot be folded
        // here is left to the instantiation that will check it, and whatever the
        // question itself says is dropped.
        let saved_diagnostics = std::mem::take(&mut self.diagnostics);
        let mut scratch = crate::types::TypeTable::new();
        let allowed = self.enable_if(
            &body.tree,
            &body.resolutions,
            &mut scratch,
            &body.attrs,
            body.function.name_span,
        );
        self.diagnostics = saved_diagnostics;
        if pushed.is_some() {
            self.type_environments.pop();
        }
        self.instantiation_constants = saved_constants;
        self.template_value_types = saved_values;
        self.restore_const_index(saved_index);
        self.substitution = saved;
        allowed != Some(false)
    }

    /// The tree and body of a templated method, wherever it was declared.
    ///
    /// Looked up by the name it is *bound* under, because that is what an
    /// imported definition records: a method of a family is bound as
    /// `__init__Array`, which is the name its own declaration produced.
    pub(super) fn template_body_for(&self, name: &str) -> Option<TemplateBody> {
        self.template_definitions.get(name).cloned()
    }

    /// The module-level constants this checker knows, which a template's body
    /// may name.
    #[must_use]
    pub fn constants(&self) -> std::collections::HashMap<String, fallc_ast::ExprId> {
        self.constants.clone()
    }

    /// Registers the types another module exports.
    ///
    /// Called before checking, for the same reason a local `type` declaration
    /// is collected first: a type has to be known before anything that names
    /// it, and an imported one is no different. Only exported types arrive
    /// here, because a module's declarations are private unless published.
    #[allow(clippy::too_many_arguments)]
    pub fn add_exported_type(
        &mut self,
        name: impl Into<String>,
        exported_fields: Vec<fallc_ast::FieldDecl>,
        fields: Vec<(String, Type)>,
        template_params: Vec<fallc_ast::TemplateParam>,
        template_name: Option<String>,
        is_union: bool,
        is_forward: bool,
        field_alignments: Vec<(String, u64)>,
        exported_nested: Vec<(String, fallc_ast::TypeSyntax)>,
        specialisations: Vec<crate::resolve::ExportedSpecialisation>,
        constants: Vec<(String, i64)>,
    ) {
        let name: String = name.into();
        self.imported_type_names.insert(name.clone());
        // A field's mutability is part of the type: it is carried by the
        // declarations below. Its *alignment* is a number measured where the
        // type was declared, and it is what puts every field after the first at
        // the right offset.
        let mutable_fields: Vec<String> = exported_fields
            .iter()
            .filter(|field| field.mutable)
            .map(|field| field.name.clone())
            .collect();
        // A member of a family is built later, from this declaration, so the
        // alignments wait for it here, under the family's name.
        if !field_alignments.is_empty() {
            self.imported_field_alignments
                .insert(name.clone(), field_alignments.clone());
        }
        // Kept for the specialisations below, which are registered after the
        // name itself has been moved into the nominal type.
        let specialisation_key = name.clone();
        // A family of types is a family here too. The parameters are what let
        // an importer write `Array<int>` -- without them the name is an
        // ordinary type with no arguments, and every use of the family is
        // reported as a type that does not exist.
        if !template_params.is_empty() {
            // The name the *declaring* module knows the family by, which is
            // what its methods are bound under: the definition is
            // `__init__Array`, not `__init__std.Array`. A member's name is
            // built from this, so using the importer's spelling here would
            // build `std.Array<i32>` and look for methods that do not exist.
            let family = template_name.unwrap_or_else(|| {
                name.rsplit_once('.')
                    .map_or(name.clone(), |(_, last)| last.to_owned())
            });
            // The *fields* travel too, because a member is built from this
            // declaration: without them `Array<int>` was a family member with
            // no fields at all, and every use of `this._len` was reported as a
            // missing field.
            let declaration = fallc_ast::TypeDecl {
                template_params,
                name: family.clone(),
                name_span: fallc_span::Span::SYNTHETIC,
                exported: true,
                fields: exported_fields
                    .iter()
                    .map(|field| fallc_ast::FieldDecl {
                        name: field.name.clone(),
                        name_span: fallc_span::Span::SYNTHETIC,
                        ty: field.ty.clone(),
                        mutable: false,
                        align: None,
                        span: fallc_span::Span::SYNTHETIC,
                    })
                    .collect(),
                // A family is a struct: a `union` or an alias cannot have
                // template parameters yet, because neither has a rule for what
                // its members would mean.
                kind: fallc_ast::TypeDeclKind::Struct,
                // An exported family is a definition, never a declaration
                // without fields: an incomplete type cannot be imported.
                pattern: Vec::new(),
                is_forward: false,
                constants: Vec::new(),
                // The types declared inside it travel with the export, as
                // `(name, target)` aliases, and are turned back into
                // declarations here so that a member is built from the same
                // shape as the declaration was.
                nested: exported_nested
                    .iter()
                    .map(|(nested_name, target)| fallc_ast::TypeDecl {
                        template_params: Vec::new(),
                        name: nested_name.clone(),
                        name_span: fallc_span::Span::SYNTHETIC,
                        exported: false,
                        fields: Vec::new(),
                        kind: fallc_ast::TypeDeclKind::Alias(target.clone()),
                        pattern: Vec::new(),
                        is_forward: false,
                        constants: Vec::new(),
                        nested: Vec::new(),
                    })
                    .collect(),
            };
            // Filed under the family's own name -- which is what its methods are
            // bound under -- *and* under the name the importer wrote, if that
            // differs. A program that imported the umbrella writes `std.Array`,
            // and looking only for `Array` found nothing.
            self.type_templates
                .insert(family.clone(), declaration.clone());
            if name != declaration.name {
                self.type_templates.insert(name.clone(), declaration);
            }
            // The answers the generic member gives travel with it: there is no
            // expression tree here to fold, and a family imported under two
            // names has to answer under both.
            if !constants.is_empty() {
                let answers: std::collections::HashMap<String, i64> =
                    constants.iter().cloned().collect();
                self.imported_family_constants
                    .insert(family.clone(), answers.clone());
                self.imported_family_constants
                    .insert(name.clone(), answers);
            }
        }
        // The types declared inside this one are named through it --
        // `Outer.Inner` -- so they are registered under the owner's name, with
        // the target resolved where the owner is known.
        for (nested_name, target) in &exported_nested {
            let resolved = self.resolve_written_type(target);
            self.aliases
                .insert(format!("{name}.{nested_name}"), resolved);
        }
        self.nominal_types
            .insert(
                name.clone(),
                crate::types::NominalType {
                    template_arguments: None,
                    name,
                    fields,
                    mutable_fields,
                    is_union,
                    // A forward declaration crosses as an incomplete type: it
                    // may be named behind a pointer, and nothing may be read
                    // through it until the definition is seen.
                    is_incomplete: is_forward,
                    // The storage member is the largest field, and which one
                    // that is needs each field's size -- which is the
                    // declaring module's to measure. The importer takes the
                    // first, which is correct for every union whose largest
                    // member is written first and a conservative reading
                    // otherwise: overlapping members still address the same
                    // bytes, only the space reserved differs.
                    union_storage: is_union.then_some(0),
                    field_alignments,
                },
            );
        // A family's specialisations arrived with it: a member's fields are
        // decided here, and a caller that did not know them would build the
        // generic member and read the wrong layout.
        if !specialisations.is_empty() {
            let entry = self
                .imported_specialisations
                .entry(specialisation_key)
                .or_default();
            for specialisation in specialisations {
                entry.push(ImportedSpecialisation {
                    params: specialisation.params,
                    pattern: specialisation.pattern,
                    fields: specialisation.fields,
                    is_union: specialisation.is_union,
                    mutable_fields: specialisation.mutable_fields,
                    alignments: specialisation.alignments,
                    constants: specialisation.constants,
                });
            }
        }
    }

    /// Resolves a name the innermost template body's own module declares.
    ///
    /// A body is checked where it is instantiated, but its names mean what they
    /// meant in the module that wrote it: a private type it names has to be
    /// visible here, and a family it uses is that module's family even when
    /// this file declares one of the same name. Only names the body actually
    /// uses are looked up, so a declaration it never names cannot conflict with
    /// anything.
    ///
    /// `None` means the module does not declare the name, and ordinary
    /// resolution continues.
    pub(super) fn environment_type(
        &mut self,
        environment: &TypeEnvironment,
        named: &fallc_ast::NamedType,
    ) -> Option<Type> {
        let name = named.path.as_str();
        // An enum is its base type, and an alias is the type it names: neither
        // is a layout of its own, so a copy made here is the same thing.
        if let Some(base) = environment.enum_bases.get(name) {
            return Some(base.clone());
        }
        if let Some(alias) = environment.aliases.get(name) {
            return Some(alias.clone());
        }
        // An interface is a type of its own here: the conversion and the
        // virtual call look it up by name, so the body's module's method list
        // is what its name has to mean. Two interfaces under one name are the
        // same conflict as two families.
        if let Some(interface) = environment.interfaces.get(name).cloned() {
            match self.interfaces.get(name) {
                Some(existing)
                    if existing.methods.len() == interface.methods.len()
                        && existing
                            .methods
                            .iter()
                            .zip(&interface.methods)
                            .all(|(left, right)| left.name == right.name) => {}
                Some(_) => {
                    let module = environment.module.clone();
                    self.report_environment_conflict(name, &module, named.span);
                    return None;
                }
                None => {
                    self.interfaces.insert(name.to_owned(), interface);
                }
            }
        }
        // A family's name is not a type of its own: `Less` in `Less<E>` names
        // the family, and the *member* is what the written arguments make. The
        // environment may carry a bare entry under the name -- the declaration
        // itself -- and taking that one dropped the arguments, so a comparator
        // call in another module resolved its receiver to `Less` and the call
        // did not fit.
        if !environment.type_templates.contains_key(name)
            && let Some(ty) = environment.nominal_types.get(name).cloned()
        {
            if let Some(existing) = self.nominal_types.get(name) {
                if !same_nominal_shape(existing, &ty) {
                    let module = environment.module.clone();
                    self.report_environment_conflict(name, &module, named.span);
                }
            } else if self.type_templates.contains_key(name) {
                let module = environment.module.clone();
                self.report_environment_conflict(name, &module, named.span);
                return None;
            } else {
                // The body's module's own declaration, made visible here so
                // that a field of it can be read.
                self.nominal_types.insert(name.to_owned(), ty.clone());
            }
            return Some(crate::types::Type::Nominal(ty));
        }
        let declaration = environment.type_templates.get(name).cloned()?;
        // A family of the same name declared *here* does not replace the one
        // the body was written against: the body's names mean what its own
        // module meant by them, and a program may declare its own `Pair` while
        // `std.tuple`'s methods still mean `std.tuple.Pair`. The environment's
        // family is therefore installed for the member being built and taken
        // out again; the local declaration keeps its meaning everywhere else.
        if let Some(existing) = self.type_templates.get(name).cloned()
            && same_declaration(&existing, &declaration)
        {
            // The same family, already registered here: build the member from
            // it like any other use.
            return self.instantiate_type_template(named);
        }
        // The body's module's family, registered only for as long as the
        // member is being built: the name itself is not adopted, because this
        // file may declare a type or a family of its own under it.
        let saved = self.type_templates.insert(name.to_owned(), declaration);
        let saved_specialisations = environment
            .type_specialisations
            .get(name)
            .cloned()
            .map(|specialisations| {
                std::mem::replace(
                    self.type_specialisations.entry(name.to_owned()).or_default(),
                    specialisations,
                )
            });
        let member = self.instantiate_type_template(named);
        match saved {
            Some(existing) => {
                self.type_templates.insert(name.to_owned(), existing);
            }
            None => {
                self.type_templates.remove(name);
            }
        }
        if let Some(saved) = saved_specialisations {
            self.type_specialisations.insert(name.to_owned(), saved);
        }
        member
    }

    /// Reports one name the two declaration tables disagree about, once.
    pub(super) fn report_environment_conflict(&mut self, name: &str, module: &str, span: Span) {
        // A conflict is a refusal: the two names would have to mean one type.
        // Reported once per name, however many instantiations meet it.
        if !self
            .environment_conflicts
            .insert((module.to_owned(), name.to_owned()))
        {
            return;
        }
        let diagnostic = Diagnostic::error(format!(
            "`{name}` means two types here: one declared in this file, and one in `{module}`, \
             whose template uses it"
        ))
        .with_code(DiagnosticCode::sema(179))
        .with_span(span)
        .with_note(
            "a name has one meaning in a program, so the two cannot both be `{name}`; rename one \
             of them, or the family it names",
        );
        self.diagnostics.push(diagnostic);
    }

    /// Registers one interface: its method list, and the value type that holds
    /// a heap copy and a method table.
    ///
    /// The same declaration arrives under every name a program may write it by
    /// -- `IShape`, `std.IShape`, `std.shape.IShape` -- and each is registered
    /// as its own name, because a virtual call looks the interface up by the
    /// name the program wrote. The methods are the same in all of them.
    pub(super) fn register_interface(&mut self, interface: &fallc_ast::InterfaceDecl) {
        self.interfaces
            .insert(interface.name.clone(), interface.clone());
        let pointer = || {
            fallc_ast::TypeSyntax::Pointer(Box::new(fallc_ast::TypeSyntax::Named(
                fallc_ast::NamedType {
                    path: "byte".to_owned(),
                    arguments: Vec::new(),
                    is_expansion: false,
                    span: interface.span,
                },
            )))
        };
        let fields: Vec<fallc_ast::FieldDecl> = ["data", "table"]
            .into_iter()
            .map(|name| fallc_ast::FieldDecl {
                name: name.to_owned(),
                name_span: interface.name_span,
                ty: pointer(),
                mutable: false,
                align: None,
                span: interface.span,
            })
            .collect();
        let declaration = fallc_ast::TypeDecl {
            template_params: interface.template_params.clone(),
            name: interface.name.clone(),
            name_span: interface.name_span,
            exported: interface.exported,
            fields,
            kind: fallc_ast::TypeDeclKind::Struct,
            nested: Vec::new(),
            pattern: Vec::new(),
            is_forward: interface.is_forward,
            constants: Vec::new(),
        };
        if interface.is_forward {
            self.nominal_types.insert(
                interface.name.clone(),
                crate::types::NominalType {
                    template_arguments: None,
                    name: interface.name.clone(),
                    fields: Vec::new(),
                    mutable_fields: Vec::new(),
                    field_alignments: Vec::new(),
                    is_union: false,
                    is_incomplete: true,
                    union_storage: None,
                },
            );
        } else if declaration.template_params.is_empty() {
            let mut resolved = Vec::with_capacity(declaration.fields.len());
            for field in &declaration.fields {
                resolved.push((field.name.clone(), self.resolve_written_type(&field.ty)));
            }
            self.nominal_types.insert(
                interface.name.clone(),
                crate::types::NominalType {
                    template_arguments: None,
                    name: interface.name.clone(),
                    fields: resolved,
                    mutable_fields: Vec::new(),
                    field_alignments: Vec::new(),
                    is_union: false,
                    is_incomplete: false,
                    union_storage: None,
                },
            );
        } else {
            self.type_templates
                .insert(interface.name.clone(), declaration);
        }
    }

    /// Registers a type another module exported as an alias.
    ///
    /// An alias introduces no type: `type Id = usize;` makes `Id` *be* `usize`,
    /// so the importer records the same equivalence. Registered as a nominal
    /// type it became an empty struct, and no value of the aliased type fitted
    /// it.
    pub fn add_imported_alias(
        &mut self,
        name: impl Into<String>,
        target: &fallc_ast::TypeSyntax,
    ) {
        let name: String = name.into();
        self.imported_type_names.insert(name.clone());
        let resolved = self.resolve_written_type(target);
        self.aliases.insert(name, resolved);
    }

    /// Records that a name is deprecated, for a declaration another module
    /// published.
    pub fn add_deprecated_name(&mut self, name: impl Into<String>, message: Option<String>) {
        self.deprecated_functions
            .entry(name.into())
            .or_insert(message);
    }

    /// Registers a constant another module exported, under one of the names a
    /// program may write for it.
    ///
    /// The value is folded already; the type is what says how wide it is, and
    /// it is resolved here because a written type names types in the declaring
    /// module's table.
    pub fn add_imported_constant(
        &mut self,
        name: impl Into<String>,
        value: i64,
        ty: &fallc_ast::TypeSyntax,
    ) {
        let name: String = name.into();
        let resolved = self.resolve_written_type(ty);
        self.constants_by_name.insert(name.clone(), value);
        self.constant_types_by_name.insert(name, resolved);
    }

    /// Registers an enum another module exported.
    ///
    /// An enum is its base type under a name that gives its members meaning, so
    /// it is registered as an alias and as an enum base, and each member
    /// becomes a constant -- `Color.RED` has to mean here the number it meant
    /// there.
    pub fn add_imported_enum(
        &mut self,
        name: impl Into<String>,
        base: Option<&fallc_ast::TypeSyntax>,
        members: &[(String, i64)],
    ) {
        let name: String = name.into();
        let base = match base {
            Some(written) => self.resolve_written_type(written),
            // The base defaults to `int`, as it does where the enum is declared.
            None => Type::INT,
        };
        self.enum_bases.insert(name.clone(), base.clone());
        self.aliases.insert(name.clone(), base.clone());
        let member = |index: usize| members[index].clone();
        for index in 0..members.len() {
            let (member, value) = member(index);
            let qualified = format!("{name}.{member}");
            self.constants_by_name.insert(qualified.clone(), value);
            self.constant_types_by_name.insert(qualified, base.clone());
        }
    }

    /// Registers an interface another module exported, under one of the names
    /// a program may write for it.
    pub fn add_imported_interface(
        &mut self,
        name: impl Into<String>,
        exported: &crate::resolve::ExportInterface,
    ) {
        let name: String = name.into();
        let declaration = fallc_ast::InterfaceDecl {
            template_params: exported.template_params.clone(),
            // An imported interface carries the signatures a box calls. Its
            // restrictions are checked where it is boxed, which needs the
            // declaring module's tree; the export table does not carry one.
            subject: None,
            requirements: Vec::new(),
            name: name.clone(),
            name_span: fallc_span::Span::SYNTHETIC,
            methods: exported
                .methods
                .iter()
                .map(|method| fallc_ast::InterfaceMethod {
                    name: method.name.clone(),
                    name_span: fallc_span::Span::SYNTHETIC,
                    params: method.params.clone(),
                    returns: method.returns.clone(),
                    throws: method.throws,
                    span: fallc_span::Span::SYNTHETIC,
                })
                .collect(),
            // What crossed the boundary is a module's published interface, so
            // it is exported here too.
            exported: true,
            is_forward: exported.is_forward,
            span: fallc_span::Span::SYNTHETIC,
        };
        self.register_interface(&declaration);
    }

    /// Checks `static_assert(cond)`: a `bool` that the compiler must know, and
    /// must be true.
    ///
    /// A constant is what makes it a *static* assertion: the claim has to be
    /// decided while compiling, because there is nothing left to check when the
    /// program runs. Inside a template the claim is checked once per
    /// instantiation, so a template nobody instantiates makes no claim.
    pub(super) fn check_static_assert(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &fallc_ast::StaticAssert,
    ) {
        let previous = self.expected.replace(Type::BOOL);
        let found = self.infer_expr(ast, resolutions, types, node.condition);
        self.expected = previous;
        let span = ast.expr(node.condition).span;
        if let Some(found) = found
            && !found.is_bool()
        {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "a `static_assert` condition must be a `bool`, but this is `{}`",
                    found.spelling()
                ))
                .with_code(DiagnosticCode::sema(163))
                .with_span(span)
                .with_note("compare the value instead: `static_assert(sizeof(T) == 4)`"),
            );
            return;
        }
        if self.constant_expression_problem(node.condition).is_some() {
            self.diagnostics.push(
                Diagnostic::error("a `static_assert` condition must be a constant")
                    .with_code(DiagnosticCode::sema(164))
                    .with_span(span)
                    .with_note(
                        "write a literal, a named constant, arithmetic on those, or a \
                         `constexpr` call",
                    ),
            );
            return;
        }
        if self.const_eval_int(node.condition) == Some(0) {
            // The message the author wrote is the point of writing it, so it is
            // part of the error rather than a note that `-Wverbose` hides.
            let message = match &node.message {
                Some(message) => format!("static assertion failed: {message}"),
                None => "static assertion failed".to_owned(),
            };
            self.diagnostics.push(
                Diagnostic::error(message)
                    .with_code(DiagnosticCode::sema(165))
                    .with_span(node.span)
                    .with_note("the condition is `false` at compile time"),
            );
        }
    }

    /// Checks every top-level item.
    ///
    /// A template is the one form skipped here: its body names types that do
    /// not exist until it is instantiated, so it is checked once per use.
    pub(super) fn check_items(&mut self, ast: &Ast, resolutions: &Resolutions, types: &mut TypeTable) {
        // A method written against a type family's parameters is a *template*
        // too -- `fn __init__(this &Box<T>, v T)` -- and there is no member for
        // it to be checked against until one is used. Where its body is is
        // recorded now, so that the instantiation can find it later.
        for &item in &ast.items {
            let item_ref = ast.item(item);
            match &item_ref.kind {
                fallc_ast::ItemKind::Function(function) => {
                    // A receiver is always a reference: `this &T` or
                    // `this &const T`. A pointer receiver would be a second
                    // spelling of the same thing, and the two disagreed about
                    // what `this` is inside the body (`this.x` versus
                    // `this->x`), so the language has one.
                    if let Some(receiver) = function.params.first()
                        && receiver.name == "this"
                        && matches!(
                            &receiver.ty,
                            fallc_ast::TypeSyntax::Pointer(_)
                                | fallc_ast::TypeSyntax::PointerToConst(_)
                                | fallc_ast::TypeSyntax::ConstPointer(_)
                        )
                    {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "a receiver must be a reference: `this &T` or `this &const T`",
                            )
                            .with_code(DiagnosticCode::sema(190))
                            .with_span(receiver.ty.span())
                            .with_note(
                                "the pointer spelling is not a second way to write a method; \
                                 use `this &T` and write `this.field` in the body",
                            ),
                        );
                    }
                    self.check_function_attributes(
                        ast,
                        &item_ref.attrs,
                        &function.name,
                        &function.return_type,
                    );
                    // A template is not checked here. Its body names types that
                    // do not exist yet -- that is what `T` is -- so checking it
                    // would report every use of a parameter. It is checked once
                    // per instantiation instead, with the parameters replaced
                    // by what they stand for; see `check_instantiation`.
                    if !function.template_params.is_empty() || function.has_pack() {
                        continue;
                    }
                    // A declaration whose `@enable_if` condition is false does
                    // not exist for this program: its body is not checked, and
                    // a call does not consider it. A template's condition is
                    // checked per instantiation, and a method of a family's per
                    // member, because both mention a type the declaration does
                    // not fix.
                    let family_method = function
                        .params
                        .first()
                        .is_some_and(|param| self.mentions_template_parameter_syntax(&param.ty));
                    if function.template_params.is_empty()
                        && !function.has_pack()
                        && !family_method
                        && self.enable_if(ast, resolutions, types, &item_ref.attrs, function.name_span)
                            == Some(false)
                    {
                        self.disabled_functions.insert(function.name_span);
                        continue;
                    }

                    // A method of a type family has the same problem with a
                    // different spelling of it: its receiver is `&Array<T>`,
                    // and `T` is the *family's* parameter rather than one the
                    // method declared. It too is checked per member.
                    if family_method
                    {
                        // Recorded so that code generation does not emit the
                        // declaration itself: it names the family's `T`, and
                        // the member is emitted from its instantiation.
                        self.templated_methods.insert(function.name_span);
                        continue;
                    }
                    // Record each parameter's declared type under its name
                    // span, so a reference to a parameter resolves exactly the
                    // way a local binding does.
                    for param in &function.params {
                        // A parameter type may ask for an expression's type:
                        // `fn f(a int, b type(a))` gives `b` the type of `a`,
                        // because the parameters before it are already in
                        // scope when the call is checked.
                        self.prepare_type_of(&param.ty, ast, resolutions, types);
                        // `auto` and `type(auto)` are deduced from an
                        // initialiser or a `return`, and a parameter has
                        // neither: its type is part of the signature, which
                        // every caller has to match.
                        if mentions_auto(&param.ty) {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "parameter `{}` cannot be deduced: `auto` and `type(auto)` \
                                     come from an initialiser or a return, and a parameter has \
                                     neither",
                                    param.name
                                ))
                                .with_code(DiagnosticCode::sema(169))
                                .with_span(param.ty.span())
                                .with_note("write the type out, or make the function a template"),
                            );
                        }
                        // In *parameter* position: `[]T` there is `*T`, and a
                        // `const` element is a qualifier on the value rather
                        // than on the binding. Resolving it as an ordinary
                        // type left `[]const T` as an unsized array, so writing
                        // through it was never refused.
                        let ty = self.resolve_type_syntax(&param.ty, true);
                        // A by-value member-type parameter is copied by a
                        // constructor nothing in the source names; ask for its
                        // instantiation here, where the parameter type is known.
                        self.request_parameter_copy_constructor(resolutions, &ty);
                        self.binding_types.insert(param.name_span, ty.clone());
                        // A default argument is an expression written against
                        // the parameters before it, and it is checked where it
                        // is written: the ones before it are already bound, so
                        // `b int = a + 1` type-checks here.
                        if let Some(default) = param.default {
                            // `check_expr`, not `infer_expr`: the default is
                            // the parameter's value, so it has to have the
                            // parameter's type.
                            let _ = self.check_expr(ast, resolutions, types, default, &ty);
                        }
                    }
                    // A `except(cond)` claim is checked where it is written,
                    // because its `except(..)` queries are answered there: they
                    // ask about types, and the types are known here.
                    if let Some(condition) = function.throws.as_ref().and_then(|throws| throws.condition)
                    {
                        let _ = self.infer_expr(ast, resolutions, types, condition);
                    }
                    // Record the return type against the function's definition
                    // so a later pass can check that the body's `return`
                    // statements agree with it.
                    self.prepare_type_of(&function.return_type, ast, resolutions, types);
                    // `auto` as a return type is deduced from the body: the
                    // first `return` decides it, and `pending_auto_returns`
                    // records which function is still waiting for that.
                    let auto_span = match &function.return_type {
                        fallc_ast::TypeSyntax::Auto(span) => Some((*span, false)),
                        fallc_ast::TypeSyntax::DecltypeAuto(span) => Some((*span, true)),
                        _ => None,
                    };
                    let expected = match auto_span {
                        Some((span, _)) => Type::unspecified("auto", span),
                        None => self.resolve_written_type(&function.return_type),
                    };
                    self.return_types
                        .push((function.name.clone(), expected.clone()));
                    self.pending_auto_returns.push(auto_span);
                    // Publish the return type as the ambient expectation for
                    // the body, so an `auto` binding with a literal initialiser
                    // picks up the function's type rather than the fallback.
                    let previous = self.expected.replace(expected);
                    // `raise` and `??` are allowed only where the function says
                    // it may raise, so that is part of checking its body.
                    self.throws_allowed.push(function.throws.is_some());
                    // A declaration-only check does not enter the body: the
                    // statements belong to the module's object, which was
                    // checked when it was built. A deduced return type is the
                    // exception -- it *is* the signature, and it is read from
                    // the body -- so that body is checked either way.
                    if let Some(body) = function.body
                        && (!self.declarations_only || auto_span.is_some())
                    {
                        self.check_block(ast, resolutions, types, body);
                    }
                    self.throws_allowed.pop();
                    self.expected = previous;
                    // A function that promised `auto` and never returned a
                    // value returns nothing: `void` is what "no value" is
                    // spelled as, and it is the answer rather than a mistake.
                    if let Some(pending) = self.pending_auto_returns.pop()
                        && let Some((span, _)) = pending
                    {
                        let nothing = Type::unspecified(String::new(), span);
                        if let Some((_, expected)) = self.return_types.last_mut() {
                            *expected = nothing.clone();
                        }
                        self.type_of_types.insert(span, nothing);
                    }
                    self.return_types.pop();
                }
                // An external variable is named here and lives in a library.
                // Its type is what the program reads and writes through the
                // name; code generation declares the symbol.
                fallc_ast::ItemKind::ExternVariable(variable) => {
                    self.prepare_type_of(&variable.ty, ast, resolutions, types);
                    let declared = self.resolve_written_type(&variable.ty);
                    self.binding_types
                        .insert(variable.name_span, declared.clone());
                    self.external_variables.insert(variable.name.clone());
                }
                // A module-level variable has storage for the whole run, so
                // its initialiser has to be computable before the program
                // starts. What it is declared to be is what the initialiser
                // must be; `auto` takes the initialiser's type, as it does for
                // a local.
                // A `static_assert` is a claim, not a declaration: it is
                // checked here, and an instantiated template's claim is checked
                // by the instantiation that copies its body.
                fallc_ast::ItemKind::StaticAssert(node) => {
                    self.check_static_assert(ast, resolutions, types, node);
                }
                // `__setup__ { .. }` runs before `main`. It is a body that
                // returns nothing and may not raise, and a module has at most
                // one: two would have no defined order between them.
                // An interface is registered before checking, with the types;
                // its signatures are checked where a value is used as one.
                fallc_ast::ItemKind::Interface(_) => {}
                fallc_ast::ItemKind::Setup(block) => {
                    let span = ast.block(*block).span;
                    if let Some(previous) = self.setup_span {
                        self.diagnostics.push(
                            Diagnostic::error("a module may have only one `__setup__` block")
                                .with_code(DiagnosticCode::sema(174))
                                .with_span(span)
                                .with_note(format!(
                                    "the other one is at bytes {}..{}",
                                    previous.lo, previous.hi
                                )),
                        );
                    } else {
                        self.setup_span = Some(span);
                    }
                    self.return_types
                        .push(("__setup__".to_owned(), Type::unspecified("", span)));
                    self.throws_allowed.push(false);
                    // A setup block is implementation: it runs before `main`,
                    // and its checked body is part of the module's object.
                    if !self.declarations_only {
                        self.check_block(ast, resolutions, types, *block);
                    }
                    self.throws_allowed.pop();
                    self.return_types.pop();
                }
                fallc_ast::ItemKind::Variable(variable) => {
                    let declared = match &variable.ty {
                        fallc_ast::BindingType::Written(ty) => {
                            self.prepare_type_of(ty, ast, resolutions, types);
                            self.resolve_written_type(ty)
                        }
                        fallc_ast::BindingType::Auto(span) => {
                            let previous = self.expected.take();
                            let found = match &variable.init {
                                Some(fallc_ast::Initializer::Expr(expr)) => {
                                    self.infer_expr(ast, resolutions, types, *expr)
                                }
                                _ => None,
                            };
                            self.expected = previous;
                            found.unwrap_or_else(|| {
                                self.diagnostics.push(
                                    Diagnostic::error(format!(
                                        "cannot deduce a type for `{}`: `auto` needs an \
                                         initialiser",
                                        variable.name
                                    ))
                                    .with_code(DiagnosticCode::sema(20))
                                    .with_span(*span),
                                );
                                Type::unspecified("auto", *span)
                            })
                        }
                    };
                    self.binding_types
                        .insert(variable.name_span, declared.clone());
                    self.require_complete(&declared, variable.name_span, "a variable");
                    self.check_binding_alignment(ast, variable, &declared);
                    // A module-level variable may carry its attributes after
                    // the declaration (`count int @static = 0;`); the item's
                    // own list holds the prefix ones, so both are checked.
                    let mut variable_attrs = item_ref.attrs.clone();
                    variable_attrs.extend(variable.attrs.iter().cloned());
                    self.check_variable_attributes(ast, &variable_attrs, variable, &declared);
                    match &variable.init {
                        Some(fallc_ast::Initializer::Expr(expr)) => {
                            let previous = self.expected.replace(declared.clone());
                            let _ = self.check_expr(ast, resolutions, types, *expr, &declared);
                            self.expected = previous;
                        }
                        Some(fallc_ast::Initializer::Designated(init)) => {
                            self.check_designated_init(ast, resolutions, types, init, &declared);
                        }
                        None => {}
                    }
                    // A module-level object exists before any code runs, so its
                    // initial value is part of the program image: it has to be
                    // computable now. A value that depends on run-time state is
                    // a local variable, which is where such a value belongs.
                    if let Some(initializer) = &variable.init {
                        let constant = self.initializer_is_constant(ast, initializer);
                        if !constant {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "the value of `{}` is not a constant expression",
                                    variable.name
                                ))
                                .with_code(DiagnosticCode::sema(68))
                                .with_span(match initializer {
                                    fallc_ast::Initializer::Expr(expr) => ast.expr(*expr).span,
                                    fallc_ast::Initializer::Designated(init) => init.span,
                                })
                                .with_note(
                                    "a variable at module scope is part of the program image; a                                      value that depends on run-time state belongs in a local",
                                ),
                            );
                        }
                    }
                }
                // A constant's value is folded, and its declared type is what
                // the value must be. Both are checked here rather than when the
                // constant is first used, so that a mistake in it is reported
                // where it was written.
                fallc_ast::ItemKind::Const(constant) => {
                    self.constants
                        .insert(constant.name.clone(), constant.value);
                    self.prepare_type_of(&constant.ty, ast, resolutions, types);
                    let declared = self.resolve_written_type(&constant.ty);
                    self.binding_types.insert(constant.name_span, declared.clone());
                    let previous = self.expected.replace(declared.clone());
                    let found = self.infer_expr(ast, resolutions, types, constant.value);
                    self.expected = previous;
                    if let Some(found) = found {
                        self.types_must_match(
                            &declared,
                            &found,
                            ast.expr(constant.value).span,
                            "constant",
                        );
                    }
                    // The value is recorded by *name*, so that a template
                    // declared in this module can use it while another file
                    // checks that template: a constant's expression id belongs
                    // to this tree, and the other file is looking at its own.
                    if let Some(folded) = self.const_eval_int(constant.value) {
                        self.constants_by_name.insert(constant.name.clone(), folded);
                        self.constant_types_by_name
                            .insert(constant.name.clone(), declared.clone());
                    }
                    // A constant has to *have* a value: one that cannot be
                    // folded would have to be computed at run time, which is
                    // not what `const` promises.
                    if self.const_eval_int(constant.value).is_none() {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "the value of `{}` is not a constant expression",
                                constant.name
                            ))
                            .with_code(DiagnosticCode::sema(68))
                            .with_span(ast.expr(constant.value).span)
                            .with_note(
                                "a constant must be computable before the program runs; a \
                                 literal, another constant, or arithmetic on them is",
                            ),
                        );
                    }
                }
                // A type's constant members are values like any others, and
                // they are checked where the type is declared -- even though a
                // family's answer is only chosen per instantiation. Checking
                // here means a mistake in `VALUE` is reported where it was
                // written, and the folded number is recorded for the export
                // pass.
                fallc_ast::ItemKind::TypeDeclaration(declaration) => {
                    self.check_type_constants(ast, resolutions, types, declaration);
                }
                // An enum's members are constants, and each is folded here so
                // that `Color.RED` has a value wherever it is used -- including
                // in a `case` label, which is a constant position.
                fallc_ast::ItemKind::EnumDeclaration(declaration) => {
                    self.check_enum(declaration);
                }
                fallc_ast::ItemKind::Import(_) => {}
                // A link instruction declares no value and no type.
                fallc_ast::ItemKind::Link(_) => {}
                // A trait's requirements are checked where the trait is
                // *asked about* a type, not here: what they mean depends on the
                // type being substituted for the variables, and a declaration
                // with no query has nothing to check them against.
                fallc_ast::ItemKind::Trait(_) => {}
                fallc_ast::ItemKind::Unresolved(_) => {}
            }
        }
    }

    /// Checks and folds the constants a type body declares.
    ///
    /// A constant member follows the same rule as a module constant -- the
    /// value must be computable before the program runs -- but it is not a
    /// name in the module's scope: a use reaches it through the type, and a
    /// family's specialisations may answer differently for different arguments.
    /// The number is recorded by the *initialiser's* span, which is the key the
    /// export pass has for it; the trait-query path folds it again where the
    /// declaration is already in hand.
    fn check_type_constants(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        declaration: &fallc_ast::TypeDecl,
    ) {
        for constant in &declaration.constants {
            self.prepare_type_of(&constant.ty, ast, resolutions, types);
            let declared = self.resolve_written_type(&constant.ty);
            let previous = self.expected.replace(declared.clone());
            let found = self.infer_expr(ast, resolutions, types, constant.value);
            self.expected = previous;
            if let Some(found) = found {
                self.types_must_match(
                    &declared,
                    &found,
                    ast.expr(constant.value).span,
                    "constant",
                );
            }
            match self.const_eval_int(constant.value) {
                Some(value) => {
                    self.constant_values
                        .insert(ast.expr(constant.value).span, value);
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "the value of `{}.{}` is not a constant expression",
                            declaration.name, constant.name
                        ))
                        .with_code(DiagnosticCode::sema(68))
                        .with_span(ast.expr(constant.value).span)
                        .with_note(
                            "a constant member must be computable before the program runs; a \
                             literal, another constant, or arithmetic on them is",
                        ),
                    );
                }
            }
        }
    }

}

/// Whether two declared types have the same shape.
///
/// Field *names* and the count are what a body depends on: a template that
/// reads `this.value` cannot be checked against a type whose member is called
/// `v`. The field types are not compared, and an empty field list counts as
/// "cannot tell" rather than as a difference -- two copies of one type made in
/// different files may carry differently deep records of the same fields.
fn same_nominal_shape(left: &crate::types::NominalType, right: &crate::types::NominalType) -> bool {
    left.is_union == right.is_union
        && (left.fields.is_empty()
            || right.fields.is_empty()
            || (left.fields.len() == right.fields.len()
                && left
                    .fields
                    .iter()
                    .zip(&right.fields)
                    .all(|((name, _), (other, _))| name == other)))
}

/// Whether two families declare the same thing.
///
/// Spans are not compared: the same declaration copied into two trees has two
/// sets of spans and is still one declaration.
fn same_declaration(left: &fallc_ast::TypeDecl, right: &fallc_ast::TypeDecl) -> bool {
    left.kind == right.kind
        && left.template_params.len() == right.template_params.len()
        && (left.fields.is_empty()
            || right.fields.is_empty()
            || (left.fields.len() == right.fields.len()
                && left
                    .fields
                    .iter()
                    .zip(&right.fields)
                    .all(|(left, right)| left.name == right.name)))
}
