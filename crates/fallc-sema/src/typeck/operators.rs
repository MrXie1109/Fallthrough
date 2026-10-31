//! Special methods and the access forms around them.
//!
//! Operators, indexing, text conversion, assignment, compound assignment, and
//! the lvalue rules that decide whether a place can be written at all.

use super::*;

/// Which call site a pack instantiation is recorded for.
///
/// A value that defines `__call__` and an ordinary method call resolve their
/// receiver in different places, so the symbol is recorded in a different table;
/// the rest of the expansion is the same.
#[derive(Clone, Copy)]
pub(super) enum PackCallKind {
    /// `value(args)`: the receiver is the callee expression.
    ValueCall,
    /// `value.method(args)`: the receiver is the field's object.
    Method,
}

impl TypeChecker {
    /// The signature, symbol, and types of a special method on `receiver`.
    ///
    /// A special method of a type *family* is declared once -- `__inc__(this
    /// &ArrayIterator<T>)` -- and used at a member, `ArrayIterator<int>`. The
    /// substitution installed here is the member's, so the symbol is the one
    /// the instantiation emits, `&T` means `&int`, and the body is emitted for
    /// the member that uses it. Without this the symbol named the family's own
    /// `T`, which no definition has.
    pub(super) fn special_method_call(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
        written: &str,
        params_wanted: usize,
    ) -> Option<SpecialMethodCall> {
        self.special_method_calls(resolutions, receiver, written, params_wanted)
            .into_iter()
            .next()
    }

    /// Every overload of a special method the receiver's type declares.
    ///
    /// An operator may have several: a reverse iterator subtracts an offset and
    /// also subtracts another position, and which one is meant is decided by the
    /// argument's type rather than by the name. Returning only the first one
    /// meant the second was unreachable, so `r - q` was refused however the
    /// method was written.
    pub(super) fn special_method_calls(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
        written: &str,
        params_wanted: usize,
    ) -> Vec<SpecialMethodCall> {
        let Some(receiver_name) = receiver.nominal_name().map(str::to_owned) else {
            return Vec::new();
        };
        let qualified = Self::special_method_qualified(&receiver_name, written);
        let signatures: Vec<crate::resolve::FunctionSignature> = self
            .definitions_named(resolutions, &qualified)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .filter(|signature| signature.param_types.len() == params_wanted)
            .collect();
        let mut calls = Vec::with_capacity(signatures.len());
        for signature in signatures {
            let member_substitution = self.member_substitution(receiver.nominal_name());
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let param_types: Vec<Type> = signature
                .param_types
                .iter()
                .map(|syntax| self.resolve_type_syntax(syntax, true))
                .collect();
            let params: Vec<String> = param_types.iter().map(crate::types::mangle_type).collect();
            let returns = self.signature_result_type(&signature);
            self.substitution = previous;
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            if let Some(substitution) = &member_substitution {
                self.record_method_instantiation(&signature, substitution, &symbol, receiver);
            }
            calls.push(SpecialMethodCall {
                signature,
                param_types,
                returns,
                symbol,
            });
        }
        calls
    }

    /// Resolves a unary operator method on a type and returns what it yields.
    ///
    /// Records the call so code generation emits it, and returns the method's
    /// own return type, which is what the expression has.
    pub(super) fn resolve_unary_result(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
        method: fallc_ast::special::SpecialMethod,
        span: fallc_span::Span,
        ast: &Ast,
    ) -> Option<Type> {
        let call = self.special_method_call(resolutions, receiver, method.name(), 1)?;
        self.unary_methods.insert(span, call.symbol);
        self.note_throwing_call(span, &call.signature, ast, None);
        Some(call.returns)
    }

    /// Finds a special method on a type and returns the symbol it is emitted
    /// under, if its parameters fit.
    ///
    /// # One helper for several methods
    ///
    /// `__bool__`, `__int__`, `__float__`, `__bytes__` and a no-argument
    /// `__call__` all have the same shape: a receiver and nothing else that the
    /// call site has to supply. They are resolved by one routine rather than
    /// five, because the only thing that differs is the name.
    ///
    /// The symbol is built from the signature's own parameter types, so it is
    /// exactly what the definition emitted -- the receiver's declared pointer
    /// form included, which the call site cannot know from the object alone.
    pub(super) fn resolve_nullary_method(
        &mut self,
        resolutions: &Resolutions,
        receiver: &Type,
        method: fallc_ast::special::SpecialMethod,
    ) -> Option<String> {
        // Only the receiver: one extra parameter would mean the method wants an
        // argument this call cannot supply.
        self.special_method_call(resolutions, receiver, method.name(), 1)
            .map(|call| call.symbol)
    }

    /// Types `str(x)` and `repr(x)`.
    ///
    /// # The rule
    ///
    /// Asking for a value's text is a question put to the value, not to the
    /// function: a type that declares `__str__` answers it, and a type that
    /// declares `__repr__` answers the other. Neither is required. A type that
    /// declares neither is rendered by the compiler, which knows how to print
    /// numbers, characters, pointers, and the aggregate kinds.
    ///
    /// The decision is recorded with the call, so code generation emits either
    /// the method call or its own rendering. It cannot make that choice later
    /// without repeating the type system's lookup.
    ///
    /// # Why the receiver is a pointer
    ///
    /// `__str__` is declared as `fn __str__(this *const T) String` so that a
    /// value is rendered where it lies rather than copied. A method call takes
    /// the address automatically, and a `const` receiver accepts both const and
    /// mutable objects, so one declaration covers every value.
    pub(super) fn type_of_text_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        name: &str,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let method = if name == "str" {
            fallc_ast::special::SpecialMethod::Str
        } else {
            fallc_ast::special::SpecialMethod::Repr
        };

