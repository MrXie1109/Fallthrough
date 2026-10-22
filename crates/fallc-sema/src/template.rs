//! Template arguments and the substitution that turns one into a function.
//!
//! # What a template is
//!
//! ```text
//! template <T type>
//! fn min(lhs &const T, rhs &const T) T { return lhs < rhs ? lhs : rhs; }
//! ```
//!
//! This declares a *family* of functions, one per type `T`. The declaration is
//! not itself callable, because there is no `T` to call it with; a call makes
//! one member of the family by deciding what `T` is, and that member is what
//! gets checked and emitted.
//!
//! # How a member is made
//!
//! The template arguments are *deduced* from the call's arguments, and a
//! template with none is written out. The arguments and the parameters are then
//! substituted for them everywhere they appear -- in the parameter types, the
//! return type, and the body -- and the result is checked like any other
//! function.
//!
//! # One member per argument list
//!
//! Two calls with the same arguments are the same function, so the second one
//! reuses the first's instantiation rather than making a copy. That is what
//! makes recursion terminate and keeps a program from growing without bound;
//! `Arguments` is the key such a cache is looked up by.

use std::collections::HashMap;

use fallc_ast::{TemplateParam, TypeSyntax};

use crate::types::Type;

/// What one instantiation supplies for a template's parameters.
///
/// A type parameter takes a type; a value parameter takes an integer or a
/// character sequence, because a template argument has to be known without
/// running the program.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateArgument {
    /// A type, from `min<int>(..)` or deduced from an argument.
    Type(Type),
    /// A value, written as an integer.
    Int(i64),
    /// A value, written as a character sequence.
    ///
    /// Kept as the decoded bytes rather than as source text, so that two
    /// spellings of one string are the same argument. It is the only way a
    /// sequence can reach a template: `template <name []char>` and
    /// `f<"text">()`.
    Text(Vec<u8>),
    /// A sequence of arguments, bound to a parameter pack.
    ///
    /// `template <Args ...type> fn count() int` called as
    /// `count<int, f64>()` binds `Args` to two type arguments. A pack
    /// *argument* is a list rather than a value, which is what lets one
    /// parameter stand for any number of them.
    Pack(Vec<TemplateArgument>),
}

impl TemplateArgument {
    /// Returns how this argument should be spelled in a diagnostic.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            TemplateArgument::Type(ty) => ty.spelling(),
            TemplateArgument::Int(value) => value.to_string(),
            TemplateArgument::Text(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                format!("\"{text}\"")
            }
            TemplateArgument::Pack(elements) => elements
                .iter()
                .map(TemplateArgument::spelling)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// Returns the key this argument contributes to an instantiation's name.
    ///
    /// Type arguments mangle as types do, which is injective; the two value
    /// kinds are tagged so that `1` and `"1"` are different arguments.
    #[must_use]
    pub fn mangle(&self) -> String {
        match self {
            TemplateArgument::Type(ty) => crate::types::mangle_type(ty),
            TemplateArgument::Int(value) => format!("L{value}_"),
            TemplateArgument::Text(bytes) => format!("S{}_", String::from_utf8_lossy(bytes)),
            // A pack mangles as a list, with the length folded in so that
            // `f<int>` and `f<int, int>` cannot share a symbol. The count comes
            // first and is followed by `_`, which keeps it unambiguous: the
            // parts of a type's own mangling never begin with a bare count.
            TemplateArgument::Pack(elements) => {
                let inner: String = elements.iter().map(TemplateArgument::mangle).collect();
                format!("P{}_{}", elements.len(), inner)
            }
        }
    }
}

/// One instantiation's arguments, which name it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Arguments(pub Vec<TemplateArgument>);

impl Arguments {
    /// Returns the key two instantiations are the same under.
    #[must_use]
    pub fn key(&self) -> String {
        self.0.iter().map(TemplateArgument::mangle).collect()
    }

