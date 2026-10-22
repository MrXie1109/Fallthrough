//! Mapping Fallthrough types to LLVM types.
//!
//! Because the IR *is* LLVM IR, every Fallthrough type must eventually become
//! an [`inkwell::types::BasicTypeEnum`] (or an aggregate built from them). This
//! module is where that mapping lives.
//!
//! An integer of N bits maps to `iN`; LLVM integers are sign-agnostic, so
//! signedness shows up only in which instruction is chosen. `float`/`double`
//! map to `f32`/`f64`, a pointer or a reference to an LLVM pointer (opaque, so
//! the pointee lives only in the Fallthrough type), and `[N]T` to an LLVM
//! array.
//!
//! A record maps to an LLVM struct whose members follow declaration order,
//! which is the layout `sizeof`/`alignof` report. A field that asked for
//! `alignas` is preceded by a zero-length alignment marker, so the request
//! raises the alignment without occupying bytes; a union is a one-member struct
//! over its largest member, because every access reinterprets the same bytes.

use inkwell::context::Context;
use inkwell::types::{BasicType as _, BasicTypeEnum};

use fallc_sema::Type;
use fallc_span::Span;

/// Returns the alignment `name` asked for with `alignas`, if any.
///
/// A request of one byte is not a request: it is the alignment everything has.
/// The pair is `None` when nothing was asked for, or when the request cannot
/// raise anything -- which is why it can be used as "does this field need a
/// marker".
fn extra_alignment(nominal: &fallc_sema::NominalType, name: &str) -> Option<u32> {
    nominal
        .field_alignments
        .iter()
        .find(|(field, _)| field == name)
        .and_then(|(_, value)| u32::try_from(*value).ok())
        .filter(|alignment| *alignment > 1)
}

/// Builds a zero-length array whose alignment is `alignment` bytes.
///
/// LLVM gives an array its element's alignment, and a vector of `n` bytes has
/// `n` bytes of alignment, so `[0 x <n x i8>]` is a value that occupies
/// nothing and aligns what follows it to `n`.
fn alignment_marker(context: &Context, alignment: u32) -> BasicTypeEnum<'_> {
    context
        .i8_type()
        .vec_type(alignment)
        .array_type(0)
        .into()
}

/// Returns the LLVM member index of a declaration's field.
///
/// A field that asked for an alignment is preceded by a marker member, so the
/// index shifts by the number of earlier fields that asked for one. A union
/// keeps its one storage member first, which is where every member points.
#[must_use]
pub fn llvm_member_index(nominal: &fallc_sema::NominalType, index: usize) -> u32 {
    if nominal.is_union {
        return 0;
    }
    let markers = nominal
        .fields
        .iter()
        .take(index)
        .filter(|(name, _)| extra_alignment(nominal, name).is_some())
        .count();
    u32::try_from(index.saturating_add(markers)).unwrap_or(0)
}

/// Maps Fallthrough types to LLVM types.
///
/// Holds a reference to the LLVM context because every type it produces is
/// created from that context.
#[derive(Clone, Copy, Debug)]
pub struct TypeMapper<'ctx> {
    context: &'ctx Context,
}

impl<'ctx> TypeMapper<'ctx> {
    /// Creates a mapper over `context`.
    #[must_use]
    pub fn new(context: &'ctx Context) -> Self {
        TypeMapper { context }
    }

