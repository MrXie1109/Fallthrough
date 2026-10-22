//! The Fallthrough semantic analyzer.
//!
//! Two passes, in this order, because name resolution must happen before
//! types can be attached to names:
//!
//! 1. [`resolve`] walks the AST and produces a [`Resolutions`] table mapping
//!    each name-occurrence to a [`DefId`], reporting unknown names and
//!    duplicate definitions.
//! 2. [`typeck`] computes a [`TypeTable`] mapping each expression to its
//!    [`Type`], using the resolutions from pass 1.
//!
//! Keeping the two passes separate matters: resolution needs only the scoping
//! rules, so a name error is reported once and without a type in hand, and the
//! type checker can assume that every name it meets already has a definition.

#![forbid(unsafe_code)]

pub mod builtins;
pub mod interp;
pub mod resolve;
pub mod template;
pub mod typeck;
pub mod types;

pub use builtins::{BUILTINS, Builtin, BuiltinType};
pub use resolve::{
    definition_name,
    Def, DefId, DefKind, ExportEnum, ExportInterface, ExportInterfaceMethod, ExportType,
    ExportedField, ExportedGlobal, ExportedSpecialisation, ExportedVariable,
    ExternalModule,
    FunctionSignature,
    ImportBinding, Resolutions, ResolvedSignature, Resolver, ScopeId,
};
pub use template::{Arguments, Substitution, TemplateArgument};
pub use typeck::{
    template_symbol_of, unspecified_signature_type,
    deprecated_message,
    InPlaceOperation,
    ClosureCall, ClosureInfo,
    Instantiation, InstantiationRecords, InterfaceCall, InterfaceConversion, InterfaceSource,
    LambdaInstance,
    OperatorCall, TemplateBody, TraitBody,
    ExpressionDefault, ExpressionDefaults, TraitOutcome,
    TypeCheckResult, TypeChecker, TypeEnvironment, is_const_widening};
pub use types::{
    BindingTypes, CharType, ConversionMethod, FloatType, IntType, NominalType, Type, TypeTable,
    UnspecifiedType,
    is_byte_pointer_cast, is_byte_pointer_conversion, mangle_type,
};
