# Appendix C Attributes

> [中文](appendix-c-attributes.zh.md)

## C.1 Scope

This appendix gathers every attribute the language accepts: where it may be written,
what it means, and what it constrains. The normative text is
[03 Declarations §9](03-declarations.md#attributes); this appendix is a reference table
and creates no rule of its own.

## C.2 General rules

C.2.1 An attribute is written `@name` or `@name(arguments)` and applies to the
declaration or branch it is written on.

C.2.2 An attribute in a position that does not allow it, an unknown name, a duplicate
attribute, or a wrong number or kind of arguments is an error; an implementation **must
not** silently ignore it ([03 Declarations §9.3](03-declarations.md#attributes)).

C.2.3 An attribute is a **request**, never a promise the caller can rely on: `@inline`
and `@cold` suggest, and a conforming implementation may ignore either.

## C.3 Functions

| Attribute | Arguments | Meaning |
| --------- | --------- | ------- |
| `@inline` | — | ask for inlining; code generation may still decline |
| `@noinline` | — | ask against inlining |
| `@noreturn` | — | the function never returns normally; its return type **must** be `void` |
| `@must_use` | — | the result of a call **must** be used; dropping it is a warning |
| `@cold` | — | the function is rarely called; code generation may put it on a slow path |
| `@deprecated` | — or one string | warn at each use; the string, when given, is the message |
| `@enable_if(cond)` | one condition | the declaration exists only when `cond` is true; `cond` **must** be a compile-time constant |
| `@constexpr` | — | the function may be called in a constant position |
| `@extern_C` | — | use the C ABI: the symbol is not mangled |

C.3.1 `@noreturn` on a function returning anything but `void` is an error.

C.3.2 `@enable_if` is decided once per instantiation. For a template it is evaluated with
the deduced arguments, which is what makes a member exist for some arguments and not for
others.

## C.4 Variables and constants

| Attribute | Arguments | Meaning |
| --------- | --------- | ------- |
| `@inline` | — | the same variable may be defined in several translation units |
| `@deprecated` | — or one string | warn at each use |
| `@align(N)` | one integer | alignment requirement in bytes; `N` **must** be a power of two |
| `@constexpr` | — | compile-time-constant initialiser; the binding is also `const` |
| `@static` | — | static storage duration (a local that outlives its scope) |
| `@thread_local` | — | one object per thread |
| `@uninitialized` | — | no zero fill and no default construction; **no** initialiser allowed; **locals only** |
| `@maybe_unused` | — | do not report the never-read warning |
| `@extern_C` | — | the variable's storage belongs to another language, found by an unmangled name |

C.4.1 `@uninitialized` on a module-level variable is an error: that variable lives in the
program image, which the loader zero-fills before the program starts, so there is no
storage whose contents could be left alone.

C.4.2 `@uninitialized` and an initialiser cannot both be written.

## C.5 Types

| Attribute | Applies to | Meaning |
| --------- | ---------- | ------- |
| `@deprecated` | type, type family, trait, interface, enum, alias | warn at each use |
| `@must_use` | type declaration | a call returning this type **must** have its result used |

C.5.1 `export` is not an attribute in this sense, though it is written in the same
position: it is part of the declaration's own syntax
([08 Modules](08-modules.md#the-export-table)).

## C.6 Branches

| Attribute | Position | Meaning |
| --------- | -------- | ------- |
| `@likely` | `if`, `else`, `case` | this branch is usually taken |
| `@unlikely` | `if`, `else`, `case` | this branch is rarely taken |
| `@constexpr` | after the condition of an `if` only | the condition **must** be a compile-time constant, and the branch not taken is not checked |

C.6.1 A `@constexpr` `if` is how a program selects code at compile time: the branch not
taken must still parse, but it is not type-checked, so it may name things that do not
exist for the program being built.

C.6.2 `@likely` and `@unlikely` carry no meaning the program can observe; they are hints
for the layout of the generated code.

## C.7 Summary

| Attribute | Function | Variable | Type | Branch |
| --------- | :------: | :------: | :--: | :----: |
| `@inline` | ✓ | ✓ | | |
| `@noinline` | ✓ | | | |
| `@noreturn` | ✓ | | | |
| `@must_use` | ✓ | | ✓ | |
| `@cold` | ✓ | | | |
| `@deprecated` | ✓ | ✓ | ✓ | |
| `@enable_if` | ✓ | | | |
| `@constexpr` | ✓ | ✓ | | ✓ |
| `@extern_C` | ✓ | ✓ | | |
| `@align(N)` | | ✓ | | |
| `@static` | | ✓ | | |
| `@thread_local` | | ✓ | | |
| `@uninitialized` | | ✓ | | |
| `@maybe_unused` | | ✓ | | |
| `@likely` | | | | ✓ |
| `@unlikely` | | | | ✓ |
