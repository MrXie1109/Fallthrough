# Fallthrough Language Standard

> [中文](README.zh.md)

This English text is the **primary** text of the standard. Every volume has a Chinese
translation named `*.zh.md`; where the two differ, this English text prevails, and a
translation is corrected rather than the other way round. A document links to the
volumes in its own language: English pages link `NN-*.md`, Chinese pages link
`NN-*.zh.md`.

## Scope

This document and its volumes constitute the **normative text** of the Fallthrough
language. It specifies:

- the lexical structure, syntactic structure and static semantics of a translation unit
  that conforms to this standard;
- the run-time behavior of a program that conforms to this standard, and the conditions
  under which behavior is not specified (undefined behavior);
- the constructs that an implementation must diagnose, and the categories of
  information that a diagnostic must carry.

This document **does not specify**:

- the contents and behavior of the standard library; the standard library is a library
  shipped with the implementation, and its specification is given in
  [`../collections.md`](../collections.md), [`../library.md`](../library.md) and
  [`../c-bindings.md`](../c-bindings.md);
- the exact spelling of the compiler's command line (its contract is given in
  [00 General provisions](00-start.md));
- ABI details of the target platform; this standard requires only that a program
  preserve the observable behavior specified in this document within the limits the
  platform ABI allows.

## Normative references

The provisions of the following documents become provisions of this document through
reference. A reference that states a version applies only in that version; a reference
that states no version applies in its latest version.

- the volumes themselves: [00](00-start.md) through
  [10](10-diagnostics.md), together with
  [Appendix A](appendix-a-errors.md) (diagnostic codes),
  [Appendix B](appendix-b-special-methods.md) (special methods, and the forms the
  implementation supplies for a type that declares none) and
  [Appendix C](appendix-c-attributes.md) (attributes).
- [`../SPEC-STATUS.md`](../SPEC-STATUS.md): the list of entries not yet finalised.
- [`../templates.md`](../templates.md): supplementary rules for parameter packs,
  folding and `sizeof...`.

## Terms and definitions

The following terms and definitions apply to this document.

| Term | Definition |
| ---- | ---------- |
| Translation unit | the compilation unit formed by one `.ft` source file together with all modules it statically imports |
| Module | a named set of declarations declared in an `.ftm` file that can be imported with `import` |
| Program | the executable image formed by linking one or more translation units together |
| Binding | a storage location with a name and a type, introduced by a declaration |
| Place | addressable storage: a binding, a field, the result of a subscript, `*p`, a call that returns a reference |
| Value | the result of an expression that is not a place; also called an rvalue |
| Dying value | a value whose life ends when the expression ends: a temporary, the result of a call |
| Nominal type | a type introduced by a `type`, `union`, `enum`, `interface` or `traits` declaration |
| Special method | a member function spelled `__name__`, called by the language at particular constructs |
| trait | a set of required names; whether a type satisfies it is answered by the implementation at compile time |
| Instantiation | one copy of a function or type corresponding to one set of template arguments |
| Implementation | the compiler that concretises this standard, together with the standard library it ships |

## Conformance

4.1 A **program that conforms to this standard** must use only the constructs specified
in this document, and must not rely on behavior that this document specifies as
undefined.

4.2 An **implementation that conforms to this standard** must:

- accept a program that conforms to this standard and produce the observable behavior
  specified in this document;
- diagnose a program that violates a constraint of this standard, unless this document
  explicitly permits other behavior;
- report diagnostics under the numbered categories specified in this document (see
  [10 Diagnostics](10-diagnostics.md)).

4.3 An implementation may impose resource limits on a program that this document does
not specify (time, memory, recursion depth, number of template instantiations, and so
on). On reaching a limit, the implementation must issue a diagnostic and must not
silently produce a result that violates this standard.

4.4 An implementation may make choices where this document does not specify them, but
those choices must be stated in the implementation's documentation and must not change
the observable behavior of a program that conforms to this standard.

## Normative keywords

This document uses the following keywords, with fixed meanings:

- **must**, **must not**: a normative requirement. A violation makes a program or an
  implementation non-conforming.
- **should**, **should not**: a normative recommendation. A violation does not change
  the meaning of a program, but an implementation should state the difference in its
  documentation.
- **may**: a permitted choice.
- **undefined behavior**: this document does not specify the behavior; a program must
  not rely on it.
