# 02 Types

> [中文](02-types.zh.md)

## Scope

This volume specifies the type system: the classification of types, their notation,
layout and alignment, type equivalence, and the implicit type adjustments that the
language permits. Type syntax appears in every declaration position; the forms of
declarations themselves are given in [03 Declarations](03-declarations.md).

## Type syntax

2.1 A type is written after the name being declared: `x int` declares an `int` named
`x`. This order applies to bindings, parameters, return types, fields, template
parameters, constants and trait parameters.

2.2 Type syntax consists of the following:

| Form | Meaning |
| ---- | ---- |
| a built-in type name | see Section 3 |
| `*T`, `*const T`, `const *T` | pointer |
| `&T`, `&const T`, `&&T`, `&&const T` | reference |
| `[N]T`, `[]T` | array |
| `fn(Ps) R`, `fn "ABI" (Ps) R`, `fn(Ps) R except` | function type |
| `*fn(...)...` | function pointer |
| `Name`, `Outer.Name`, `Family<Args>` | nominal type |
| `type(expr)` | the type of an expression |
| `?T` | optional-value sugar |
| `auto`, `type(auto)` | only for declarations and return types whose type is deduced |

2.3 The qualifiers `const` and `volatile` are written before the type and constrain the
layer of indirection that follows them. Declaration modifiers (`static`, `mutable`,
`@align`) are written after the type, see
[03 Declarations](03-declarations.md#declaration-modifiers).

## Basic types

| Name | Specification |
| ---- | ---- |
| `i8`, `i16`, `i32`, `i64`, `i128` | signed integers, with the respective fixed widths |
| `u8`, `u16`, `u32`, `u64`, `u128` | unsigned integers, with the respective fixed widths |
| `int`, `uint` | the same width as `i32` and `u32` respectively |
| `isize`, `usize` | pointer width; the result type of lengths, subscripts and `sizeof` is `usize` |
| `byte` | an 8-bit raw memory unit; **not** an alias of `u8` |
| `bool` | the boolean type |
| `c8`, `c16`, `c32` | character types; `char` is equivalent to `c8` |
| `f32`, `float` | single-precision floating point |
| `f64`, `double` | double-precision floating point |
| `void` | the no-value type; used only as a return type |

3.1 `byte` takes no part in arithmetic and does not convert implicitly to or from `u8`;
a conversion between the two **must** be written `as`.

3.2 Character types take no part in arithmetic; to operate on code points, an explicit
conversion to an integer type **must** be written.

3.3 The representation of signed integers is implementation-defined, but this
implementation uses two's complement. Integer overflow **must** produce a diagnostic or
a controlled failure in the default mode, and is undefined behavior in `--release` mode.

## Function types

### Forms

| Spelling | Meaning |
| ---- | ---- |
| `fn(int) int` | the type of a function taking an `int` and returning an `int` |
| `*fn(int) int` | a pointer to that function type |
| `fn(int) int except` | a function type that may raise |
| `fn "C" (int) int` | a function type using the C ABI |

4.1.1 A function type consists of the sequence of parameter types, the return type, the
calling convention, and whether it may raise.

4.1.2 `except` is part of the function type. A function type that may raise and one that
does not are mutually incompatible, and **must not** be converted implicitly in either
direction.

4.1.3 A call through a function pointer has the same call-site obligations as a direct
call (see [06 Exceptions](06-exceptions.md)).

### Conversion from a function name to a pointer

4.2.1 Where a function pointer is required, a function name is **adjusted** to a pointer
to that function; `&f` produces the same pointer.

4.2.2 An overloaded function name is resolved to a unique overload by the target
function type. When it cannot be determined uniquely, the program does not conform to
this standard.

4.2.3 A function pointer is called with function-call syntax: `p(1)`; `(*p)(1)` is not
a spelling permitted by this standard.

## Pointers

### Forms and meaning

| Spelling | Meaning |
| ---- | ---- |
| `*T` | a pointer to a writable `T` |
| `*const T` | a pointer to a read-only `T` |
| `const *T` | the pointer itself is read-only; the pointed-to object may be written |

5.1.1 A qualifier constrains the layer that follows it: `*const T` constrains the
pointed-to object, and `const *T` constrains the pointer itself.

5.1.2 `&x` produces a **pointer** of type `*T`, not a reference. The operand of
address-of **must** be a place.

### Pointer arithmetic

Pointer arithmetic is in units of elements, scaled by the implementation according to
the size of the pointed-to type.

| Spelling | Offset type | Result |
| ---- | -------- | ---- |
| `p + n`, `n + p` | `usize` | a pointer of the same type as `p`, `n` elements forward |
| `p - n` | `usize` | a pointer of the same type as `p`, `n` elements backward |
| `p - q` (same pointed-to type) | — | `isize`, the difference in element count |
| `p += n`, `p -= n` | `usize` | assignment forms |
| `++p`, `p++`, `--p`, `p--` | — | move one element |
| `p[i]` | `usize` | equivalent to `*(p + i)` |

5.2.1 The offset type **must** be `usize`. An unsuffixed integer literal takes `usize`;
other integer types **must** be converted explicitly.

5.2.2 The result of pointer arithmetic preserves the qualifiers of the original pointer.

5.2.3 Pointer arithmetic **does not check bounds**: pointing to any element of an array,
or to the position one past the last element, is legal; any other behavior is undefined
behavior. This rule is independent of the optimization level.

## References

### Forms and creation

| Spelling | Meaning |
| ---- | ---- |
| `&T` | a reference; the name denotes a piece of storage |
| `&const T` | a read-only reference |
| `&&T` | an rvalue reference; binds a dying value |
| `&&const T` | a read-only rvalue reference |

6.1.1 A reference appears only in type position. The language has **no** operator that
produces a reference.

6.1.2 The type of `*p` (where `p` is `*T`) is `&T`: dereference yields the pointed-to
storage itself, not a copy.

6.1.3 `&&` in source text has exactly one meaning: rvalue reference. Reference
collapsing occurs only on deduced types (template arguments, the result of `type(expr)`):
when two references are stacked, one of them is taken.

6.1.4 `*` **must not** be applied to a reference: a reference already names a value, so
the expression is used directly.

6.1.5 An expression of type `&T` reads as `T` where a value is required. This rule
allows `__deref__` and `__getitem__`, which return references, to be used for reading
and assignment.

6.1.6 Implicit address-taking in parameter position: a value of type `T` may satisfy a
parameter of type `&T` or `&const T`. This adjustment holds **only** in parameter
position.

6.1.7 An rvalue reference binds only dying values; binding an lvalue to an rvalue
reference is an error.

## Arrays

### Forms

| Spelling | Meaning |
| ---- | ---- |
| `[N]T` | `N` `T`s stored contiguously; `N` is a constant expression |
| `[]T` | unknown length; as a parameter type it is equivalent to `*T` |
| `[N]const T` | the elements are read-only |

7.1.1 Array elements are inlined in the array object, with no level of indirection.

7.1.2 In argument position an array is **adjusted** to a pointer to its first element,
`*T`.

7.1.3 An array **must not** be returned by value. When a function's return type has
array form, the implementation **must** report a diagnostic.

### Subscripts

7.2.1 For `[N]T` and `*T`, the subscript type **must** be `usize`; out of bounds is
undefined behavior.

7.2.2 A nominal type may declare `__getitem__` to specify its own subscript type and
out-of-bounds behavior, see [07 Expressions](07-expressions.md#special-methods).

7.2.3 The language has no slice type and no tuple type. `[]T` is only a parameter
spelling; multiple values are carried by a nominal type.

## Nominal types

### Declaration forms

```ft
type Point {
    x int;
    y int;
}

union Value {
    i int;
    b byte;
}

enum Color int {
    RED,
    GREEN = 5,
    BLUE,
}

type Id = usize;
```

8.1.1 `type Name { … }` declares a structure: fields are laid out in the order written.

8.1.2 `union Name { … }` declares a union: the fields share the starting address, the
width is that of the widest member, and the active member is guaranteed by the program.
Reading an inactive member is undefined behavior.

8.1.3 `enum Name Base { … }` declares an enumeration: the members are named constants of
type `Base`; `Base` **must** be an integer type, and is `int` when omitted. A member
with no value written equals the previous member's value plus one, and the first member
is 0.

8.1.4 `type Name = Other;` declares an alias: it introduces no new type, and the two
names denote the same type.

### Self-reference

8.2.1 A type **must not** contain itself by value. Mentioning itself through a pointer
or a reference is allowed.

8.2.2 An incomplete type may be used only in pointers, references and declarations;
defining a value, `sizeof`, accessing a member and passing by value **must** require a
complete type (see [03 Declarations](03-declarations.md#forward-declarations)).

### Nested types

8.3.1 A nominal type may declare in its body aliases that belong to that type, named
`Outer.Name`.

8.3.2 A nested type is not a field: it occupies no storage and takes no part in
initialization.

8.3.3 For a type family, a nested type belongs to the **instance**: `Cell<int>.Element`
and `Cell<u8>.Element` are different types.

8.3.4 This standard permits only nested **aliases**. Nested structures, unions,
enumerations and templates are outside the scope of this standard, and an
implementation **must** report a diagnostic.

## Layout and alignment

### Layout

9.1.1 The layout of a nominal type follows these rules, the same as C:

1. a field is placed at the next multiple of its alignment requirement, with padding
   inserted where necessary;
2. the whole type is padded to the alignment of its widest field.

9.1.2 `sizeof(T)` is the padded number of bytes; `alignof(T)` is the alignment of the
type.

9.1.3 The alignment of a union equals the largest alignment among its members;
`sizeof` is the width of the widest member after padding.

9.1.4 Layout is fixed at compile time; a program must not rely on the contents of
padding bytes that this standard does not specify.

### `@align`

9.2.1 A field or variable may raise its alignment with `@align(N)`; `N` **must** be a
positive power of two and a compile-time constant. `@align(T)` is equivalent to
`@align(alignof(T))`.

9.2.2 `@align` **may only** raise alignment; an `@align` that requires less than the
type's own alignment is an error.

9.2.3 A raised alignment propagates to subsequent fields and determines the alignment
and `sizeof` of the whole type.

## Const and volatile

10.1 `const T` means read-only: it must not be written through that name. `volatile T`
means that every access must really occur. The two may be combined.

10.2 `const` is shallow: it constrains only the layer through which a write passes, and
does not propagate to a pointed-to or referenced object.

10.3 A `const` object **must not** call a member function whose receiver is `this &T`; a
member function whose receiver is `this &const T` may be called. A `mutable` field is
the only exception.

## Type equivalence

11.1 The following type pairs are **equivalent**: `int` and `i32`; `uint` and `u32`;
`char` and `c8`; `float` and `f32`; `double` and `f64`; an alias and the type it names.

11.2 `const T` and `T` are the same for type equivalence and differ only in the
writability constraint; `*T` and `*const T` are different types.

11.3 Nominal types with the same name in two different translation units are the same
type if they come from the same module declaration; if they are declared separately,
they are different types.

## Type conversions and implicit adjustments

12.1 The language has **no** implicit numeric conversion in the general sense. A
conversion between numeric types **must** be written `as`; the only exception is the
**integer promotion** specified in 12.5.

12.2 The following **implicit adjustments** are all the implicit type changes this
standard permits; no other implicit type change may occur:

| Adjustment | Position |
| ---- | ---- |
| truth test | condition position |
| an array decays to a pointer to its first element | expression and argument position |
| a function name decays to a function pointer | position requiring a function pointer |
| the address of a value is passed to a reference parameter | parameter position |
| adding a `const` qualifier | pointers, references and objects |
| a pointer and a reference of the same shape are mutually compatible | pointer/reference parameter position |
| `*T` and `*byte` convert to each other | pointer position |
| a value is boxed into an `interface` it satisfies | initialization and assignment |
| a literal takes the type required by context | literal position |
| integer promotion (see 12.5) | a position requiring the target integer type |
| an `__im_init__` of the target type | a position requiring the target type (see [07 Expressions](07-expressions.md#implicit-initialisation-and-implicit-conversions)) |
| an `__im_as__` of the source type | a position requiring the target type (see [07 Expressions](07-expressions.md#implicit-initialisation-and-implicit-conversions)) |
| the optional-value syntax `?T` expands to a library type | type position |

12.3 Removing `const` **must** be written `as`.

12.4 A lossy numeric conversion wraps to the target width; this is the semantics of the
conversion itself, not an error.

### Integer promotion

12.5 Integer promotion is a **lossless** implicit integer conversion: when every value
of the source integer type falls within the value range of the target integer type,
implicit promotion is permitted.

12.5.1 The types participating in integer promotion **must** be integer types: `i8`,
`i16`, `i32`, `i64`, `i128`, `u8`, `u16`, `u32`, `u64`, `u128`, `int`, `uint`, `isize`,
`usize`. The following types **do not participate** in integer promotion, either as a
source type or as a target type:

- `bool`;
- the character types `c8`, `c16`, `c32`;
- `byte`;
- enumerations and other nominal types.

12.5.2 Let the source type be `S`, the target type be `T`, and `width(X)` be the bit
width of a type. The value of `S` may be implicitly promoted to `T` only when one of the
following conditions holds:

| Condition | Example |
| ---- | ---- |
| `S` and `T` are both unsigned, and `width(S) <= width(T)` | `u8` → `u16`, `u32` → `u64` |
| `S` and `T` are both signed, and `width(S) <= width(T)` | `i8` → `i16`, `i32` → `isize` |
| `S` is unsigned, `T` is signed, and `width(S) < width(T)` | `u8` → `i16`, `u32` → `i64` |

12.5.3 **A signed integer must not be converted implicitly to an unsigned integer**:
`i8` → `u8`, `i32` → `u64` and the like must be written `as`. This is a natural
consequence of 12.5.2 (a negative value is not in the target's value range); it is
stated separately because it is the most easily misused case.

12.5.4 An integer conversion that does not satisfy 12.5.2 (same width with different
signedness, narrowing, signed to unsigned) **must** be written `as`. An explicit `as`
permits any numeric direction; for its truncation and wrapping semantics see 12.4.

12.5.5 Integer promotion applies in positions that require the target integer type:
binding initialization, assignment, argument binding, return values, field initializers
and construction arguments. Promotion does not change operator selection: the operands
of arithmetic and bitwise operations **must not** become different types by virtue of
integer promotion, and must still have the same type or use an explicit `as`. The rules
for comparison operations are given in [07 Expressions](07-expressions.md) and are
not affected by this clause.

12.5.6 An unsuffixed integer literal takes the type required by context; this is the
type selection of the literal, not integer promotion.

## `type(expr)`

13.1 `type(expr)` produces a type and may appear only in type position.

13.2 When the operand is a bare name or a member access, the result is the type with
which that **entity** is declared.

13.3 Otherwise the result is the type of the **expression**: an expression that names
storage gives `&T`, and every other expression gives `T`.

13.4 Reference collapsing applies.

13.5 The operand is not evaluated, but it is checked; errors in it **must** be reported.

13.6 A field declaration **must not** use `type(expr)`: the layout is determined before
any expression is checked.

## Optional values

14.1 `?T` is type sugar equivalent to the standard library's `Optional<T>`. The syntax
itself is part of this standard; the behavior of `Optional<T>` is specified by the
standard library.

14.2 When `?T` is used, the program **must** be able to import the module that declares
`Optional<T>`.
