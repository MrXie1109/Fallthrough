//! Attributes, alignment, and the layout questions attributes ask.
//!
//! `@align`, `sizeof` and `alignof` all come down to the same
//! layout rules, so they are answered in one place.

use super::*;

impl TypeChecker {
    /// Resolves a type named inside a constant expression.
    ///
    /// The same resolution a written type gets, but it may not report: folding
    /// happens while a type is being resolved, and a type named in a `sizeof`
    /// has not necessarily been collected yet. A type that does not resolve
    /// makes the expression not-a-constant, which is reported where the
    /// constant was required.
    pub(super) fn resolve_const_type(&self, ty: &fallc_ast::TypeSyntax) -> Type {
        let mut probe = Self::new(self.pointer_bits);
        probe.nominal_types = self.nominal_types.clone();
        probe.resolving_probe = true;
        // The substitution travels with the probe, so that a `sizeof(T)` inside
        // a template measures what `T` actually stands for. Without it the
        // probe had never heard of `T`, and every `if (sizeof(T) == n)` in a
        // template was undecidable -- which left both branches live.
        probe.substitution = self.substitution.clone();
        probe.resolve_type_syntax(ty, false)
    }

    /// Reads and checks the `@align` of each field.
    ///
    /// A requested alignment has to be a compile-time constant, a positive
    /// power of two, and no weaker than the field's own: `@align` *raises* a
    /// field's alignment, and a request that would lower it is a mistake rather
    /// than a hint. What is returned is the requested alignment; the layout
    /// takes the larger of it and the type's own, so a request equal to the
    /// natural one changes nothing.
    ///
    /// The pairs are in declaration order and only cover fields that asked for
    /// something, because a field that asked for nothing needs no entry.
    /// Checks the attributes written before a function.
    ///
    /// `@inline` asks the code generator to inline the body, `@noreturn` says
    /// the function does not come back, and `@deprecated` warns at each use.
    /// The first two say something about a function that a variable cannot be,
    /// which is why they are refused on anything else.
    pub(super) fn check_function_attributes(
        &mut self,
        ast: &Ast,
        attrs: &[fallc_ast::Attr],
        name: &str,
        name_span: fallc_span::Span,
        return_type: &fallc_ast::TypeSyntax,
    ) {
        for attr in attrs {
            match attr.name.as_str() {
                "inline" => {
                    if !attr.arguments.is_empty() {
                        self.attribute_arity(attr, 0);
                    }
                }
                "noreturn" => {
                    if !attr.arguments.is_empty() {
                        self.attribute_arity(attr, 0);
                    }
                    // A function that never comes back cannot hand a value to
                    // anyone: the declaration and the attribute would disagree.
                    let declares_a_value = !matches!(return_type, fallc_ast::TypeSyntax::Auto(_))
                        && !matches!(
                            return_type,
                            fallc_ast::TypeSyntax::Named(named) if named.path == "void"
                        );
                    if declares_a_value {
                        self.diagnostics.push(
                            Diagnostic::error("a `@noreturn` function has no value to return")
                                .with_code(DiagnosticCode::sema(156))
                                .with_span(attr.span)
                                .with_note("declare it `void`, or drop the attribute"),
                        );
                    }
                }
                // `@enable_if(cond)` is checked where the declaration is: its
                // value decides whether the declaration exists at all.
                "enable_if" => {
                    if attr.arguments.len() != 1 {
                        self.attribute_arity(attr, 1);
                    }
                }
                "deprecated" => {
                    // The message is optional: `@deprecated` alone says enough.
                    if attr.arguments.len() > 1 {
                        self.attribute_arity(attr, 1);
                    }
                    self.deprecated_functions
                        .insert(name.to_owned(), deprecated_message(ast, attrs));
                }
                // `@align` is about storage, and a function has none.
                "align" => {
                    self.diagnostics.push(
                        Diagnostic::error("`@align` applies to a variable, not to a function")
                            .with_code(DiagnosticCode::sema(157))
                            .with_span(attr.span)
                            .with_note("write `@align(..)` after the type of a variable instead"),
                    );
                }
                // Attributes that replace an old declaration keyword. The
                // parser has already folded them into the declaration's flags;
                // accepting them here is what keeps them from being reported as
                // unknown while their finer semantics are filled in.
                "must_use" => {
                    self.must_use_functions.insert(name.to_owned());
                }
                // `@overridable` marks a default implementation: a library
                // states one, and a program that writes a definition with the
                // same name and the same parameter list replaces it. The
                // replacement is silent, which is the point -- the two do not
                // meet at a call site, so nothing there has to choose.
                "overridable" => {
                    if !attr.arguments.is_empty() {
                        self.attribute_arity(attr, 0);
                    }
                    self.overridable_functions.insert(name_span);
                }
                // `@coroutine`'s own shape is checked here; what it must return
                // is checked where the declaration's types are known.
                "coroutine" => {
                    if !attr.arguments.is_empty() {
                        self.attribute_arity(attr, 0);
                    }
                }
                "export" | "extern_C" | "constexpr" | "noinline" | "maybe_unused" | "cold" => {}
                other => self.unknown_attribute(other, attr.span),
            }
        }
    }