- **unspecified behavior**: this document permits an implementation to choose one of
  several possible results, but must not produce a crash or a result that violates type
  safety.

Code examples are always marked `ft`. Examples are **informative** and serve only to
illustrate provisions; where an example conflicts with the body of a provision, the body
prevails. In examples, `...` denotes an omission and is not part of the language.

## Organization of this document

| Volume | Subject |
| ---- | ------- |
| [00 General provisions](00-start.md) | Source files, translation units, the program entry, the compilation interface (executable, object, static or shared library), consuming diagnostics |
| [01 Lexical](01-lexical.md) | Source text encoding, tokens, comments, identifiers, keywords, literals, operators |
| [02 Types](02-types.md) | The type system, layout and alignment, type equivalence, implicit adjustments |
| [03 Declarations](03-declarations.md) | Declaration forms, bindings and initialization, attributes, macros, traits, interface, templates |
| [04 Functions](04-functions.md) | Functions, member functions, lambda, call conventions, argument passing, overloading, moves |
| [05 Statements](05-statements.md) | Blocks, conditions, loops, range loops, `switch`, jumps, `defer`, compile-time assertions |
| [06 Exceptions](06-exceptions.md) | The exception model, `raise`, `try`, `??`, `!!`, obligations at a call site |
| [07 Expressions](07-expressions.md) | Expression forms, conversions, operators, assignment, special methods, evaluation order |
| [08 Modules](08-modules.md) | Importing and exporting, name identity, module lookup, multi-file compilation and linking |
| [09 Scopes](09-scopes.md) | Scopes, shadowing, repeated declarations, `_`, lifetimes |
| [10 Diagnostics](10-diagnostics.md) | Diagnostic numbers, structure, warning groups, suppression, color, deduplication |

## Reading order

A reader meeting the language for the first time can take the volumes in order; the
first three are enough to read a program, and the rest are needed to write one:

1. [00 General provisions](00-start.md) and [01 Lexical](01-lexical.md) — what a source
   file is and how it becomes tokens;
2. [02 Types](02-types.md) and [03 Declarations](03-declarations.md) — the type system
   and how names are introduced;
3. [04 Functions](04-functions.md), [05 Statements](05-statements.md) and
   [07 Expressions](07-expressions.md) — behavior;
4. [06 Exceptions](06-exceptions.md), [08 Modules](08-modules.md) and
   [09 Scopes](09-scopes.md) — the rules a larger program needs;
5. [10 Diagnostics](10-diagnostics.md) and the appendices — reference tables.

## Revision of this edition

This edition differs from the previous one in the following ways; each is normative
text, not a note.

| Change | Where |
| ------ | ----- |
| English is the primary text; the Chinese translation is `*.zh.md` | every volume |
| `defer { … }` runs a block when its scope ends | [05 §8](05-statements.md#defer), [09 §5.7](09-scopes.md#lifetime-and-destruction) |
| `export` is a keyword again, not an attribute; `@export` is a diagnostic | [01 §5](01-lexical.md#keywords), [03 §9](03-declarations.md#attributes) |
| `try` may carry several `except` clauses, named and bare, tried top to bottom | [06 §5](06-exceptions.md) |
| type traits are answered by `std.type_traits` families; `$MOVES(T)` is the constant-position wrapper around `std.util.move_can_raise<T>()` | [03 §11.4](03-declarations.md#built-in-type-traits) |
| a by-value aggregate parameter is copied by its copy constructor and destroyed at every way out of the function | [04 §4.3](04-functions.md#semantics-of-aggregate-arguments-passed-by-value) |
| a compilation may produce a static or shared library as well as an executable or object | [00 §4.1](00-start.md#compilation-interface) |
| the diagnostic appendix is a single deduplicated table generated against the compiler's codes | [Appendix A](appendix-a-errors.md) |
| the special-method appendix states the default forms, not only the declared ones | [Appendix B](appendix-b-special-methods.md) |
| every attribute is listed once, with where it may be written | [Appendix C](appendix-c-attributes.md) |

## Entries not yet finalised

Entries not covered by this standard are marked *Open* in
[`../SPEC-STATUS.md`](../SPEC-STATUS.md). On encountering such an entry, an
implementation **must** report an internal-error diagnostic (`I0999`) and must not guess
at a semantics. An internal-error diagnostic means that the implementation is
incomplete, not that the program fails to conform to this standard.