        // One argument, exactly: there is nothing else `str` could take.
        if node.args.len() != 1 {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{name}` takes 1 argument, but {} were given",
                    node.args.len()
                ))
                .with_code(DiagnosticCode::sema(36))
                .with_span(span),
            );
            return None;
        }

        let argument = node.args[0];
        let previous = self.expected.take();
        let found = self.infer_expr(ast, resolutions, types, argument);
        self.expected = previous;
        let found = found?;

        // The value being rendered has to have a type of its own: an integer
        // literal with no expectation is whichever integer fits, which is fine,
        // but a value whose type was never determined cannot be rendered.
        //
        // A reference is rendered as what it refers to, because reading it is
        // what a reader would expect -- `str(r)` for `r: &T` is `str(*r)`.
        let rendered = found.referent().cloned().unwrap_or_else(|| found.clone());

        // `str(x)` asks for `__str__`, and falls back to `__repr__` when the
        // type declares none: a type that writes only `__repr__` still has a
        // `str`, which is what Appendix B states and what a reader expects.
        let mut method = method;
        let mut resolved = self.resolve_nullary_method(resolutions, &rendered, method);
        if resolved.is_none() && method == fallc_ast::special::SpecialMethod::Str {
            method = fallc_ast::special::SpecialMethod::Repr;
            resolved = self.resolve_nullary_method(resolutions, &rendered, method);
        }
        // The method's return type is what the call has, which lets a `__str__`
        // return something other than the standard string.
        let result = resolved.and_then(|symbol| {
            let receiver_name = rendered.nominal_name()?.to_owned();
            let qualified = Self::special_method_qualified(&receiver_name, method.name());
            let signature = self
                .definitions_named(resolutions, &qualified)
                .into_iter()
                .find_map(|def| def.signature.clone())?;
            self.text_calls.insert(span, Some(symbol));
            self.note_throwing_call(span, &signature, ast, None);
            Some(self.signature_result_type(&signature))
        });

        if let Some(result) = result {
            return Some(result);
        }
        // Without a method the compiler renders the value itself, and what it
        // can write down is a finite list: the scalars, a C string, and a
        // pointer. Anything else -- a declared type with no `__str__` -- would
        // reach code generation as a request nothing can answer, which is an
        // internal error rather than a diagnostic the program can act on.
        // The defaults, in the order the standard library states them:
        //
        // * `str(x)` with no `__str__` is `repr(x)`, so a type that declares
        //   only `__repr__` still has a `str`;
        // * `repr(x)` with no `__repr__` is the compiler's own rendering, which
        //   writes `<Type object at 0x...>` for a declared type -- the same
        //   answer every language gives when nothing better is available.
        //
        // So a declared type is never "unwritable": it has a `repr`, and
        // therefore a `str`, whatever it declares.
        if !Self::renderable(&rendered) && method == fallc_ast::special::SpecialMethod::Str {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "a value of type `{}` cannot be written down: declare `__{}__` for it",
                    rendered.spelling(),
                    method.name().trim_matches('_')
                ))
                .with_code(DiagnosticCode::sema(182))
                .with_span(span)
                .with_note(format!(
                    "`{name}` writes the scalars, characters, C strings and pointers itself, \
                     and asks a type for `{}` when it has one",
                    method.name()
                )),
            );
            return None;
        }
        // Without a method the compiler renders the value itself and the result
        // is text, which is the standard string.
        self.text_calls.insert(span, None);
        self.standard_string_type().or_else(|| {
            // A program that never imported the string module has no
            // string type, so `str(x)` cannot produce one. That is a real
            // error rather than an unanswered question: the fix is to
            // import it, and the diagnostic says so.
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{name}` produces text, which needs `std.String`, but the string \
                         module is not imported"
                ))
                .with_code(DiagnosticCode::sema(65))
                .with_span(span)
                .with_note("add `import std;` or `import std.string;`"),
            );
            None
        })
    }

    /// Whether the compiler can write a value of this type down itself.
    ///
    /// The kinds it knows how to render are the ones whose meaning is not a
    /// program's to define: `bool`, the integers, characters, floats, pointers,
    /// and a C string, which is its own text.
    fn renderable(ty: &Type) -> bool {
        match ty.unqualified() {
            Type::Bool
            | Type::Byte
            | Type::Int(_)
            | Type::Character(_)
            | Type::Float(_)
            | Type::Pointer { .. }
            | Type::Reference { .. }
            | Type::NullPointer => true,
            Type::Array { element, .. } => matches!(
                element.as_ref().unqualified(),
                Type::Character(crate::types::CharType::C8)
            ),
            _ => false,
        }
    }

    /// The declared standard string type.
    ///
    /// Found by its last name segment, because the same type is `std.String` in
    /// a file that imported the umbrella module and `String` inside the module
    /// that declares it. Both are one type, and which spelling is in the table
    /// depends only on how this file came to know it.
    pub(super) fn standard_string_type(&self) -> Option<Type> {
        self.nominal_types
            .values()
            .find(|nominal| nominal.name.rsplit('.').next().unwrap_or(&nominal.name) == "String")
            .map(|nominal| Type::Nominal(nominal.clone()))
    }

    /// Types `a[i]`.
    ///
    /// # The rule
    ///
    /// An array or a pointer is indexed directly: the result is the element
    /// type, and the index may be any integer. Nothing else has a subscript
    /// built in, so any other type is asked for `__getitem__`, whose return
    /// type is what the expression has.
    ///
    /// The two are told apart by the object's type rather than by looking for a
    /// method first, because an array of a type that also defines `__getitem__`
    /// would otherwise be indexed by the method, which is not what `a[i]` means
    /// for an array.
    pub(super) fn type_of_index(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &fallc_ast::IndexExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // `values[1]` -- one element of a parameter pack. A pack has no
        // storage to subscript: its elements are compile-time values, so the
        // subscript has to be one too, and what it selects is emitted as the
        // number it is.
        if let fallc_ast::ExprKind::Name(name) = &ast.expr(node.object).kind
            && matches!(
                self.substitution
                    .as_ref()
                    .and_then(|substitution| substitution.get(name)),
                Some(crate::template::TemplateArgument::Pack(_))
            )
        {
            return self.type_of_pack_index(ast, node, name, span);
        }

        let previous = self.expected.take();
        let object = self.infer_expr(ast, resolutions, types, node.object);
        let index = self.infer_expr(ast, resolutions, types, node.index);
        self.expected = previous;

        let (Some(object), Some(index)) = (object, index) else {
            return None;
        };
        // A reference is indexed through itself: `c &[N][N]double` is an array
        // to subscript, not an address. What the subscript needs is the address
        // the reference holds, so the object is *not* recorded as a dereference
        // read: a read loads the referent's value, while indexing needs the
        // referent's address.
        let object = object.referent().cloned().unwrap_or(object);

        // An array's element, or a pointer's pointee.
        let element = match &object {
            Type::Array { element, .. } => Some(element.unqualified().clone()),
            Type::Pointer { pointee, .. } => Some(pointee.unqualified().clone()),
            Type::UnsizedArray(element) => Some(element.unqualified().clone()),
            _ => None,
        };
        if let Some(element) = element {
            // The language indexes memory by `usize` and nothing else, for the
            // reasons `usize` exists: it cannot be negative, and it does not
            // truncate on the way to an address. An unsuffixed literal adopts
            // it, as it adopts every other expected type -- `a[0]` is the
            // ordinary case, and it reads as the 0 that `usize` calls zero.
            let literal_fits = self.is_default_int_literal(ast, node.index);
            if literal_fits {
                self.adopted_types
                    .insert(ast.expr(node.index).span, Type::USIZE);
            } else if !index.is_integer() {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "an index must be an integer, but this is `{}`",
                        index.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(66))
                    .with_span(ast.expr(node.index).span),
                );
                return None;
            } else if index != Type::USIZE {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is indexed by `usize`, but `{}` was given",
                        object.spelling(),
                        index.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(66))
                    .with_span(ast.expr(node.index).span)
                    .with_note("there are no implicit conversions; convert with `as`"),
                );
                return None;
            }
            return Some(element);
        }

        // Anything else needs `__getitem__`. Keyed by the whole expression's
        // span, which is what code generation has when it lowers this node.
        match self.resolve_index_method(ast, resolutions, &object, &index, node.index, span) {
            IndexResolution::Found(result) => return Some(result),
            // The type has `__getitem__`, and the subscript is what did not
            // fit. Saying "cannot be indexed" about it named the wrong thing:
            // the index is the part that is wrong, and the declared index type
            // is what the reader needs to hear.
            IndexResolution::IndexMismatch(expected) => {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}` is indexed by `{}`, but `{}` was given",
                        object.spelling(),
                        expected.spelling(),
                        index.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(66))
                    .with_span(span)
                    .with_note("there are no implicit conversions; convert with `as`"),
                );
                return None;
            }
            IndexResolution::NoMethod => {}
        }

        self.diagnostics.push(
            Diagnostic::error(format!("`{}` cannot be indexed", object.spelling()))
                .with_code(DiagnosticCode::sema(66))
                .with_span(span)
                .with_note(
                    "an array and a pointer are indexed directly; a type needs `__getitem__`",
                ),
        );
        None
    }

    /// Picks one element out of a parameter pack: `values[1]`.
    ///
    /// Which element it is has to be known before the program runs, for the
    /// same reason every other use of a pack is: the elements are compile-time
    /// values, and nothing names them at run time. The result is the number
    /// the chosen element is.
    pub(super) fn type_of_pack_index(
        &mut self,
        ast: &Ast,
        node: &fallc_ast::IndexExpr,
        name: &str,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let pack = fallc_ast::PackRef {
            pack: name.to_owned(),
            pack_span: ast.expr(node.object).span,
        };
        let elements = self.pack_elements(&pack, "indexed")?;
        // The element type is the type the parameter was declared with, so
        // `values[0]` is an `int` for `template <values ...int>`.
        let Some(element_type) = self.template_value_types.get(name).cloned() else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{name}` is a pack of types, so it has no value to index"
                ))
                .with_code(DiagnosticCode::sema(80))
                .with_span(span),
            );
            return None;
        };
        let Some(index) = self.const_eval_int(node.index) else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "the index into `{name}` must be a constant, because a pack's \
                     elements exist only before the program runs"
                ))
                .with_code(DiagnosticCode::sema(80))
                .with_span(ast.expr(node.index).span),
            );
            return None;
        };
        if index < 0 || index as usize >= elements.len() {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{name}[{index}]` is out of range: the pack has {} element(s)",
                    elements.len()
                ))
                .with_code(DiagnosticCode::sema(80))
                .with_span(span),
            );
            return None;
        }
        let element = elements[index as usize].clone();
        let value = self.pack_element_value(&pack, &element, "indexed as values")?;
        self.constant_values.insert(span, value);
        Some(element_type)
    }

    /// Finds `__getitem__` on a type, checking the subscript against it.
    pub(super) fn resolve_index_method(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        object: &Type,
        index: &Type,
        index_expr: fallc_ast::ExprId,
        span: fallc_span::Span,
    ) -> IndexResolution {
        let Some(receiver_name) = object.nominal_name().map(str::to_owned) else {
            return IndexResolution::NoMethod;
        };
        let written = fallc_ast::special::SpecialMethod::GetItem.name();
        let qualified = Self::special_method_qualified(&receiver_name, written);
        let mut expected_index: Option<Type> = None;
        // Matched on the last name segment, so an imported `__getitem__` is
        // found: a type defined in a module is reached from an importing file
        // under the module's qualified name.
        for def in self.definitions_named(resolutions, &qualified) {
            let Some(signature) = def.signature.clone() else {
                continue;
            };
            // The subscript is the second parameter, after the receiver.
            let Some(parameter) = signature.param_types.get(1) else {
                continue;
            };
            // The member's own arguments are in place before the parameter is
            // read: a method of `Array<int>` says `&T`, and what it means is
            // `&int`. The index is matched against the *member's* type, which
            // is the type a caller can check against.
            let member_substitution = self.member_substitution(object.nominal_name());
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let parameter_type = self.resolve_type_syntax(parameter, true);
            // An unsuffixed integer literal takes the parameter's type, as it
            // does everywhere else: `a[0]` on a type whose `__getitem__` takes a
            // `usize` is the ordinary case, and requiring the literal to
            // already be one rejected it.
            let literal_fits =
                self.is_default_int_literal(ast, index_expr) && parameter_type.is_integer();
            // A parameter written `&const K` takes a value as its address,
            // exactly as a call argument does -- that is what makes
            // `ages[String("Alan")]` writeable. Without this the subscript was
            // reported as the wrong type even though the same value fits the
            // same parameter in `ages.find(String("Alan"))`.
            let indirect_fits =
                !literal_fits && self.operand_fits_indirect(index, &parameter_type).is_some();
            if !self.is_compatible(index, &parameter_type) && !literal_fits && !indirect_fits {
                // Remembered, so that a type *with* `__getitem__` is not
                // reported as one that cannot be indexed at all.
                expected_index = Some(parameter_type);
                self.substitution = previous;
                continue;
            }
            if literal_fits {
                self.adopted_types
                    .insert(ast.expr(index_expr).span, parameter_type.clone());
            }
            if indirect_fits {
                // The value is passed by its address, which code generation
                // takes when it sees this record.
                self.by_address_arguments.insert(ast.expr(index_expr).span);
            }
            let returns = self.signature_result_type(&signature);
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            self.substitution = previous;
            // The symbol is recorded so code generation emits the call rather
            // than reaching into memory: `a[i]` on a user type is a method
            // call, and only the checker knows which overload it selected.
            //
            // Every parameter is mangled from the signature's own written
            // types, which is exactly how the definition built its symbol --
            // including the receiver, whose pointer form is the declaration's
            // and not the object's.
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            // A `__getitem__` returning a reference names storage, and reading
            // it as a value is a fact about the *use* -- which the surrounding
            // expectation decides, exactly as it does for `*p`.
            if !self
                .expected
                .as_ref()
                .is_some_and(|wanted| wanted.is_indirection())
                && let Some(referent) = returns.referent().cloned()
            {
                self.deref_reads.insert(span, referent);
            }
            // A method of a templated type is emitted only when some member
            // uses it, exactly as a constructor is.
            if let Some(substitution) = &member_substitution {
                self.record_method_instantiation(&signature, substitution, &symbol, object);
            }
            self.index_methods.insert(span, symbol);
            self.note_throwing_call(span, &signature, ast, None);
            return IndexResolution::Found(returns);
        }
        match expected_index {
            Some(expected) => IndexResolution::IndexMismatch(expected),
            None => IndexResolution::NoMethod,
        }
    }

    /// Finds an `__init__` that accepts `argument`, and records the call.
    ///
    /// Returns the symbol when one applies. The argument is checked as though
    /// it were the constructor's second parameter -- the first is the receiver,
    /// which the binding supplies itself.
    ///
    /// A type with no constructor, or none that fits, returns `None` and the
    /// caller falls back to an ordinary type check.
    pub(super) fn init_overload_for(
        &mut self,
        target: &Type,
        argument: fallc_ast::ExprId,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
    ) -> Option<String> {
        let receiver_name = target.nominal_name()?.to_owned();
        // The *last segment* of the name: a special method is written
        // `fn __init__(this *String, ..)` inside its own module, so it is bound
        // as `__init__String` however the importer spells the type. Looking up
        // `__init__std.String` found nothing.
        //
        // A template's member is named `Box<int>`, and the method that builds
        // it is written `__init__` on `Box<T>` -- so what it is *bound* under
        // is `__init__Box` with the parameter names, not the arguments. Taking
        // the whole member name looked for `__init__Box<int>`, which no
        // definition has.
        let bare = match receiver_name.split_once('<') {
            Some((base, _)) => base,
            None => receiver_name
                .rsplit_once('.')
                .map_or(receiver_name.as_str(), |(_, last)| last),
        };
        // An initialiser converts implicitly only through `__im_init__`;
        // `__init__` is the explicit constructor. A same-type initialiser is
        // copy or move construction rather than a conversion, so the explicit
        // copy/move constructors may serve it too; that is settled after the
        // argument's type is known.
        let qualified = format!("__init__{bare}");
        // An imported definition is bound under its qualified name --
        // `std.__init__String` -- so the comparison is on the last segment,
        // which is the name the method was written with. The fallback table is
        // consulted here too: inside an `Array<T>` body the constructor being
        // found is the substituted `T`'s, which may belong to another module.
        let candidates: Vec<FunctionSignature> = self
            .definitions_named(resolutions, &qualified)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .collect();
        if candidates.is_empty() {
            return None;
        }

        // The argument's type is inferred with no expectation, then matched
        // against each candidate's parameters after the receiver.
        let previous = self.expected.take();
        let saved_flag = self.in_parameter_position;
        self.in_parameter_position = false;
        let argument_ty = self.infer_expr(ast, resolutions, types, argument);
        self.in_parameter_position = saved_flag;
        self.expected = previous;
        let argument_ty = argument_ty?;
        // Copy or move construction is what an explicit `__init__` may serve,
        // even though the initialiser's type is a reference to the target
        // rather than the target itself: `String b = move(a)` passes `&&String`
        // and `String b = a` passes `&String`. Anything else must go through
        // `__im_init__`.
        let same_type = argument_ty == *target
            || argument_ty
                .referent()
                .is_some_and(|referent| referent == target);
        let candidates: Vec<FunctionSignature> = candidates
            .into_iter()
            .filter(|candidate| same_type || candidate.written_name.starts_with("__im_init__"))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        // `T x = T(..);` is not a constructor call: the initialiser already
        // *is* a `T`, built by the call it names, and the binding holds it.
        // Passing it on to `__init__(this &T, other T)` would copy a temporary
        // that was just made for this binding -- and for the copy constructor
        // of a family like `Array<T>` that meant the receiver and the source
        // were both the new storage, so nothing was copied at all.

        // `T x = T(..);` is not a constructor call: the initialiser already
        // *is* a `T`, built by the call it names, and the binding holds it.
        // Passing it on to `__init__(this &T, other T)` would copy a temporary
        // that was just made for this binding -- and for the copy constructor
        // of a family like `Array<T>` that meant the receiver and the source
        // were both the new storage, so nothing was copied at all.
        if self
            .constructor_calls
            .contains_key(&ast.expr(argument).span)
            && argument_ty == *target
        {
            return None;
        }

        // The member's arguments, which the *binding's* type names: `Box<int>
        // b = 42` reaches the constructor written against `Box<T>`.
        let member_substitution = self.member_substitution(target.nominal_name());

        // The initialiser *is* the constructor's argument, so it stands in
        // parameter position for as long as the candidates are compared. It was
        // restored to the enclosing value above -- correct for inferring the
        // argument, since `T x = y` does not take `y`'s address for `x` -- but
        // leaving it that way made `operand_fits_indirect` unreachable and so
        // rejected every constructor whose parameter is a reference:
        // `Array<int> copy = numbers;` never reached
        // `__init__(this &Array<T>, other &const Array<T>)`, and silently
        // stored the source's fields instead, sharing its buffer.
        let previous_flag = self.in_parameter_position;
        self.in_parameter_position = true;
        let chosen = self.choose_init_candidate(
            target,
            &candidates,
            &argument_ty,
            &member_substitution,
            ast,
            argument,
        );
        self.in_parameter_position = previous_flag;
        chosen
    }

    /// Picks the constructor among `candidates` that accepts `argument_ty`.
    ///
    /// Split out so that the comparison runs with `in_parameter_position` set:
    /// the flag governs how a value reaches an indirect parameter, and it has
    /// to be restored on every path out, including the one that finds nothing.
    /// Whether an argument is a *value* rather than a place.
    ///
    /// A name, an index, and a dereference all name storage, so the value keeps
    /// existing after the call and only a copy is safe. A call or a
    /// construction produces a value nothing else can name, which is what lets
    /// a `&&T` parameter take it.
    pub(super) fn argument_is_dying(ast: &Ast, expr: ExprId) -> bool {
        matches!(
            &ast.expr(expr).kind,
            ExprKind::Call(_) | ExprKind::TypedInit(_) | ExprKind::NestedInit(_)
        )
    }

    pub(super) fn choose_init_candidate(
        &mut self,
        target: &Type,
        candidates: &[FunctionSignature],
        argument_ty: &Type,
        member_substitution: &Option<crate::template::Substitution>,
        ast: &Ast,
        argument: fallc_ast::ExprId,
    ) -> Option<String> {
        // A dying argument prefers a parameter that takes it by rvalue
        // reference: `String b = make();` moves the buffer, and only falls back
        // to copying when the type offers no move.
        let dying = Self::argument_is_dying(ast, argument);
        let mut candidates: Vec<FunctionSignature> = candidates.to_vec();
        if dying {
            // Stable: among equals the declaration order still decides.
            candidates.sort_by_key(|signature| {
                let is_rvalue = signature.param_types.get(1).is_some_and(|written| {
                    matches!(
                        written,
                        fallc_ast::TypeSyntax::RvalueReference(_)
                            | fallc_ast::TypeSyntax::ConstRvalueReference(_)
                    )
                });
                // `true` (an rvalue parameter) sorts first.
                !is_rvalue
            });
        }
        for signature in &candidates {
            let parameter = signature.param_types.get(1).cloned();
            let Some(parameter) = parameter else {
                continue;
            };
            // A candidate whose own types mention the declaring family's
            // parameters is only a constructor of a member when those
            // parameters are bound by the target. Without that, its written
            // `Pair<A, B>` resolves with the arguments unresolved and looks
            // like a constructor of an unrelated type that happens to share
            // the name.
            if !signature.template_params.is_empty() && member_substitution.is_none() {
                continue;
            }
            // A constructor taking only the receiver cannot be this one: the
            // initialiser supplied a value it has nowhere to put.
            if signature.param_types.len() != 2 {
                continue;
            }
            // Resolved with both substitutions in place: the member's
            // arguments, which name the type being built, and the method's own
            // template parameters, which the argument decides. Resolving
            // without them reported every use of a parameter as an unknown
            // type.
            let previous = self.substitution.clone();
            if let Some(substitution) = &member_substitution {
                self.substitution = Some(substitution.clone());
            }
            let mut parameter_type = self.resolve_type_syntax(&parameter, true);
            // The candidate has to be a constructor *of this type*, not merely
            // one filed under the same name. Two families may share a name --
            // a program's own `Pair` and `std.tuple`'s -- and the imported
            // constructor's written `Pair<A, B>` would otherwise resolve with
            // its arguments unresolved and look like a constructor of the
            // local `Pair`.
            let receiver_written = &signature.param_types[0];
            let receiver = self.resolve_type_syntax(receiver_written, true);
            // One type may be known by two names: `String` where it is
            // declared, `std.String` in a file that imported the umbrella. The
            // receiver was written with one of them and the target was written
            // with the other, and they are the same type -- comparing them with
            // `==` made every constructor of an imported type reject the type
            // itself.
            let receiver_referent = receiver.referent().unwrap_or(&receiver);
            let same_receiver = self.same_type_named_apart(receiver_referent, target);
            self.substitution = previous;
            if !same_receiver {
                continue;
            }
            // A parameter that is still a bare template parameter is deduced
            // from the argument, exactly as a free template's is: `v T` with an
            // `int` given means `T` is `int`.
            if parameter_type.is_unspecified()
                || parameter_type == Type::unspecified("", parameter.span())
            {
                parameter_type = argument_ty.clone();
            }
            // `is_compatible` accepts the array decay a string literal needs:
            // `"hello"` is `[6]const c8` and the parameter is `*const c8`. It
            // also accepts a value reaching a reference parameter, which is
            // what `String s = "hi"` uses: the constructor's receiver is
            // `&String` and the binding is the value it initialises.
            if !self.is_compatible(argument_ty, &parameter_type) {
                continue;
            }
            // The symbol is mangled from the *substituted* parameters, so that
            // `Box<int>`'s constructor is `__init__` on `&Box<int>` and `int`
            // -- not on `&Box<T>` and `T`, which is the template's own name and
            // names no function that exists.
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            self.substitution = previous;
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            // A constructor of a *templated* type is a function that does not
            // exist until something asks for it, exactly as an instantiated
            // free template is. Recording it here is what makes code generation
            // emit it -- and what makes two `Box<int>`s share one.
            if let Some(substitution) = &member_substitution {
                self.record_method_instantiation(signature, substitution, &symbol, argument_ty);
            }
            // Recorded, so code generation emits the constructor call rather
            // than trying to store the initialiser as-is. The constructor is
            // called at the initialiser's span, so that is where raising is
            // reported and where the duty to protect it is judged.
            self.init_calls
                .insert(ast.expr(argument).span, symbol.clone());
            self.note_throwing_call(ast.expr(argument).span, signature, ast, None);
            // The initialiser stands where the constructor's second parameter
            // is, so an indirect one takes its address exactly as an ordinary
            // call's argument would: `Array<int> copy = numbers;` passes
            // `&numbers`, not the struct. Without this the call handed LLVM a
            // value where the function takes a pointer, and it refused to
            // verify the module.
            self.note_indirect_argument(argument, argument_ty, &parameter_type, ast);
            return Some(symbol);
        }
        None
    }

    /// Types an assignment.
    ///
    /// The target's type and the value's type must match exactly. The result is
    /// the assigned value's type, so `x = y` can be used as an expression.
    pub(super) fn type_of_assign(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &AssignExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let target_ty = self.infer_lvalue(ast, resolutions, types, node.target);
        let target_ty = target_ty?;
        // Writing through a reference writes what it refers to, so the type
        // that answers `__assign__` and the operators is the referent's. Asking
        // the reference itself found no method, and the assignment then fell
        // through to a member-wise store -- which for a type that owns a buffer
        // duplicates the pointer, and for a type with a throwing `__assign__`
        // dropped the exception on the floor.
        let assigned_ty = target_ty
            .referent()
            .cloned()
            .unwrap_or_else(|| target_ty.clone());

        // A `const` binding cannot be assigned to.
        if let ExprKind::Name(name) = &ast.expr(node.target).kind
            && let Some(def) = resolutions.defs.iter().find(|def| def.name == *name)
            && self.const_bindings.contains(&def.name_span)
        {
            self.diagnostics.push(
                Diagnostic::error(format!("cannot assign to `const` binding `{name}`"))
                    .with_code(DiagnosticCode::sema(42))
                    .with_span(ast.expr(node.target).span),
            );
            return None;
        }

        // The value is inferred first, because a compound assignment on a
        // user-defined type is a method call and the method has to be chosen by
        // both operands.
        //
        // A compound assignment on a pointer is pointer arithmetic, so the
        // value is an offset and is inferred as the `usize` every offset is.
        let offset_compound = target_ty.is_pointer()
            && matches!(
                node.op,
                Some(fallc_ast::BinaryOp::Add | fallc_ast::BinaryOp::Sub)
            );
        let previous = self.expected.replace(if offset_compound {
            Type::USIZE
        } else {
            target_ty.clone()
        });
        let value = self.infer_expr(ast, resolutions, types, node.value);
        self.expected = previous;
        let value_ty = value?;
        // The value is read through a reference too: `t = u` stores what `u`
        // names, not the address it holds. A reference that reached the
        // comparison unread looked like an argument of type `&T` for a
        // parameter of type `&const T`, which is not what a value position
        // means, so no `__assign__` matched and the store became member-wise.
        let mut value_ty = match value_ty.referent() {
            Some(referent) => {
                self.deref_reads
                    .insert(ast.expr(node.value).span, referent.clone());
                referent.clone()
            }
            None => value_ty,
        };
        // A pointer is an address *type*, so the expectation "an address is
        // wanted" suppressed the read of `x *Node = *p;`-shaped values here and
        // stored the address of the slot instead of the pointer in it. The
        // assignment wants the value, exactly as a pointer binding does.
        if target_ty.is_pointer() && value_ty.is_reference() {
            self.deref_reads
                .insert(ast.expr(node.value).span, target_ty.clone());
        }

        if let Some(op) = node.op {
            // A pointer is moved by an offset, exactly as `p = p + n` would
            // move it. The pointer keeps its qualifiers; only the address
            // changes.
            if target_ty.is_pointer() {
                if matches!(op, fallc_ast::BinaryOp::Add | fallc_ast::BinaryOp::Sub) {
                    if value_ty == Type::USIZE {
                        return Some(target_ty);
                    }
                    if value_ty.is_integer() {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "`{}=` offsets `{}` by `usize`, but `{}` was given",
                                op.spelling(),
                                target_ty.spelling(),
                                value_ty.spelling()
                            ))
                            .with_code(DiagnosticCode::sema(32))
                            .with_span(span)
                            .with_note("there are no implicit conversions; write `as usize`"),
                        );
                        return None;
                    }
                }
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "`{}=` cannot be applied to `{}`",
                        op.spelling(),
                        target_ty.spelling()
                    ))
                    .with_code(DiagnosticCode::sema(33))
                    .with_span(span)
                    .with_note(
                        "a pointer may be offset with `+=` and `-=` and moved with `++` and `--`",
                    ),
                );
                return None;
            }
            // A compound assignment is an operation, so the operator must be
            // applicable to the operands before the store happens.
            if assigned_ty.is_numeric() {
                if value_ty != assigned_ty {
                    self.types_must_match(&assigned_ty, &value_ty, span, "assignment");
                    return None;
                }
                return Some(target_ty);
            }
            // `a += b` is `__iadd__` when the type defines one, and `a = a + b`
            // otherwise -- which the binary operator path already handles, so
            // only the in-place method is looked for here.
            if self
                .resolve_in_place_method(
                    resolutions,
                    &assigned_ty,
                    op,
                    &value_ty,
                    node.value,
                    ast,
                    span,
                )
                .is_some()
            {
                return Some(target_ty);
            }
            // No in-place form: fall back to `a = a + b`, which the checker
            // accepts when the type defines `__add__` with an operand of this
            // type.
            //
            // The decision is recorded under this statement's span, because
            // that is the span code generation lowers: it emits the store and
            // the operator call together, not as a separate binary expression.
            if let Some(method) = self.resolve_operator_method_for(
                op,
                &assigned_ty,
                &value_ty,
                resolutions,
                ast,
                types,
                span,
            ) {
                self.operator_methods.insert(span, method);
                return Some(target_ty);
            }
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}=` cannot be applied to `{}`",
                    op.spelling(),
                    target_ty.spelling()
                ))
                .with_code(DiagnosticCode::sema(33))
                .with_span(span)
                .with_note(format!(
                    "a type defines `{}=` with `{}` or `{}`",
                    op.spelling(),
                    op.spelling(),
                    op.spelling()
                )),
            );
            return None;
        }

        // Compared against what is being written, which for a reference target
        // is its referent: `t = u` with both of them references compares the two
        // values, and only a genuine mismatch stops here. Comparing against the
        // reference itself refused every such assignment before the method
        // lookup below could run.
        if value_ty != assigned_ty {
            if super::methods::integer_promotes(&value_ty, &assigned_ty) {
                self.adopted_types
                    .insert(ast.expr(node.value).span, assigned_ty.clone());
            } else if self.try_implicit_conversion(
                resolutions,
                ast,
                types,
                node.value,
                &assigned_ty,
            ) {
                // The value now *is* the target type for the rest of the
                // assignment: method lookup and the store both use it.
                value_ty = assigned_ty.clone();
            } else {
                let previous = self.expected.take();
                let _ = previous;
                self.types_must_match(&assigned_ty, &value_ty, span, "assignment");
                return None;
            }
        }

        // A type that defines `__assign__` is assigned by calling it, which is
        // what lets a type own a resource: a member-wise copy of a string
        // duplicates its `_data` pointer, so two strings would share one buffer
        // and free it twice.
        //
        // The method is recorded like every other resolved call, so code
        // generation emits it rather than storing the value.
        if let Some(symbol) = self.resolve_assign_method(
            resolutions,
            &assigned_ty,
            &value_ty,
            node.value,
            ast,
            types,
            span,
        ) {
            self.assign_methods.insert(span, symbol);
            // The method reads the value through the address it is given, so
            // the value must not also be read through here: two loads leave the
            // call with a copy where a pointer is wanted.
            self.deref_reads.remove(&ast.expr(node.value).span);
        }
        Some(target_ty)
    }

    /// Finds a type's `__assign__` and returns the symbol it is emitted under.
    ///
    /// The parameter is checked against the value, so that a type may define
    /// one `__assign__` per kind of source. Returns `None` when the type has
    /// none, in which case the assignment is the implicit member-wise copy.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_assign_method(
        &mut self,
        resolutions: &Resolutions,
        target: &Type,
        value: &Type,
        value_expr: ExprId,
        ast: &Ast,
        types: &TypeTable,
        span: fallc_span::Span,
    ) -> Option<String> {
        // The target *itself*, not what it points at: a pointer has no
        // `__assign__`, and the type behind it is not what is being assigned.
        let receiver_name = target.receiver_base_name()?.to_owned();
        // The *member's* arguments -- `Array<int>`'s `T` is `int` -- so that a
        // parameter written `&const Array<T>` resolves as `&const Array<int>`.
        // Without this the `T` was no type at all, every candidate that takes
        // its source by reference failed to match, and `dst = src` fell through
        // to a plain struct store: the two arrays then shared one buffer, the
        // destination's own buffer leaked, and both destructors freed the same
        // pointer.
        let member_substitution = self.member_substitution(Some(&receiver_name));
        // The *family's* name, not the member's: the method is written
        // `__assign__(this &Array<T>, other &const Array<T>)` and bound as
        // `__assign__Array`, so `Array<i32>` has to be reduced to `Array`
        // before it names anything. Stripping only the module qualifier left
        // `__assign__Array<i32>`, which no definition is filed under -- so the
        // method was never found and `dst = src` became a member-wise store.
        let bare = match receiver_name.split_once('<') {
            Some((base, _)) => base,
            None => receiver_name
                .rsplit_once('.')
                .map_or(receiver_name.as_str(), |(_, last)| last),
        };
        let written = fallc_ast::special::SpecialMethod::Assign.name();
        let qualified = format!("{written}{bare}");
        // A dying value prefers the parameter that takes it by rvalue
        // reference, for the same reason a constructor does: `s = make();`
        // should move. Only the *shape* of the value decides this, because the
        // types are the same either way -- a `&&String` parameter accepts a
        // named `String` too, so without this ordering `b = a;` took the move
        // and left `a` empty. The sort is stable, so among equals the
        // declarations still decide.
        let mut defs = self.definitions_named(resolutions, &qualified);
        // A type this module declares shadows an imported type of the same
        // name, so the imported type's methods must not answer for it: the
        // two nominal types share a name, and the symbol would then be
        // mangled from the local type while the definition belongs to the
        // imported family. A type that is itself imported -- `Array<int>` from
        // `std`, written unqualified -- keeps its methods; only an actual
        // local declaration filters the imported candidates out.
        let target_is_local = resolutions.defs.iter().any(|definition| {
            definition.kind == crate::resolve::DefKind::Type
                && definition.imported_via.is_none()
                && definition.name == receiver_name
        });
        if target_is_local {
            defs.retain(|definition| definition.imported_via.is_none());
        }
        if Self::argument_is_dying(ast, value_expr) {
            defs.sort_by_key(|def| {
                let is_rvalue = def.signature.as_ref().is_some_and(|signature| {
                    signature.param_types.get(1).is_some_and(|written| {
                        matches!(
                            written,
                            fallc_ast::TypeSyntax::RvalueReference(_)
                                | fallc_ast::TypeSyntax::ConstRvalueReference(_)
                        )
                    })
                });
                // `true` (an rvalue parameter) sorts first.
                !is_rvalue
            });
        }
        for def in defs {
            let Some(signature) = def.signature.clone() else {
                continue;
            };
            let Some(parameter) = signature.param_types.get(1) else {
                continue;
            };
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let parameter_type = self.resolve_type_syntax(parameter, true);
            self.substitution = previous;
            // The value stands where the parameter is, so a value may reach a
            // reference parameter here exactly as it may at a call: `dst = src`
            // passes `&src`, and without the flag the comparison rejected every
            // `__assign__` whose source is taken by reference.
            let was_parameter = self.in_parameter_position;
            self.in_parameter_position = true;
            let ok = self.is_compatible(value, &parameter_type);
            self.in_parameter_position = was_parameter;
            if !ok {
                continue;
            }
            let previous = match &member_substitution {
                Some(substitution) => self.substitution.replace(substitution.clone()),
                None => self.substitution.take(),
            };
            let params: Vec<String> = signature
                .param_types
                .iter()
                .map(|written| {
                    let ty = self.resolve_type_syntax(written, true);
                    crate::types::mangle_type(&ty)
                })
                .collect();
            self.substitution = previous;
            let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
            // The source may reach the parameter by address -- `__assign__(this
            // &Array<T>, other &const Array<T>)` takes it by reference -- so the
            // call has to pass the address. Only the checker knows the parameter
            // is a reference, which is why the record is made here.
            if parameter_type.is_reference() {
                // The method takes the value's address, and a value that is
                // itself a reference already *is* that address: reading it
                // through first would pass the bytes of a loaded copy where the
                // method expects a pointer to them.
                //
                // The expression's own type is what decides, not the type the
                // assignment compared: `b = std.move(a)` has a `&&String` on the
                // right, whose value *is* the address of `a`, so taking its
                // address produced a pointer to the pointer -- and the callee
                // then read a length out of the slot. `types` is the checker's
                // own record of what the expression is.
                let expression = ast.expr(value_expr);
                let already_an_address =
                    types.get(value_expr).is_some_and(|ty| ty.is_indirection());
                if !already_an_address {
                    self.by_address_arguments.insert(expression.span);
                }
            } else {
                self.note_indirect_argument(value_expr, value, &parameter_type, ast);
            }
            // A probe asks whether this assignment could raise, so the answer
            // is recorded even though nothing is emitted.
            if self.probe_depth > 0 && signature.throws {
                self.probe_saw_throw = true;
            }
            // An assignment that calls `__assign__` is a call site like any
            // other: if the method may raise, the raise has to be handled here
            // or the enclosing function has to be the one that lets it out.
            self.note_throwing_call(span, &signature, ast, None);
            // A method of a templated type exists only once something asks for
            // it, so the assignment is what makes `__assign__Array<i32>` be
            // emitted at all. Without this the call had no definition to reach.
            if let Some(substitution) = &member_substitution {
                self.record_method_instantiation(&signature, substitution, &symbol, target);
            }
            return Some(symbol);
        }
        None
    }

    /// Finds a type's in-place method for a compound assignment.
    ///
    /// `__iadd__` is asked for `+=` before `__add__`, because the in-place form
    /// is the one that means "modify this"; when only `__add__` exists, `+=` is
    /// `a = a + b`, which is why an `i` form is never required.
    ///
    /// Returns the method's symbol and result type.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_in_place_method(
        &mut self,
        resolutions: &Resolutions,
        target: &Type,
        op: fallc_ast::BinaryOp,
        value: &Type,
        value_expr: fallc_ast::ExprId,
        ast: &Ast,
        span: fallc_span::Span,
    ) -> Option<(String, Type)> {
        // Which base method this operator is, then its in-place name.
        let base = fallc_ast::special::SpecialMethod::for_binary(op, false)?;
        let written = base.in_place_name()?;
        // Resolved through the same path as any other special method, so that a
        // type's *member* substitution applies: `__iadd__` of
        // `ArrayReverseIterator<T>` has to be looked at as the one of
        // `ArrayReverseIterator<int>`, or its parameter mangles with a `T` in it
        // and the symbol names a function that does not exist.
        let calls = self.special_method_calls(resolutions, target, written, 2);
        for call in calls {
            let signature = call.signature.clone();
            let Some(parameter_type) = call.param_types.get(1).cloned() else {
                continue;
            };
            // The same fitting rule the binary operator uses: a value fits a
            // pointer to it by address, and an unsuffixed literal takes the
            // parameter's type. Requiring exact compatibility meant
            // `s += "text"` reached `__iadd__(c8)` -- the first definition --
            // and was reported as a type error instead of finding the string
            // overload.
            let by_address = !self.is_compatible(value, &parameter_type)
                && self.operand_fits(value, &parameter_type);
            let literal_fits =
                self.is_default_int_literal(ast, value_expr) && parameter_type.is_integer();
            // A string literal is constructed into the parameter's type, which
            // is what makes `s += "text"` work -- the operand is an array of
            // characters and the method takes a `String`.
            // The type to construct is the one the parameter *points at*: the
            // method takes `*const String`, so the temporary handed to it must
            // be a `String`.
            let constructed = !literal_fits
                && !by_address
                && !self.is_compatible(value, &parameter_type)
                && parameter_type.pointee().is_some_and(|pointee| {
                    self.construct_from_literal(
                        resolutions,
                        ast,
                        pointee,
                        value_expr,
                        ast.expr(value_expr).span,
                    )
                });
            if !self.is_compatible(value, &parameter_type)
                && !by_address
                && !literal_fits
                && !constructed
            {
                continue;
            }
            if literal_fits {
                self.adopted_types
                    .insert(ast.expr(value_expr).span, parameter_type.clone());
            }
            // Whether the operand is handed over by address is a property of
            // the method, and code generation needs it to lower the call.
            //
            // A constructed literal counts: the parameter is a pointer to the
            // constructed type, so what is passed is the temporary's address.
            self.in_place_by_address
                .insert(span, by_address || constructed);
            let symbol = call.symbol.clone();
            let returns = call.returns.clone();
            self.in_place_methods.insert(span, symbol.clone());
            self.note_throwing_call(span, &signature, ast, None);
            return Some((symbol, returns));
        }
        None
    }

    /// Resolves the binary operator method for `+=`'s fallback, recording it.
    ///
    /// One implementation, used by both `a + b` and the `a += b` that means the
    /// same thing. There were two, and they disagreed: this one did not accept
    /// an operand passed by address, so `+=` on a type whose `__add__` takes
    /// `*const T` failed while `+` succeeded.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_operator_method_for(
        &mut self,
        op: fallc_ast::BinaryOp,
        lhs: &Type,
        rhs: &Type,
        resolutions: &Resolutions,
        ast: &Ast,
        types: &mut TypeTable,
        span: fallc_span::Span,
    ) -> Option<OperatorCall> {
        let method = fallc_ast::special::SpecialMethod::for_binary(op, false)?;
        let call = self.special_method_call(resolutions, lhs, method.name(), 2)?;
        let parameter_type = call.param_types.get(1)?.clone();
        let by_address =
            !self.is_compatible(rhs, &parameter_type) && self.operand_fits(rhs, &parameter_type);
        if !self.is_compatible(rhs, &parameter_type) && !by_address {
            return None;
        }
        self.note_throwing_call(span, &call.signature, ast, None);
        let _ = types;
        Some(OperatorCall {
            symbol: call.symbol,
            argument_by_address: by_address,
            reflected: false,
        })
    }

    /// Infers the type of an expression used as an assignment target.
    ///
    /// A name, a dereference, a field, an index, and a call that returns a
    /// reference all denote storage. Assigning through a pointer or reference
    /// whose pointee is `const` is refused, and so is a field of a `const`
    /// object or `const` pointer -- except a field declared `mutable`.
    pub(super) fn infer_lvalue(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        target: ExprId,
    ) -> Option<Type> {
        match &ast.expr(target).kind {
            // A name denotes storage.
            ExprKind::Name(_) => self.infer_expr(ast, resolutions, types, target),
            // A dereference denotes the storage a pointer points at, so it can
            // be assigned through — unless the pointee is const.
            ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => {
                let operand_ty = self.infer_expr(ast, resolutions, types, unary.operand)?;
                if let Some(pointee) = operand_ty.pointee().cloned() {
                    if operand_ty.pointee_is_const() {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "cannot assign through a pointer to const `{}`",
                                Type::spelling(&pointee)
                            ))
                            .with_code(DiagnosticCode::sema(44))
                            .with_span(ast.expr(target).span),
                        );
                        return None;
                    }
                    return Some(pointee);
                }
                // A class iterator names storage through `__deref__`, and the
                // call to that method is what `*it` lowers to; checking the
                // whole expression records the method for code generation and
                // says what the reference points at.
                let result = self.infer_expr(ast, resolutions, types, target)?;
                if result.pointee_is_const() {
                    self.diagnostics.push(
                        Diagnostic::error("cannot assign through a reference to const")
                            .with_code(DiagnosticCode::sema(44))
                            .with_span(ast.expr(target).span),
                    );
                    return None;
                }
                Some(result.referent().cloned().unwrap_or(result))
            }
            // A field denotes storage, so it can be assigned to: `this->x =
            // v` writes the field. A field of a const pointee is refused, which
            // is part of what makes `*const T` mean something.
            ExprKind::Field(field) => {
                let object_ty = self
                    .infer_expr(ast, resolutions, types, field.object)
                    .unwrap_or_else(|| Type::unspecified("", ast.expr(field.object).span));
                // A `mutable` field is the one exception to "a const method
                // does not modify its receiver". The field says so itself,
                // which is what makes a cache or a reference count possible in
                // a type whose other fields are constant.
                if object_ty.pointee_is_const() && !self.field_is_mutable(&object_ty, &field.field)
                {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "cannot assign to `{}` through a pointer to const",
                            field.field
                        ))
                        .with_code(DiagnosticCode::sema(44))
                        .with_span(ast.expr(target).span)
                        .with_note(
                            "a field marked `mutable` may be changed even then, but this one \
                             is not",
                        ),
                    );
                    return None;
                }
                // A `const` binding names an object that may not change, so a
                // field of it may not be assigned, however many field and index
                // steps the path takes: `h.n = 9` modifies `h` itself.
                if !self.field_is_mutable(&object_ty, &field.field)
                    && self.const_root(ast, resolutions, field.object).is_some()
                {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "cannot assign to `{}` of a `const` object",
                            field.field
                        ))
                        .with_code(DiagnosticCode::sema(44))
                        .with_span(ast.expr(target).span)
                        .with_note(
                            "a field marked `mutable` may be changed even then, but this one \
                             is not",
                        ),
                    );
                    return None;
                }
                self.infer_expr(ast, resolutions, types, target)
            }
            // An index denotes storage too, so `a[i] = v` writes through it.
            // The element cannot be written when the array is `const` or the
            // pointer points at const, which `type_of_index` records.
            ExprKind::Index(index) => {
                let element = self.infer_expr(ast, resolutions, types, target)?;
                // A `__getitem__` that returns a reference names storage, and
                // *that* is what is written: `a[i] = v` stores through the
                // reference, which is why there is no `__setitem__`. A
                // reference to const is refused, like any other.
                if let Some(referent) = element.referent().cloned() {
                    if element.pointee_is_const() {
                        self.diagnostics.push(
                            Diagnostic::error("cannot assign through a reference to const")
                                .with_code(DiagnosticCode::sema(44))
                                .with_span(ast.expr(target).span),
                        );
                        return None;
                    }
                    // The reference *is* the storage: `a[i] = v` writes through
                    // it. A compound assignment reads the value first, and the
                    // reader is a value, so that read is recorded -- the same
                    // record `*p` makes for `*p + 1`.
                    self.deref_reads
                        .insert(ast.expr(target).span, referent.clone());
                    return Some(referent);
                }
                // A pointer to const may not be written through.
                let object_ty = types
                    .get(index.object)
                    .cloned()
                    .unwrap_or_else(|| Type::unspecified("", ast.expr(index.object).span));
                // An array marked `const` is an object that may not change, so
                // an element of it may not be written either -- however many
                // index and field steps the path takes, `a[1] = 4` modifies
                // `a` itself. The `const` is on the binding, not on the
                // element's type, so it is the root of the path that answers.
                // A read-only array says the same thing twice: `const [N]T`
                // and `[N]const T` are one type, and either way its elements
                // may not be written. The binding is not marked `const` in the
                // second spelling, so the *element* is what is asked.
                let element_is_const = matches!(
                    object_ty.unqualified(),
                    Type::Array { element, .. } if matches!(**element, Type::Const(_))
                );
                if element_is_const || self.const_root(ast, resolutions, index.object).is_some() {
                    self.diagnostics.push(
                        Diagnostic::error("cannot assign to an element of a `const` object")
                            .with_code(DiagnosticCode::sema(44))
                            .with_span(ast.expr(target).span)
                            .with_note(
                                "a `const` binding names an object that may not change, and \
                                 its elements are part of it",
                            ),
                    );
                    return None;
                }
                if object_ty.pointee_is_const() {
                    self.diagnostics.push(
                        Diagnostic::error("cannot assign through a pointer to const")
                            .with_code(DiagnosticCode::sema(44))
                            .with_span(ast.expr(target).span),
                    );
                    return None;
                }
                Some(element)
            }
            // A method that returns a reference names storage just as an index
            // does, so `a.last() = 99` writes the element. It is the same rule
            // `a[i] = v` uses, and for the same reason: a reference *is* a
            // place, so an expression of reference type is somewhere a value can
            // go. This is why the accessors return `&T` rather than `T`.
            ExprKind::Call(_) => {
                // Inferred with an *address* as the expectation. A method that
                // returns a reference is normally read as its referent -- `x =
                // a.last()` wants the value -- and only the context says
                // otherwise. As an assignment target the context does say so,
                // but `self.expected` is empty here, so the reference was
                // stripped before this arm ever saw it and `a.last_mut() = 99`
                // was refused as "a call returns a value, not a place".
                let previous = self.expected.replace(Type::Pointer {
                    pointee: Box::new(Type::unspecified("", ast.expr(target).span)),
                    pointee_is_const: false,
                    pointer_is_const: false,
                });
                let value = self.infer_expr(ast, resolutions, types, target);
                self.expected = previous;
                let value = value?;
                let Some(referent) = value.referent().cloned() else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "this call returns a value, not a place, so it cannot be assigned to",
                        )
                        .with_code(DiagnosticCode::sema(34))
                        .with_span(ast.expr(target).span)
                        .with_note(
                            "return a reference (`&T`) from the function to name the storage it \
                             should write",
                        ),
                    );
                    return None;
                };
                if value.pointee_is_const() {
                    self.diagnostics.push(
                        Diagnostic::error("cannot assign through a reference to const")
                            .with_code(DiagnosticCode::sema(44))
                            .with_span(ast.expr(target).span),
                    );
                    return None;
                }
                // A compound assignment reads the value first, and that read is
                // recorded exactly as it is for `a[i]`.
                self.deref_reads
                    .insert(ast.expr(target).span, referent.clone());
                Some(referent)
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("this expression cannot be assigned to")
                        .with_code(DiagnosticCode::sema(34))
                        .with_span(ast.expr(target).span)
                        .with_note(
                            "only a variable, a field, an index, or a dereference may appear \
                             on the left of `=`",
                        ),
                );
                None
            }
        }
    }

    /// Types a call of a value whose type defines `__call__`.
    ///
    /// The object is the receiver, so the arguments are checked against the
    /// method's parameters after `this`. A type without `__call__` cannot be
    /// called, which is what the diagnostic says.
    /// The name a written parameter is, when it is a template parameter with no
    /// arguments of its own, seen through the references and pointers around it.
    ///
    /// `a A` and `value &A` both deduce `A`; the indirection says how the
    /// argument is passed, not what its type is.
    pub(super) fn bare_parameter_name(
        ty: &fallc_ast::TypeSyntax,
    ) -> Option<(&fallc_ast::NamedType, bool)> {
        use fallc_ast::TypeSyntax;
        fn walk(ty: &TypeSyntax) -> Option<(&fallc_ast::NamedType, bool)> {
            match ty {
                TypeSyntax::Named(named) => Some((named, false)),
                TypeSyntax::UnsizedArray(inner)
                | TypeSyntax::Pointer(inner)
                | TypeSyntax::PointerToConst(inner)
                | TypeSyntax::ConstPointer(inner)
                | TypeSyntax::Reference(inner)
                | TypeSyntax::ConstReference(inner)
                | TypeSyntax::RvalueReference(inner)
                | TypeSyntax::ConstRvalueReference(inner) => {
                    walk(inner).map(|(named, _)| (named, true))
                }
                TypeSyntax::Volatile(inner) | TypeSyntax::Const(inner) => walk(inner),
                _ => None,
            }
        }
        walk(ty)
    }

    pub(super) fn type_of_call_method(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        let previous = self.expected.take();
        let callee = self.infer_expr(ast, resolutions, types, node.callee);
        self.expected = previous;
        let callee = callee?;

        let Some(receiver_name) = callee.nominal_name().map(str::to_owned) else {
            self.diagnostics.push(
                Diagnostic::error(format!("`{}` cannot be called", callee.spelling()))
                    .with_code(DiagnosticCode::sema(35))
                    .with_span(span)
                    .with_note("a value is callable when its type defines `__call__`"),
            );
            return None;
        };

        let written = fallc_ast::special::SpecialMethod::Call.name();
        let qualified = Self::special_method_qualified(&receiver_name, written);
        // The receiver's own arguments, needed before the candidates are
        // compared: `@enable_if` decides by them which of two same-shaped
        // `__call__`s exists for this member.
        let member_substitution = self.receiver_substitution(&callee);
        let candidates: Vec<FunctionSignature> = self
            .definitions_named_for_method(resolutions, &qualified)
            .into_iter()
            .filter_map(|def| def.signature.clone())
            .filter(|signature| self.signature_enabled(signature, &member_substitution))
            .collect();
        if candidates.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(format!("`{}` cannot be called", callee.spelling()))
                    .with_code(DiagnosticCode::sema(35))
                    .with_span(span)
                    .with_note("a value is callable when its type defines `__call__`"),
            );
            return None;
        }

        // The receiver's family parameters, exactly as for any other method: a
        // comparator family is written `Less<E>`, and `Less<int>` called with an
        // `int` has to see `E` as `int`. Without this the parameters were read
        // as written, so every call to a family's `__call__` was a type error
        // about an unknown `E`.
        let saved_substitution = match &member_substitution {
            Some(substitution) => self.substitution.replace(substitution.clone()),
            None => self.substitution.take(),
        };

        // Which member's `__call__` this is: the one whose receiver fits, and
        // whose parameter count matches the arguments. A family may declare
        // `__call__` once per member, and the receiver decides which.
        let chosen = candidates.iter().find_map(|signature| {
            let receiver_syntax = signature.param_types.first()?;
            // A candidate written against a family's parameters belongs to that
            // family and no other; resolving it without the member's arguments
            // would report the parameters as arguments of the wrong kind.
            if member_substitution.is_none()
                && self.mentions_template_parameter_syntax(receiver_syntax)
            {
                return None;
            }
            let receiver = self.resolve_type_syntax(receiver_syntax, true);
            if !self.receiver_fits(&callee, &receiver) {
                return None;
            }
            // A pack takes the rest of the arguments, so its parameter count
            // is a lower bound rather than the answer.
            let arity_ok = match signature.pack {
                Some(pack) => node.args.len() + 1 >= pack,
                None => signature.param_types.len() == node.args.len() + 1,
            };
            arity_ok.then(|| signature.clone())
        });
        let Some(signature) = chosen else {
            let found = self.substitution.is_some();
            self.substitution = saved_substitution;
            let mut diagnostic = Diagnostic::error(format!(
                "`{}` cannot be called with {} argument(s)",
                callee.spelling(),
                node.args.len()
            ))
            .with_code(DiagnosticCode::sema(35))
            .with_span(span)
            .with_note("a value is callable when its type defines `__call__`");
            if found {
                diagnostic = diagnostic.with_note(format!(
                    "`__call__` on `{receiver_name}` takes its receiver and then the \
                     arguments it declares"
                ));
            }
            self.diagnostics.push(diagnostic);
            return None;
        };

        // The method's own template parameters are deduced from the arguments,
        // the same way a free template's are: `__call__(this &Device<M>, a A)`
        // takes `A` from what is passed, and `Device<M>`'s parameters from the
        // receiver.
        //
        // A *pack* parameter takes the rest of the arguments. Its elements are
        // the arguments from that position on, and the element type is the
        // written type with one element bound -- so `values ...Ts` gives the
        // arguments' own types, `values ...&Ts` gives references to them, and
        // `values ...int` gives `int` however many there are.
        self.finish_pack_call(
            ast,
            resolutions,
            types,
            node,
            span,
            &callee,
            &signature,
            &member_substitution,
            saved_substitution,
            PackCallKind::ValueCall,
        )
    }

    /// Finishes a call whose callee takes a parameter pack.
    ///
    /// The elements are the arguments from the pack's position on; each is
    /// checked against the written element type with that element's type bound,
    /// so `values ...Ts` takes the arguments' own types, `values ...&Ts` takes
    /// references to them, and `values ...int` takes `int` however many there
    /// are. The symbol carries the element types and their count, because every
    /// arity is a function of its own.
    ///
    /// Shared by a call of a value that defines `__call__` and a call of an
    /// ordinary method that declares a pack: the two differ only in where the
    /// receiver is written and which table records the symbol.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_pack_call(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &CallExpr,
        span: fallc_span::Span,
        receiver: &Type,
        signature: &FunctionSignature,
        member_substitution: &Option<crate::template::Substitution>,
        caller_substitution: Option<crate::template::Substitution>,
        kind: PackCallKind,
    ) -> Option<Type> {
        self.substitution = member_substitution.clone();
        let pack = signature.pack;
        let pack_written = pack
            .and_then(|index| signature.param_types.get(index))
            .cloned();
        let element_parameter = pack_written.as_ref().and_then(|written| {
            super::templates::pack_element_parameter(written, &signature.template_params)
        });
        let mut combined = member_substitution.clone().unwrap_or_default();
        let mut deduced_packs: std::collections::HashMap<String, Vec<Type>> =
            std::collections::HashMap::new();
        // The value each element argument stands for: the type the written
        // parameter is resolved with, one element at a time.
        let mut element_values: Vec<Type> = Vec::new();
        for (position, arg) in node.args.iter().enumerate() {
            let index = position + 1;
            let is_element = pack.is_some_and(|pack| index >= pack);
            let written = if is_element {
                pack_written.clone()
            } else {
                signature.param_types.get(index).cloned()
            };
            let Some(written) = written else {
                continue;
            };
            let Some((named, indirect)) = Self::bare_parameter_name(&written) else {
                // A concrete element type: the arguments still have to be
                // checked against it, which the check below does.
                if is_element {
                    let previous = self.expected.take();
                    let found = self.infer_expr(ast, resolutions, types, *arg);
                    self.expected = previous;
                    if let Some(found) = found {
                        element_values.push(found);
                    }
                }
                continue;
            };
            if !named.arguments.is_empty() {
                continue;
            }
            if is_element {
                let previous = self.expected.take();
                let found = self.infer_expr(ast, resolutions, types, *arg);
                self.expected = previous;
                if let Some(found) = found {
                    let bound = if indirect {
                        found.referent().cloned().unwrap_or(found)
                    } else {
                        found
                    };
                    element_values.push(bound.clone());
                    if let Some(name) = element_parameter {
                        deduced_packs
                            .entry(name.to_owned())
                            .or_default()
                            .push(bound);
                    }
                }
                continue;
            }
            let is_type_parameter = signature.template_params.iter().any(|parameter| {
                parameter.name == named.path && parameter.kind == fallc_ast::TemplateParamKind::Type
            });
            // A name the receiver's member already bound is the family's, and
            // the receiver is what decides it: `Less<int>`'s `E` is `int`
            // before the call, whatever the argument is passed as. Taking it
            // from the argument made a comparator of `Less<*int>`.
            if !is_type_parameter || combined.get(&named.path).is_some() {
                continue;
            }
            let previous = self.expected.take();
            let found = self.infer_expr(ast, resolutions, types, *arg);
            self.expected = previous;
            if let Some(found) = found {
                // An indirect parameter takes the value behind the argument:
                // `other &const T` given a `&const int` binds `T` to `int`, not
                // to the reference it arrived in.
                let bound = if indirect {
                    found.referent().cloned().unwrap_or(found)
                } else {
                    found
                };
                combined.bind_type(named.path.clone(), bound);
            }
        }
        // The elements, as ParameterTypes: the written element type with each
        // element bound in turn. This is where `&Ts` becomes `&int`.
        let mut element_types: Vec<Type> = Vec::with_capacity(element_values.len());
        if let Some(written) = &pack_written {
            for value in &element_values {
                let mut element_substitution = combined.clone();
                if let Some(name) = element_parameter {
                    element_substitution.bind_type(name.to_owned(), value.clone());
                }
                element_types.push(self.resolve_template_type(written, &element_substitution));
            }
        }
        for (name, types) in deduced_packs {
            combined.bind(
                name,
                crate::template::TemplateArgument::Pack(
                    types
                        .into_iter()
                        .map(crate::template::TemplateArgument::Type)
                        .collect(),
                ),
            );
        }
        self.substitution = Some(combined.clone());

        // The arguments are checked against the parameters after the receiver.
        let mut element_index = 0usize;
        for (position, arg) in node.args.iter().enumerate() {
            let index = position + 1;
            let is_element = pack.is_some_and(|pack| index >= pack);
            let expected = if is_element {
                let expected = element_types.get(element_index).cloned();
                element_index += 1;
                expected
            } else {
                signature
                    .param_types
                    .get(index)
                    .map(|parameter| self.resolve_type_syntax(parameter, true))
            };
            let Some(expected) = expected else {
                continue;
            };
            let previous = self.expected.replace(expected.clone());
            let found = self.infer_expr(ast, resolutions, types, *arg);
            self.expected = previous;
            if let Some(found) = found {
                // A reference parameter takes the argument's address, exactly
                // as it does for any other method: `__call__(this &C, a &T)`
                // receives `a`'s address, and the code generator has to be told
                // which arguments those are.
                self.note_indirect_argument(*arg, &found, &expected, ast);
                let was_parameter = self.in_parameter_position;
                self.in_parameter_position = true;
                if !self.adopt_literal_argument(resolutions, ast, *arg, &expected, &found) {
                    self.types_must_match(&expected, &found, ast.expr(*arg).span, "argument");
                }
                self.in_parameter_position = was_parameter;
            }
        }

        // The symbol is recorded, so code generation emits the call rather than
        // trying to call a value. A pack's elements are part of the mangled
        // signature: `f(1)` and `f(1, 2)` are two functions.
        let mut expanded_params: Vec<Type> = Vec::with_capacity(signature.param_types.len());
        for (index, written) in signature.param_types.iter().enumerate() {
            if pack == Some(index) {
                expanded_params.extend(element_types.iter().cloned());
            } else {
                expanded_params.push(self.resolve_template_type(written, &combined));
            }
        }
        let params: Vec<String> = expanded_params
            .iter()
            .map(crate::types::mangle_type)
            .collect();
        let returns = self.signature_result_type(signature);
        self.substitution = caller_substitution;
        let symbol = fallc_ast::special::symbol_for(&signature.written_name, &params);
        match kind {
            PackCallKind::ValueCall => {
                self.call_methods.insert(span, symbol.clone());
            }
            PackCallKind::Method => {
                self.method_calls.insert(span, symbol.clone());
            }
        }
        // A method of a templated type is emitted only when some member uses it,
        // so the instance is recorded here for the same reason the method-call
        // path records one: without it the symbol had no body.
        if signature.pack.is_some() {
            self.record_method_instantiation_with_pack(
                signature,
                &combined,
                &symbol,
                receiver,
                Some(element_types),
            );
        } else if !combined.is_empty() {
            self.record_method_instantiation(signature, &combined, &symbol, receiver);
        }
        self.note_throwing_call(span, signature, ast, Some(node));
        Some(returns)
    }

    /// Checks that an expression denotes storage, without requiring it to be
    /// writable.
    ///
    /// Used by `&`, which needs an address but does not write through it. The
    /// assignment path wants the stricter check, because that one does write.
    pub(super) fn check_addressable(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        target: ExprId,
    ) -> Option<Type> {
        let ty = self.infer_expr(ast, resolutions, types, target)?;
        match &ast.expr(target).kind {
            ExprKind::Name(_) | ExprKind::Field(_) | ExprKind::Index(_) => Some(ty),
            ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => Some(ty),
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("this expression has no address")
                        .with_code(DiagnosticCode::sema(34))
                        .with_span(ast.expr(target).span)
                        .with_note("only a variable, a field, an index, or a dereference does"),
                );
                None
            }
        }
    }

    /// Returns `true` if an lvalue's storage may not be written.
    ///
    /// True when reaching it passed through a `*const`: a field of a `*const`
    /// receiver, or a dereference of a `*const` pointer.
    pub(super) fn storage_is_const(&self, ast: &Ast, types: &TypeTable, target: ExprId) -> bool {
        match &ast.expr(target).kind {
            ExprKind::Field(field) => {
                let object_ty = types.get(field.object);
                let through_const = object_ty.is_some_and(Type::pointee_is_const);
                through_const || self.storage_is_const(ast, types, field.object)
            }
            ExprKind::Unary(unary) if unary.op == UnaryOp::Deref => {
                types.get(unary.operand).is_some_and(Type::pointee_is_const)
            }
            ExprKind::Index(index) => {
                let object_ty = types.get(index.object);
                object_ty.is_some_and(Type::pointee_is_const)
                    || self.storage_is_const(ast, types, index.object)
            }
            _ => false,
        }
    }

    /// The name a type is written by, when this expression is one.
    ///
    /// A type named through the module that declares it -- `lib.Color` -- is
    /// written as a field access, so the spelling is rebuilt from the
    /// expression rather than taken from one identifier.
    fn written_type_name(ast: &Ast, expr: ExprId) -> Option<String> {
        match &ast.expr(expr).kind {
            ExprKind::Name(name) => Some(name.clone()),
            ExprKind::Field(field) => {
                let ExprKind::Name(module) = &ast.expr(field.object).kind else {
                    return None;
                };
                Some(format!("{module}.{}", field.field))
            }
            _ => None,
        }
    }

    /// Types a member access.
    ///
    /// `pointer->field` is `(*pointer).field`, so the arrow form follows one
    /// level of indirection before looking the field up.
    pub(super) fn type_of_field(
        &mut self,
        ast: &Ast,
        resolutions: &Resolutions,
        types: &mut TypeTable,
        node: &FieldExpr,
        span: fallc_span::Span,
    ) -> Option<Type> {
        // `lim.LIMIT` is a *constant* named through a module: the object half
        // is a module and the member is an exported value. Folded here, which
        // is what makes `C.EOF` usable wherever a value is.
        if let ExprKind::Name(object_name) = &ast.expr(node.object).kind
            && self.names_a_module(resolutions, ast, node.object)
        {
            let qualified = format!("{object_name}.{}", node.field);
            if let Some(message) = self.deprecated_functions.get(&qualified).cloned() {
                self.warn_deprecated(&qualified, message, span);
            }
            // An external variable is a member like a constant is, but it is
            // read rather than folded: its storage is in the library.
            if let Some(ty) = self.imported_variables.get(&qualified).cloned() {
                self.external_global_refs.insert(span, qualified.clone());
                return Some(ty);
            }
            if let Some(value) = self.constants_by_name.get(&qualified).copied() {
                self.constant_values.insert(span, value);
                // The type is the declared one; without it the value has no
                // width and code generation cannot emit it.
                let ty = self
                    .constant_types_by_name
                    .get(&qualified)
                    .cloned()
                    .unwrap_or(Type::INT);
                return Some(ty);
            }
        }
        // `std.Array` is a *type* named through a module, not a field of a
        // value: `std` names a module, and `Array` is what it exports. The
        // parser cannot tell the two apart -- `C.printf` and `std.Array` have
        // the same shape -- so it is decided here, where the module table is
        // known.
        //
        // Resolved to the same type the qualified name would name, so that
        // `std.Array<int>` and a bare `Array<int>` build one family member and
        // not two.
        if let ExprKind::Name(object_name) = &ast.expr(node.object).kind
            && self.names_a_module(resolutions, ast, node.object)
        {
            let qualified = format!("{object_name}.{}", node.field);
            if self.type_templates.contains_key(&qualified)
                || self.nominal_types.contains_key(&qualified)
            {
                return self
                    .resolve_type_syntax(
                        &fallc_ast::TypeSyntax::Named(fallc_ast::NamedType {
                            path: qualified,
                            arguments: Vec::new(),
                            is_expansion: false,
                            span,
                        }),
                        false,
                    )
                    .into();
            }
        }
        // `Color.RED` is an enumerator, not a field of a value: the object half
        // names a *type*, and the member names a constant of it. It is checked
        // before the object is inferred, because a type name has no value to
        // infer -- asking for one is what produced "cannot generate code for
        // this field's object".
        //
        // The object may be a type from another module, written `lib.Color`, so
        // the spelling is rebuilt from the expression rather than taken from one
        // identifier.
        if let Some(object_name) = Self::written_type_name(ast, node.object)
            && let Some(base) = self.enum_bases.get(&object_name).cloned()
        {
            let qualified = format!("{object_name}.{}", node.field);
            match self.constants_by_name.get(&qualified).copied() {
                Some(value) => {
                    // The value is a constant, so it is folded where it stands
                    // -- the same record every other constant makes, which is
                    // what code generation reads instead of an expression.
                    self.constant_values.insert(span, value);
                    return Some(base);
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "`{object_name}` has no member `{}`",
                            node.field
                        ))
                        .with_code(DiagnosticCode::sema(79))
                        .with_span(span),
                    );
                    return None;
                }
            }
        }
        let previous = self.expected.take();
        let object = self.infer_expr(ast, resolutions, types, node.object);
        self.expected = previous;
        let object = object?;

        // The arrow form dereferences first.
        //
        // A pointer is followed directly. Any other type is asked for
        // `__pointer__`, and its result is followed the same way, so `a->b->c`
        // may take several steps: each one is a type that itself supports `->`.
        // That is what lets an iterator or a wrapper be used with the same
        // syntax as a pointer.
        let base = if node.through_pointer {
            match object.pointee() {
                Some(pointee) => pointee.clone(),
                None => {
                    let mut current = object.clone();
                    // Bounded so that a type whose `__pointer__` returns itself
                    // is a diagnostic rather than a hang.
                    let mut steps = 0;
                    loop {
                        steps += 1;
                        if steps > 16 {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "`{}` resolves `->` to itself, so `{}` cannot be reached",
                                    object.spelling(),
                                    node.field
                                ))
                                .with_code(DiagnosticCode::sema(60))
                                .with_span(span),
                            );
                            return None;
                        }
                        if let Some(pointer) = current.pointee() {
                            // The field is looked up in what the chain ends at,
                            // which code generation needs and cannot recover
                            // from the object's own type.
                            self.pointer_targets.insert(span, pointer.clone());
                            break pointer.clone();
                        }
                        // Each step's call is recorded under its own key, so
                        // code generation can walk the chain without deciding
                        // anything: the sequence of symbols *is* the chain.
                        let step = fallc_span::Span::new(
                            span.file,
                            span.lo.saturating_add(u32::try_from(steps).unwrap_or(0)),
                            span.lo.saturating_add(u32::try_from(steps).unwrap_or(0)),
                        );
                        match self.resolve_pointer_step(resolutions, &current, step, ast) {
                            Some(next) => {
                                if next == current {
                                    self.diagnostics.push(
                                        Diagnostic::error(format!(
                                            "`{}` resolves `->` to itself",
                                            current.spelling()
                                        ))
                                        .with_code(DiagnosticCode::sema(60))
                                        .with_span(span),
                                    );
                                    return None;
                                }
                                current = next;
                            }
                            None => {
                                self.diagnostics.push(
                                    Diagnostic::error(format!(
                                        "`->` needs a pointer or a type with `__pointer__`, but \
                                         `{}` is neither",
                                        object.spelling()
                                    ))
                                    .with_code(DiagnosticCode::sema(60))
                                    .with_span(span),
                                );
                                return None;
                            }
                        }
                    }
                }
            }
        } else {
            object
        };

        // A pointer used with `.` is followed too, which is what makes
        // `this.x` work on a `*Point` receiver as well as `this->x`. The two
        // spellings are accepted interchangeably rather than one being an
        // error, because the receiver of a member function is always a pointer.
        let base = match base.pointee() {
            Some(pointee) if !node.through_pointer => pointee.clone(),
            _ => base,
        };
        // A member of a family that mentions itself leaves an incomplete
        // placeholder behind while its own fields are being resolved, and a
        // pointer taken to it carries that placeholder. Reading a field through
        // such a pointer is reading the *finished* type's field -- the two have
        // one name, and `nominal_types` holds the finished one by the time any
        // body is checked -- so the placeholder is replaced here.
        // `volatile` says how memory is accessed, not what the value is, so a
        // field of a `volatile T` is a field of `T`. Reaching one through a
        // `&volatile T` receiver -- `this.x` -- has to see through the
        // qualifier, exactly as a plain value does.
        let base = match base {
            Type::Volatile(inner) => *inner,
            other => other,
        };
        let base = match base.unqualified() {
            Type::Nominal(nominal) if nominal.is_incomplete => self
                .nominal_types
                .get(&nominal.name)
                .filter(|completed| !completed.is_incomplete)
                .cloned()
                .map_or(base, Type::Nominal),
            _ => base,
        };

        // A type declared but not defined has no members to reach: saying that
        // is more useful than "has no field", which would suggest the name was
        // misspelt.
        if let Some(name) = self.incomplete_nominal(&base) {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` is incomplete, so its members are not known yet",
                    name
                ))
                .with_code(DiagnosticCode::sema(167))
                .with_span(span)
                .with_note(
                    "a forward declaration names the type; its fields come with the \
                     definition",
                ),
            );
            return None;
        }

        let Some(nominal) = base.as_nominal() else {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` has no fields, so `{}` cannot be accessed",
                    base.spelling(),
                    node.field
                ))
                .with_code(DiagnosticCode::sema(60))
                .with_span(span),
            );
            return None;
        };

        match nominal.field(&node.field) {
            Some((_, ty)) => Some(ty.clone()),
            None => {
                let known: Vec<&str> = nominal
                    .fields
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                self.diagnostics.push(
                    Diagnostic::error(format!("`{}` has no field `{}`", nominal.name, node.field))
                        .with_code(DiagnosticCode::sema(61))
                        .with_span(span)
                        .with_note(format!("its fields are: {}", known.join(", "))),
                );
                None
            }
        }
    }
}