    /// Returns how these arguments are spelled for a diagnostic.
    #[must_use]
    pub fn spelling(&self) -> String {
        self.0
            .iter()
            .map(TemplateArgument::spelling)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A substitution from template parameter names to arguments.
#[derive(Clone, Debug, Default)]
pub struct Substitution {
    /// The parameters, in order, with what each is bound to.
    bindings: HashMap<String, TemplateArgument>,
    /// The declarations the bindings came from, in order.
    ///
    /// Kept because a reader of a *method* of a family needs to know what the
    /// family's own value parameters are called and what type each has, and the
    /// method's signature does not repeat them: `Device<Mode>`'s `write` names
    /// `Mode` only through its receiver. The declaration is what says `Mode` is
    /// an `OpenMode`, which is the type an expression naming it has.
    params: Vec<TemplateParam>,
}

impl Substitution {
    /// Builds the substitution pairing a template's parameters with arguments.
    ///
    /// `None` when the counts cannot be made to line up, which is a call that
    /// supplied the wrong number of explicit template arguments.
    ///
    /// A trailing pack takes every argument left over, including none at all:
    /// `template <T type, Rest ...type>` accepts one argument (binding `Rest`
    /// to the empty pack) or several. Nothing else may be short or long, since
    /// there would be no parameter to bind the extra argument to.
    #[must_use]
    pub fn new(params: &[TemplateParam], arguments: &Arguments) -> Option<Self> {
        let fixed = params.iter().take_while(|param| !param.is_pack).count();
        let has_pack = fixed < params.len();
        let given = arguments.0.len();
        if if has_pack {
            given < fixed
        } else {
            given != fixed
        } {
            return None;
        }
        let mut bindings: HashMap<String, TemplateArgument> = params
            .iter()
            .take(fixed)
            .zip(&arguments.0)
            .map(|(param, argument)| (param.name.clone(), argument.clone()))
            .collect();
        if has_pack {
            let mut rest = arguments.0[fixed..].to_vec();
            // A caller that has already gathered the pack's elements into one
            // `Pack` argument hands over a list of one; the binding is the
            // pack itself, never a pack of a pack. Splicing it here keeps one
            // meaning of "what a pack is bound to" for every reader -- the
            // expansion of `Args...`, a fold over `values`, and the key two
            // instantiations are the same under.
            if let [TemplateArgument::Pack(elements)] = rest.as_slice() {
                rest = elements.clone();
            }
            let pack = &params[fixed];
            bindings.insert(pack.name.clone(), TemplateArgument::Pack(rest));
        }
        Some(Substitution {
            bindings,
            params: params.to_vec(),
        })
    }

    /// Returns the declarations the bindings came from, in order.
    #[must_use]
    pub fn params(&self) -> &[TemplateParam] {
        &self.params
    }

    /// Returns `true` when nothing is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Returns what a name is bound to, if anything.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&TemplateArgument> {
        self.bindings.get(name)
    }

    /// Binds a name to a type, adding to whatever this substitution already has.
    ///
    /// Used where a name stands for a type without a template at all: a trait's
    /// type variables are the types of its parameters, so asking a trait about a
    /// type means binding those variables to it. A name already bound is
    /// shadowed, which is what makes the inner question win.
    pub fn bind_type(&mut self, name: impl Into<String>, ty: Type) {
        self.bindings.insert(name.into(), TemplateArgument::Type(ty));
    }

    /// Binds a name to an argument of whatever kind it is.
    ///
    /// A method of a family declares template parameters of its own -- the
    /// comparator of `sort_by`, say -- and the body is checked under them: the
    /// family's substitution says what the receiver's arguments are, and this
    /// adds the method's.
    pub fn bind(&mut self, name: impl Into<String>, argument: TemplateArgument) {
        self.bindings.insert(name.into(), argument);
    }

    /// Returns `true` if this name is a type parameter.
    #[must_use]
    pub fn is_type_parameter(&self, name: &str) -> bool {
        matches!(self.bindings.get(name), Some(TemplateArgument::Type(_)))
    }

    /// Returns the type a name stands for, if it is a type parameter.
    #[must_use]
    pub fn type_of(&self, name: &str) -> Option<&Type> {
        match self.bindings.get(name) {
            Some(TemplateArgument::Type(ty)) => Some(ty),
            _ => None,
        }
    }

    /// Every value parameter that is bound to a number, with that number.
    ///
    /// Used where an expression is folded under an instantiation: the folder
    /// reads values from a table of constants, not from the substitution, so the
    /// substitution's numbers have to be published to it.
    #[must_use]
    pub fn int_bindings(&self) -> Vec<(String, i64)> {
        self.bindings
            .iter()
            .filter_map(|(name, argument)| match argument {
                TemplateArgument::Int(value) => Some((name.clone(), *value)),
                _ => None,
            })
            .collect()
    }

    /// Returns the value a name stands for, if it is a value parameter.
    #[must_use]
    pub fn value_of(&self, name: &str) -> Option<&TemplateArgument> {
        match self.bindings.get(name) {
            Some(argument @ (TemplateArgument::Int(_) | TemplateArgument::Text(_))) => {
                Some(argument)
            }
            _ => None,
        }
    }

    /// Rewrites a written type, replacing every template parameter in it.
    ///
    /// Used where a type is written rather than resolved: a `sizeof(T)` or a
    /// cast target inside a template body. A type that is *resolved* goes
    /// through the checker instead, which substitutes as it resolves and so
    /// never has to write a type back as text.
    ///
    /// A binding that is not a type -- a value parameter -- leaves the name
    /// alone here; it is replaced where it is *used*, as a value.
    #[must_use]
    pub fn substitute_written_type(&self, ty: &TypeSyntax) -> TypeSyntax {
        match ty {
            // A type parameter is replaced by a *name* the instantiation
            // registers, not by the type itself.
            //
            // The two are equivalent and the name is the one that fits: the
            // AST holds names, the checker holds what they mean, and keeping
            // the resolved type out of the tree means every later pass reads
            // one representation rather than two. It is also what lets a
            // template be substituted before its argument types are even
            // known -- which is what deduction needs.
            // A type parameter cannot be written back as a name, because the
            // name it would be written as does not exist in the program. Such a
            // use is resolved by the checker rather than by rewriting syntax;
            // this path leaves it named, and a `sizeof(T)` inside a template is
            // substituted by the caller that knows the type.
            TypeSyntax::Named(_) => ty.clone(),
            TypeSyntax::Array(array) => TypeSyntax::Array(fallc_ast::ArrayType {
                len: array.len,
                len_span: array.len_span,
                element: Box::new(self.substitute_written_type(&array.element)),
            }),
            // The parts of a function type are substituted like any other
            // written type: `fn(T) T` inside a template means the argument's
            // type on both sides.
            TypeSyntax::Function(function) => {
                TypeSyntax::Function(fallc_ast::FunctionTypeSyntax {
                    abi: function.abi.clone(),
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| self.substitute_written_type(parameter))
                        .collect(),
                    variadic: function.variadic,
                    throws: function.throws,
                    returns: Box::new(self.substitute_written_type(&function.returns)),
                    span: function.span,
                })
            }
            // `type(auto)` has no part to substitute: its type comes from the
            // initialiser, which the instantiation's own checker sees.
            TypeSyntax::DecltypeAuto(_) => ty.clone(),
            TypeSyntax::UnsizedArray(inner) => {
                TypeSyntax::UnsizedArray(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::Pointer(inner) => TypeSyntax::Pointer(Box::new(self.substitute_written_type(inner))),
            TypeSyntax::PointerToConst(inner) => {
                TypeSyntax::PointerToConst(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::ConstPointer(inner) => {
                TypeSyntax::ConstPointer(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::Reference(inner) => {
                TypeSyntax::Reference(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::ConstReference(inner) => {
                TypeSyntax::ConstReference(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::RvalueReference(inner) => {
                TypeSyntax::RvalueReference(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::ConstRvalueReference(inner) => {
                TypeSyntax::ConstRvalueReference(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::Volatile(inner) => {
                TypeSyntax::Volatile(Box::new(self.substitute_written_type(inner)))
            }
            TypeSyntax::Const(inner) => TypeSyntax::Const(Box::new(self.substitute_written_type(inner))),
            // `type(expr)` names no parameter, so substituting into it changes
            // nothing.
            TypeSyntax::Member(member) => TypeSyntax::Member(fallc_ast::TypeMember {
                outer: Box::new(self.substitute_written_type(&member.outer)),
                name: member.name.clone(),
                span: member.span,
            }),
            TypeSyntax::TypeOf(_) | TypeSyntax::Auto(_) | TypeSyntax::Unresolved(_) => {
                ty.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_ast::TemplateParamKind;
    use fallc_span::Span;

    fn param(name: &str) -> TemplateParam {
        TemplateParam {
            name: name.to_owned(),
            name_span: Span::SYNTHETIC,
            kind: TemplateParamKind::Type,
            is_pack: false,
            default: None,
            span: Span::SYNTHETIC,
        }
    }

    /// A trailing pack of types, as `template <Args ...type>` declares.
    fn pack(name: &str) -> TemplateParam {
        TemplateParam {
            is_pack: true,
            ..param(name)
        }
    }

    #[test]
    fn a_trailing_pack_takes_every_argument_left_over() {
        // A pack binds to a *list*, including the empty one, so a template with
        // one pack accepts any number of arguments. Nothing else may be short
        // or long: there would be no parameter for the extra argument.
        let params = vec![pack("Args")];
        let many = Substitution::new(
            &params,
            &Arguments(vec![
                TemplateArgument::Int(1),
                TemplateArgument::Int(2),
            ]),
        )
        .expect("a pack accepts any number");
        assert_eq!(
            many.get("Args"),
            Some(&TemplateArgument::Pack(vec![
                TemplateArgument::Int(1),
                TemplateArgument::Int(2)
            ]))
        );

        let none = Substitution::new(&params, &Arguments(Vec::new()))
            .expect("a pack accepts none");
        assert_eq!(none.get("Args"), Some(&TemplateArgument::Pack(Vec::new())));
    }

    #[test]
    fn a_fixed_parameter_still_has_a_fixed_arity() {
        // With no pack, the count must match exactly -- including when a pack
        // follows, where only the part before it is required.
        let params = vec![param("T"), pack("Rest")];
        assert!(Substitution::new(&params, &Arguments(vec![TemplateArgument::Int(1)])).is_some());
        assert!(Substitution::new(&params, &Arguments(Vec::new())).is_none());
    }

    #[test]
    fn a_substitution_binds_each_parameter() {
        let params = vec![param("T"), param("U")];
        let arguments = Arguments(vec![
            TemplateArgument::Type(Type::INT),
            TemplateArgument::Type(Type::BOOL),
        ]);
        let substitution = Substitution::new(&params, &arguments).expect("counts match");
        assert_eq!(substitution.type_of("T"), Some(&Type::INT));
        assert_eq!(substitution.type_of("U"), Some(&Type::BOOL));
        assert_eq!(substitution.type_of("V"), None);
    }

    #[test]
    fn the_wrong_number_of_arguments_does_not_match() {
        let params = vec![param("T")];
        let arguments = Arguments(vec![]);
        assert!(Substitution::new(&params, &arguments).is_none());
    }

    #[test]
    fn arguments_of_different_kinds_are_different_keys() {
        // `1` and `"1"` must not be the same instantiation, or one would
        // silently stand in for the other.
        let number = Arguments(vec![TemplateArgument::Int(1)]);
        let text = Arguments(vec![TemplateArgument::Text(b"1".to_vec())]);
        assert_ne!(number.key(), text.key());
    }

    #[test]
    fn the_same_arguments_are_the_same_key() {
        let left = Arguments(vec![TemplateArgument::Type(Type::INT)]);
        let right = Arguments(vec![TemplateArgument::Type(Type::INT)]);
        assert_eq!(left.key(), right.key());
    }
}