    /// Returns the LLVM context.
    #[must_use]
    pub fn context(&self) -> &'ctx Context {
        self.context
    }

    /// Maps a Fallthrough type to an LLVM basic type.
    ///
    /// Returns `None` only when there is no representation to give: a type the
    /// checker could not resolve (`Type::Unspecified`), or a zero-length array,
    /// which LLVM refuses.
    #[must_use]
    pub fn map(&self, ty: &Type) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            // `volatile T` occupies exactly what `T` does: the qualifier is
            // about how the memory is accessed, not how it is arranged.
            Type::Volatile(inner) => self.map(inner),
            Type::Int(int) => {
                let bits =
                    std::num::NonZeroU32::new(int.bits()).expect("no integer type has zero width");
                // `custom_width_int_type` is fallible: LLVM accepts only a
                // defined set of widths, so `i7` would be refused here.
                self.context
                    .custom_width_int_type(bits)
                    .ok()
                    .map(Into::into)
            }
            // `byte` is a distinct type in the type system but shares `u8`'s
            // representation. That is a lowering detail: the two are not
            // interchangeable at the source level.
            Type::Byte => Some(self.context.i8_type().into()),
            // `bool` lowers to `i1`, which is what LLVM's `br` condition
            // requires, so a `bool` condition needs no conversion.
            Type::Bool => Some(self.context.bool_type().into()),
            // `nullptr` becomes a null of whatever pointer type it was used
            // at; on its own it has no representation, and the checker only
            // lets it appear where a pointer is wanted.
            Type::NullPointer => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // A character type has its base integer's representation. The
            // distinctness from that integer is a type system matter, not a
            // representation one.
            Type::Character(character) => {
                let bits = std::num::NonZeroU32::new(character.bits())
                    .expect("no character type has zero width");
                self.context.custom_width_int_type(bits).ok().map(Into::into)
            }
            Type::Float(float) => Some(match float {
                fallc_sema::FloatType::Single => self.context.f32_type().into(),
                fallc_sema::FloatType::Double => self.context.f64_type().into(),
            }),
            // LLVM has opaque pointers, so a pointer's LLVM type does not
            // record its pointee. The Fallthrough type keeps that information
            // for the type checker; codegen only needs the address.
            Type::Pointer { .. } | Type::Reference { .. } => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // A function type has no representation of its own: code is not a
            // value. What a program can hold is a *pointer* to a function, and
            // that is `Type::Pointer` around this -- which is why a function
            // name used as a value decays to an address.
            //
            // The address this returns is what a declaration's signature uses
            // for an ABI that speaks in pointers, and what a parameter of a
            // function type becomes.
            Type::Function { .. } => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            Type::Array { element, len } => {
                let element = self.map(element)?;
                let len = u32::try_from(*len).ok()?;
                // LLVM rejects a zero-length array. `[0]T` is legal in C as an
                // extension but is not specified here, so it is refused.
                if len == 0 {
                    return None;
                }
                Some(element.array_type(len).into())
            }
            // An unsized array is only valid as a parameter type, where it has
            // already been turned into a pointer.
            Type::UnsizedArray(_) => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // A const-qualified value has the same representation as its
            // unqualified form: constness is a property of the type system, not
            // of the layout.
            Type::Const(inner) => self.map(inner),
            // A nominal type is an LLVM struct with one member per field, in
            // declaration order, because a positional initialiser assigns by
            // position and field access uses the index.
            Type::Nominal(nominal) => {
                // A type declared but not defined has no layout: only a pointer
                // or a reference to it may be written, so asking for its LLVM
                // type means something upstream let an incomplete type through.
                if nominal.is_incomplete {
                    return None;
                }
                // A union's members share one address, so its representation is
                // the *largest* member alone: every access reinterprets the
                // same bytes, and the offsets are all zero.
                //
                // Encoding it as a one-member struct rather than as LLVM's own
                // union keeps a single rule for offsets -- field `i` of a struct
                // is index `i`, and field `i` of a union is index `0` -- which
                // is what field access and initialisers already assume.
                if nominal.is_union {
                    // Which member the storage is represented by was decided by
                    // the checker, which knows each field's size: measuring one
                    // here would need target data this mapper does not have.
                    //
                    // The storage member comes first, so that every member is
                    // reached at index zero whatever the markers do; they are
                    // zero bytes, so they cost nothing and cannot move it.
                    let mut members = Vec::with_capacity(nominal.fields.len());
                    let storage = match nominal
                        .union_storage
                        .and_then(|index| nominal.fields.get(index))
                    {
                        Some((_, field_ty)) => self.map(field_ty)?,
                        // An empty union has no members to overlap, so it
                        // occupies nothing.
                        None => return Some(self.context.struct_type(&[], false).into()),
                    };
                    members.push(storage);
                    // An alignment any member asked for raises the union's,
                    // because the members overlap: the request applies to the
                    // object they share.
                    for (name, _) in &nominal.fields {
                        if let Some(alignment) = extra_alignment(nominal, name) {
                            members.push(alignment_marker(self.context, alignment));
                        }
                    }
                    return Some(self.context.struct_type(&members, false).into());
                }
                // A field that asked for an alignment is preceded by a
                // *zero-length* marker of that alignment. Such a marker
                // occupies no bytes but raises the alignment of what follows
                // it, and of the struct as a whole, which is exactly what
                // `alignas` means -- and it needs no target data to compute,
                // because the marker's size is zero by construction.
                //
                // Only fields that asked for something get one, so a struct
                // without `alignas` is represented exactly as before. The
                // member index is translated by [`llvm_member_index`].
                let mut members = Vec::with_capacity(nominal.fields.len() * 2);
                for (name, field_ty) in &nominal.fields {
                    if let Some(alignment) = extra_alignment(nominal, name) {
                        members.push(alignment_marker(self.context, alignment));
                    }
                    members.push(self.map(field_ty)?);
                }
                Some(self.context.struct_type(&members, false).into())
            }
            // The placeholder `String` that builtin signatures are written
            // with has no layout of its own, so it is passed as a pointer to
            // its bytes. A program's `std.String` is a nominal type declared in
            // the standard library and maps as a record, like any other.
            Type::String => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // A name the checker could not resolve has no representation; the
            // message that says so comes from [`TypeMapper::unmappable`].
            Type::Unspecified(_) => None,
        }
    }

    /// Reports that a type cannot be mapped, naming the type and its location.
    #[must_use]
    pub fn unmappable(&self, ty: &Type, span: Span) -> String {
        match ty {
            Type::Unspecified(u) if u.text.is_empty() => {
                format!("the type at {span:?} is unresolved, so it has no LLVM representation")
            }
            Type::Unspecified(u) => format!(
                "the type `{}` at {span:?} is unresolved, so it has no LLVM representation",
                u.text
            ),
            other => format!("the type `{}` at {span:?}", other.spelling()),
        }
    }

    /// Returns `i1`, the condition type LLVM's `br` and `select` require.
    ///
    /// `bool` maps to it, so a `bool` condition needs no conversion; every
    /// other truthy value is brought to `i1` where a condition is lowered.
    #[must_use]
    pub fn condition_type(&self) -> BasicTypeEnum<'ctx> {
        self.context.bool_type().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    #[test]
    fn unspecified_types_do_not_map() {
        let context = Context::create();
        let mapper = TypeMapper::new(&context);
        let ty = Type::unspecified("i32", Span::SYNTHETIC);
        assert!(mapper.map(&ty).is_none());
    }

    #[test]
    fn unmappable_message_names_the_type_and_the_marker() {
        let context = Context::create();
        let mapper = TypeMapper::new(&context);
        let span = Span::new(FileId::from_raw(0), 3, 6);
        let message = mapper.unmappable(&Type::unspecified("Foo", span), span);
        assert!(message.contains("Foo"), "{message}");
        assert!(message.contains("no LLVM representation"), "{message}");
    }

    #[test]
    fn condition_type_is_llvm_i1() {
        let context = Context::create();
        let mapper = TypeMapper::new(&context);
        // LLVM's `br` and `select` require i1 conditions.
        assert_eq!(
            mapper.condition_type(),
            BasicTypeEnum::IntType(context.bool_type())
        );
    }
}