    /// Checks the attributes written before a module-level variable.
    ///
    /// `@align(..)` is the one that applies: it raises a declaration's
    /// alignment above the type's own.
    pub(super) fn check_variable_attributes(
        &mut self,
        ast: &Ast,
        attrs: &[fallc_ast::Attr],
        node: &fallc_ast::BindStmt,
        declared: &Type,
    ) {
        for attr in attrs {
            match attr.name.as_str() {
                "align" => {
                    if attr.arguments.len() != 1 {
                        self.attribute_arity(attr, 1);
                        continue;
                    }
                    self.apply_alignment_request(
                        ast,
                        attr.arguments[0],
                        attr.span,
                        &node.name,
                        node.name_span,
                        declared,
                    );
                }
                "inline" | "noreturn" => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`@{}` applies to a function, not to a variable",
                            attr.name
                        ))
                        .with_code(DiagnosticCode::sema(157))
                        .with_span(attr.span),
                    );
                }
                "deprecated" => {
                    // A module-level variable may be deprecated as a function
                    // may: a use of the name says so.
                    if attr.arguments.len() > 1 {
                        self.attribute_arity(attr, 1);
                    }
                    self.deprecated_functions
                        .insert(node.name.clone(), deprecated_message(ast, attrs));
                }
                // A module-level variable is part of the program image, which
                // is zero-filled by the loader; there is no storage whose
                // contents could be left alone.
                "uninitialized" => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "`@uninitialized` is for a local binding, not a module-level variable",
                        )
                        .with_code(DiagnosticCode::sema(189))
                        .with_span(attr.span)
                        .with_note(
                            "a module-level variable lives in the program image and is \
                             zero-filled before the program starts",
                        ),
                    );
                }
                // Storage and export spellings the parser already folded into
                // the binding's fields, plus the ones the checker treats as
                // intent markers; none of them is a diagnostic on a variable.
                "export" | "static" | "thread_local" | "constexpr" | "extern_C" | "must_use"
                | "maybe_unused" | "cold" => {}
                other => self.unknown_attribute(other, attr.span),
            }
        }
    }

    /// Reports an attribute this version does not know.
    pub(super) fn unknown_attribute(&mut self, name: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!("`@{name}` is not an attribute"))
                .with_code(DiagnosticCode::sema(158))
                .with_span(span)
                .with_note(
                    "the attributes are `@inline`, `@noreturn`, `@deprecated` and `@align(N)`",
                ),
        );
    }

    /// The value of a declaration's `@enable_if(cond)`, if it has one.
    ///
    /// `None` means the attribute is absent. `Some(false)` means the declaration
    /// does not exist for this program: the condition was a constant and false.
    /// A condition that is not constant is refused, because whether a
    /// declaration exists cannot depend on run-time state.
    pub(super) fn enable_if(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        attrs: &[fallc_ast::Attr],
        span: fallc_span::Span,
    ) -> Option<bool> {
        let attr = attrs.iter().find(|attr| attr.name == "enable_if")?;
        let [condition] = attr.arguments.as_slice() else {
            self.diagnostics.push(
                Diagnostic::error("`@enable_if` takes one condition")
                    .with_code(DiagnosticCode::sema(175))
                    .with_span(attr.span),
            );
            return Some(false);
        };
        // The condition may name the template's *value* parameters -- `Mode !=
        // OpenMode.READ` -- and the folder reads values from the instantiation's
        // table of constants rather than from the substitution. They are
        // published for the length of the check and taken back afterwards.
        let published: Vec<(String, Option<i64>)> = self
            .substitution
            .as_ref()
            .map(crate::template::Substitution::int_bindings)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, value)| {
                let previous = self.instantiation_constants.insert(name.clone(), value);
                (name, previous)
            })
            .collect();
        // The question is asked once per candidate, and two candidates may share
        // one condition's span -- a family's two `@enable_if`s are two
        // *declarations*, so they do not, but one declaration is considered
        // against several receivers and each folds its condition afresh. The
        // answer filed by the previous asking belongs to the previous receiver,
        // so it is dropped before this one is worked out: keeping it made a
        // candidate answer with a neighbour's verdict.
        self.constant_values.remove(&ast.expr(*condition).span);
        let _ = self.check_expr(ast, resolutions, types, *condition, &Type::BOOL);
        let value = self
            .constant_values
            .get(&ast.expr(*condition).span)
            .copied()
            .or_else(|| self.const_eval_int(*condition));
        for (name, previous) in published {
            match previous {
                Some(value) => {
                    self.instantiation_constants.insert(name, value);
                }
                None => {
                    self.instantiation_constants.remove(&name);
                }
            }
        }
        match value {
            Some(value) => Some(value != 0),
            None => {
                self.diagnostics.push(
                    Diagnostic::error("the condition of `@enable_if` must be a constant")
                        .with_code(DiagnosticCode::sema(175))
                        .with_span(attr.span)
                        .with_note(
                            "whether a declaration exists is decided while compiling, so the \
                             condition cannot depend on run-time state",
                        ),
                );
                let _ = span;
                Some(false)
            }
        }
    }

    /// Reports an attribute written with the wrong number of arguments.
    pub(super) fn attribute_arity(&mut self, attr: &fallc_ast::Attr, expected: usize) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "`@{}` takes {expected} argument(s), but {} were given",
                attr.name,
                attr.arguments.len()
            ))
            .with_code(DiagnosticCode::sema(159))
            .with_span(attr.span),
        );
    }

    /// Reads and checks the `@align` of a binding.
    ///
    /// The same rules a field's alignment follows: a compile-time constant (or
    /// a type, which is `alignof` of it), a positive power of two, and no
    /// weaker than the type's own alignment. What is recorded is the alignment
    /// the binding asked for, which code generation sets on its storage.
    pub(super) fn check_binding_alignment(
        &mut self,
        ast: &Ast,
        node: &fallc_ast::BindStmt,
        declared: &Type,
    ) {
        if let Some(align) = &node.align {
            self.apply_alignment_request(
                ast,
                align.alignment,
                align.span,
                &node.name,
                node.name_span,
                declared,
            );
        }
    }

    /// Applies one alignment request to a binding.
    ///
    /// The alignment request is written `@align(..)`, so
    /// the rule about what a request may ask for is stated once.
    pub(super) fn apply_alignment_request(
        &mut self,
        ast: &Ast,
        argument: ExprId,
        span: Span,
        name: &str,
        name_span: Span,
        declared: &Type,
    ) {
        let Some(value) = self.alignas_value(Some(ast), argument) else {
            self.diagnostics.push(
                Diagnostic::error("an `alignas` needs a value the compiler knows")
                    .with_code(DiagnosticCode::sema(137))
                    .with_span(span)
                    .with_note(
                        "the alignment of a binding is fixed before the program runs, like the                          layout of a type",
                    ),
            );
            return;
        };
        if value <= 0 || (value & (value - 1)) != 0 {
            self.diagnostics.push(
                Diagnostic::error(format!("`@align({value})` is not a power of two"))
                    .with_code(DiagnosticCode::sema(138))
                    .with_span(span)
                    .with_note("an alignment is a power of two, and it must be positive"),
            );
            return;
        }
        let natural = self.align_of(declared).unwrap_or(1);
        if value < natural {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`@align({value})` is weaker than `{name}`'s own alignment of {natural} byte(s)"
                ))
                .with_code(DiagnosticCode::sema(139))
                .with_span(span)
                .with_note(
                    "`alignas` only raises an alignment: a value cannot be packed more tightly                      than its type is laid out",
                ),
            );
            return;
        }
        self.binding_alignments.insert(
            name_span,
            u64::try_from(value).expect("a positive alignment"),
        );
    }

    /// Reads the value an `@align(..)` asks for.
    ///
    /// The argument is a constant expression, or a *type*: `alignas(T)` is
    /// `alignas(alignof(T))`, and the two spellings mean the same thing. Which
    /// one it is decided here rather than in the parser, because `alignas(N)`
    /// where `N` is a constant and `alignas(T)` where `T` is a type are spelled
    /// the same way.
    pub(super) fn alignas_alignment(
        &mut self,
        ast: Option<&Ast>,
        align: &fallc_ast::AlignSpec,
    ) -> Option<i64> {
        self.alignas_value(ast, align.alignment)
    }

    /// The value of one `alignas` or `@align` argument.
    pub(super) fn alignas_value(&mut self, ast: Option<&Ast>, argument: ExprId) -> Option<i64> {
        if let Some(value) = self.const_eval_int(argument) {
            return Some(value);
        }
        // The type reading needs the tree to look the argument up. A member of
        // a type family is laid out while its arguments are substituted, which
        // happens without a tree in hand, so `alignas(T)` is read as a number
        // there and nothing else.
        let ast = ast?;
        let fallc_ast::ExprKind::Name(name) = &ast.expr(argument).kind else {
            return None;
        };
        // A type, if the name denotes one. The alignment of a type is fixed, so
        // this is a constant like `alignof` of it.
        let ty = self
            .nominal_types
            .get(name)
            .map(|nominal| Type::Nominal(nominal.clone()))
            .or_else(|| Type::from_name(name));
        let ty = ty?;
        self.align_of(&ty)
    }

    pub(super) fn collect_field_alignments(
        &mut self,
        ast: Option<&Ast>,
        fields: &[(String, Type)],
        declared: &[fallc_ast::FieldDecl],
    ) -> Vec<(String, u64)> {
        let mut alignments = Vec::new();
        for (field, (name, ty)) in declared.iter().zip(fields) {
            let Some(align) = &field.align else {
                continue;
            };
            let span = align.span;
            let Some(value) = self.alignas_alignment(ast, align) else {
                self.diagnostics.push(
                    Diagnostic::error("an `alignas` needs a value the compiler knows")
                        .with_code(DiagnosticCode::sema(137))
                        .with_span(span)
                        .with_note("a field's alignment is part of the type's layout, so it is fixed before the program runs"),
                );
                continue;
            };
            if value <= 0 || (value & (value - 1)) != 0 {
                self.diagnostics.push(
                    Diagnostic::error(format!("`@align({value})` is not a power of two"))
                        .with_code(DiagnosticCode::sema(138))
                        .with_span(span)
                        .with_note("an alignment is a power of two, and it must be positive"),
                );
                continue;
            }
            let natural = self.align_of(ty).unwrap_or(1);
            if value < natural {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`@align({value})` is weaker than `{name}`'s own alignment of {natural} byte(s)"
                    ))
                    .with_code(DiagnosticCode::sema(139))
                    .with_span(span)
                    .with_note(
                        "`alignas` only raises a field's alignment: the type it has cannot be \
                         packed more tightly than its own layout",
                    ),
                );
                continue;
            }
            alignments.push((name.clone(), value as u64));
        }
        alignments
    }

    /// Returns the name of the incomplete type this type is, if it is one.
    ///
    /// Only the type itself: a pointer to an incomplete type is a complete
    /// pointer, and is exactly what a forward declaration is for.
    pub(super) fn incomplete_nominal(&self, ty: &Type) -> Option<String> {
        match ty.unqualified() {
            Type::Nominal(nominal) if nominal.is_incomplete => Some(nominal.name.clone()),
            _ => None,
        }
    }

    /// Reports a use that needs the type to be defined.
    ///
    /// Returns `true` when the type is complete, so a caller can keep going; an
    /// incomplete one is a mistake where it is used, and the diagnostic says
    /// what may still be written.
    pub(super) fn require_complete(&mut self, ty: &Type, span: Span, what: &str) -> bool {
        let Some(name) = self.incomplete_nominal(ty) else {
            return true;
        };
        self.diagnostics.push(
            Diagnostic::error(format!(
                "{what} needs a complete type, and `{name}` is only declared"
            ))
            .with_code(DiagnosticCode::sema(167))
            .with_span(span)
            .with_note(
                "only a pointer or a reference to it may be written until its definition is \
                 seen",
            ),
        );
        false
    }

    /// Returns the size in bytes of a type whose layout is known.
    ///
    /// `None` for a type whose layout is not fixed: a pointer's width depends on
    /// the target, and a declared type's on its fields. A `sizeof` of one of
    /// those is not a constant this compiler can fold, and saying so is better
    /// than inventing a number.
    pub(super) fn size_of(&self, ty: &Type) -> Option<i64> {
        // An incomplete type has no layout to measure.
        if ty.unqualified().is_incomplete() {
            return None;
        }
        self.size_of_at(ty, 0)
    }

    /// Returns the size of a type, refusing to recurse forever.
    ///
    /// A type that contains itself by value has no size, and the checker
    /// reports it where it is declared. A *cycle* between two types -- `A`
    /// holds a `B`, `B` holds an `A` -- has the same shape and reaches this
    /// measure before any declaration complains, so the depth is bounded.
    pub(super) fn size_of_at(&self, ty: &Type, depth: u32) -> Option<i64> {
        if depth > 32 {
            return None;
        }
        match ty {
            // `volatile T` has `T`'s layout.
            Type::Volatile(inner) | Type::Const(inner) => self.size_of_at(inner, depth + 1),
            Type::Array { element, len } => {
                let element = self.size_of_at(element, depth + 1)?;
                let len = i64::try_from(*len).ok()?;
                element.checked_mul(len)
            }
            Type::Nominal(nominal) => {
                let nominal = self.complete_nominal(nominal);
                // A union is as large as its largest member, because every
                // member starts at offset zero, rounded up to the union's own
                // alignment. A struct is its fields side by side, each at the
                // next multiple of its alignment, and the whole padded to the
                // struct's.
                //
                // The padding is what makes `sizeof` agree with the layout
                // code generation gives the struct: `{i32, *Node}` is sixteen
                // bytes, not twelve, and a buffer allocated with the smaller
                // number was written past its end by the second field.
                if nominal.is_union {
                    let mut largest: i64 = 0;
                    let mut alignment: i64 = 1;
                    for (name, field) in &nominal.fields {
                        largest = largest.max(self.size_of_at(field, depth + 1)?);
                        alignment = alignment.max(self.field_alignment(
                            &nominal,
                            name,
                            field,
                            depth + 1,
                        )?);
                    }
                    return Some(round_up(largest, alignment));
                }
                let mut total: i64 = 0;
                for (name, field) in &nominal.fields {
                    let alignment = self.field_alignment(&nominal, name, field, depth + 1)?;
                    total = round_up(total, alignment);
                    total = total.checked_add(self.size_of_at(field, depth + 1)?)?;
                }
                let alignment = self.align_of_at(ty, depth + 1)?;
                Some(round_up(total, alignment))
            }
            // A pointer is the target's width, which is what makes
            // `sizeof(Array<int>)` computable: the type is a pointer and two
            // `usize`es, and all three have known sizes.
            Type::Pointer { .. } | Type::Reference { .. } => {
                Some(i64::from(self.pointer_bits.div_ceil(8)))
            }
            other => other.bits().map(|bits| i64::from(bits.div_ceil(8))),
        }
    }

    /// Returns the alignment in bytes of a type.
    ///
    /// A struct is aligned to its most-aligned field, an array to its element,
    /// and a primitive to its own size rounded down to a power of two -- the
    /// rule C uses, and the one every field's offset depends on.
    pub(super) fn align_of(&self, ty: &Type) -> Option<i64> {
        if ty.unqualified().is_incomplete() {
            return None;
        }
        self.align_of_at(ty, 0)
    }

    pub(super) fn align_of_at(&self, ty: &Type, depth: u32) -> Option<i64> {
        if depth > 32 {
            return None;
        }
        match ty {
            Type::Volatile(inner) | Type::Const(inner) => self.align_of_at(inner, depth + 1),
            Type::Array { element, .. } | Type::UnsizedArray(element) => {
                self.align_of_at(element, depth + 1)
            }
            Type::Nominal(nominal) => {
                let nominal = self.complete_nominal(nominal);
                let mut alignment: i64 = 1;
                for (name, field) in &nominal.fields {
                    alignment =
                        alignment.max(self.field_alignment(&nominal, name, field, depth + 1)?);
                }
                Some(alignment)
            }
            Type::Pointer { .. } | Type::Reference { .. } => {
                Some(i64::from(self.pointer_bits.div_ceil(8)))
            }
            other => {
                let size = other.bits().map(|bits| i64::from(bits.div_ceil(8)))?;
                // The largest power of two not exceeding the size, capped at
                // sixteen bytes: an alignment wider than the machine's widest
                // move buys nothing.
                let mut alignment: i64 = 1;
                while alignment * 2 <= size && alignment < 16 {
                    alignment *= 2;
                }
                Some(alignment)
            }
        }
    }

    /// Returns the alignment a field is placed with.
    ///
    /// The field's own alignment, raised to what its `alignas` asked for: an
    /// alignment may only be raised, which is why the larger of the two is the
    /// answer rather than the request. The request is recorded against the
    /// *owner's* declaration, so the owner is passed in rather than read off
    /// the field's type.
    pub(super) fn field_alignment(
        &self,
        owner: &crate::types::NominalType,
        name: &str,
        field: &Type,
        depth: u32,
    ) -> Option<i64> {
        let natural = self.align_of_at(field, depth)?;
        let requested = owner
            .field_alignments
            .iter()
            .find(|(field, _)| field == name)
            .and_then(|(_, value)| i64::try_from(*value).ok())
            .unwrap_or(1);
        Some(natural.max(requested))
    }

    /// Returns a nominal type with its fields, filling in a shallow entry.
    ///
    /// A type registers its name before its fields are resolved, so a field may
    /// name the type being declared -- `next *Node` inside `Node`. What that
    /// self-reference records is the name; the fields live in the table, and a
    /// shallow entry is completed from it here.
    pub(super) fn complete_nominal(
        &self,
        nominal: &crate::types::NominalType,
    ) -> crate::types::NominalType {
        if !nominal.fields.is_empty() {
            return nominal.clone();
        }
        if let Some(complete) = self.nominal_types.get(&nominal.name) {
            return complete.clone();
        }
        let last = nominal.name.rsplit_once('.').map(|(_, last)| last);
        last.and_then(|last| self.nominal_types.get(last))
            .cloned()
            .unwrap_or_else(|| nominal.clone())
    }
}

/// The message of a declaration's `@deprecated`, if it has the attribute.
///
/// `Some("")` means deprecated with nothing to add. A message is a string
/// literal, and what it says is the source text between the quotes with its
/// escapes decoded.
#[must_use]
pub fn deprecated_message(ast: &Ast, attrs: &[fallc_ast::Attr]) -> Option<String> {
    let attr = attrs.iter().find(|attr| attr.name == "deprecated")?;
    let message = attr
        .arguments
        .first()
        .and_then(|argument| match &ast.expr(*argument).kind {
            ExprKind::StringLiteral(text) => {
                let body = fallc_ast::literal::string_body(text);
                Some(
                    String::from_utf8_lossy(&fallc_ast::literal::decode_escapes(body)).into_owned(),
                )
            }
            _ => None,
        });
    Some(message.unwrap_or_default())
}
