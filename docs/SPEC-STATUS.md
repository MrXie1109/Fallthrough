# Language specification status

This file records the decisions the Fallthrough language is made of, and the
handful of things it deliberately does not provide. The reference for the
language itself is [`language/`](language/README.md).

The rule for this repository: **when a design question is unanswered, the code
does nothing and says so.** It does not guess. A wrong guess is worse than a
missing feature, because a wrong guess compiles, passes tests, and silently
shapes every later decision. Now that the answers are in, the same rule applies
to the other direction: the code and the documents say the same thing, and a
test pins every rule a program can observe.

## How to use this file

The language designer answers questions in the order below. As each answer
arrives, the corresponding code is filled in and the row is marked done. The
"Blocks" column says what cannot proceed until that answer exists.

## Open questions

### 1. Lexical structure — `crates/fallc-lexer/src/token.rs`

| # | Question | Status |
| - | -------- | ------ |
| 1.1 | The `TokenKind` set | **Partly answered.** Ident, String, Keyword, Punct, Eof, Unknown. |
| 1.2 | Control-flow keywords | **Answered.** 15 keywords, in `KEYWORDS`. |
| 1.3 | Control-flow punctuation | **Answered.** `( ) { } ; : , . =`, in `PUNCTUATION`. |
| 1.4 | Declaration keywords | **Partly answered.** `fn`, `as`, `auto`. |
| 1.5 | Operators, precedence, and maximal munch | **Answered.** C's set and C's precedence. |
| 1.6 | Numeric literals | **Answered.** Decimal, hex, octal, and binary integers; floats; type suffixes; `_` separators. |
| 1.7 | Character and byte literals | **Answered.** `'x'` is a `c8`; a width prefix (`u16'x'`) gives a `c16`. `b'x'` is **withdrawn**. |
| 1.7a | Raw and interpolated strings; escape sequences in strings | **Partly answered.** Escapes are decoded in literals and strings. Raw strings are settled: `r"..."` and `r#tag"..."tag#`, where the tag may hold no whitespace or `"`, is at most 64 bytes, and is repeated at the closing delimiter; no escape is processed and a newline is an ordinary character. Multi-line strings are `""".."""`: the text may span lines, escapes still apply, and no indentation is trimmed. Interpolated strings remain open. |
| 1.8 | Comment syntax | **Answered.** `//` and non-nesting `/* */`. Whether they are preserved as trivia is still open. |
| 1.9 | Non-ASCII identifiers | **Answered.** UAX #31 `XID_Start` / `XID_Continue` (plus `_`); no normalization is performed. |
| 1.10 | Contextual vs reserved keywords | **Answered.** `dynamic` and `static` are contextual: they mean something only after `link`, where they choose the kind of library, and are ordinary names everywhere else. `delete` is contextual too, recognised by position. Every keyword in `KEYWORDS` is reserved. |
| 1.11 | Preprocessor, shebang, other non-token text | **Partly answered.** Macros are C-style: `define NAME body` to the end of the line (`\` continues it), `export define` exports, invocation is `$NAME` or `$NAME(args)` wherever an expression or a token may appear, expansion is recursive with a depth limit, `#param` stringifies, `a ## b` pastes, and `...` / `__VA_LIST__` are variadic. A shebang and other non-token text remain open. |

### 2. Grammar — `crates/fallc-parser/`, `crates/fallc-ast/src/node/`

| # | Question | Blocks |
| - | -------- | ------ |
| 2.1 | Top-level constructs | **Partly answered.** Functions (with `@extern_C` and `export`), `import`, `link`, `type`, macros (`define` / `export define`), attributes (`@name` / `@name(args)`), module-level variables (including `@extern_C` external variables), constants, and `traits` are specified, as are templates (2.12). |
| 2.2 | Visibility/export syntax | **Answered.** `export` before a module-level declaration makes it visible to importers; without it the declaration is private to the module. |
| 2.3 | Attributes and where they may appear | **Answered.** Written before the declaration they describe, one or more of them: `@inline` and `@noreturn` on a function, `@deprecated` on a function or a module-level variable, `@align(...)` on a variable. A `@noreturn` function may not declare a return value. |
| 2.4 | **Control flow** (if/while/for/for-each/switch/break/continue/return/raise/try) | **Answered** — see below |
| 2.5 | **`fallthrough`**: statement form and position | **Answered.** `fallthrough;`, inside `switch`; falls to the next case |
| 2.6 | Binding declarations | **Answered.** `name type;`, `name type = expr;`, `name type = .{..};`, `name auto = expr;`. A binding may carry the postfix declaration modifiers `static` and `@align(...)` after its type. See below. |
| 2.7 | Assignment | **Answered.** `x = y;`, plus compound `+= -= *= /= %= &= \|= ^= <<= >>=`. |
| 2.7a | Element syntax inside `.{ ... }` | **Answered.** The elements are expressions, in field order, and a nested `.{ ... }` stands for a nested value. `.{` is one token, and the raw text between the braces is kept only so a diagnostic can quote what was written. |
| 2.7b | Whether `auto` may be deduced in more places (parameters, return types, fields) | **Answered.** `auto` is deduced in a binding's initialiser and as a function's return type, and nowhere else: there is no inference beyond `auto`. |
| 2.8 | Expression statements and calls | **Answered.** Any expression may be a statement. Calls parse and type-check. |
| 2.9 | Whether blocks are expressions, and whether a trailing expression is the block's value | Open |
| 2.9a | Whether `{ ... }` on its own is a *statement* (`{ String s = ...; }`, and `case X: { ... }` in a `switch`) | **Answered.** Yes: a block is a statement, with a scope of its own. `StmtKind::Block`, parsed in `parse_stmt`, and a value it declares is destroyed at the closing brace |
| 2.10 | The expression grammar | **Answered.** Literals, names, calls, field access, `->`, indexing, designated initialisers, aggregate construction, calls through a function pointer, `lambda` literals and calls of a closure. |
| 2.11 | The full operator table: spelling, precedence, associativity | **Answered.** C's spelling, precedence and associativity, plus `**` (exponentiation); the table is in `language/01-lexical.md`. |
| 2.12 | Generics/parameters on items, and their syntax | **Answered.** `template <T>`, `template <t T>`, `template <values ...int>`, `template <Args ...type>` before a declaration; `Name<...>` names a member of a type family; `Args...` expands a type pack; `values + ...`, `sizeof...(values)` and `values[1]` read a value pack. A function parameter may also be a pack (`values ...Ts`, `values ...&Ts`, `values ...int`), expanded per call, with `f(values...)` in argument position and `(statement, ...)` as a comma fold. See `docs/templates.md` and `docs/language/04-functions.md`. |
| 2.13 | Condition typing: truthiness is allowed, so what counts as truthy? | **Answered.** Numeric non-zero is true and `bool` is itself; a non-null pointer is true; a reference is tested through its referent; an array is always true (it is viewed as a pointer); anything else needs `__bool__`. |

#### Type syntax, as specified

Types are written with **prefix** operators, unlike declarations, whose type
follows the name (`x int`, `x [3]int`):

| Syntax | Meaning |
| ------ | ------- |
| `T` | a named type |
| `[N]T` | fixed-size array of `N` elements |
| `[]T` | in a parameter list, equivalent to `*T` |
| `fn(T) R` | a function taking a `T` and returning an `R` |
| `*fn(T) R` | a pointer to such a function |
| `fn "C" (T) R` | the same function type with the C ABI |
| `*T` | pointer to mutable `T` |
| `*const T` | pointer to constant `T` — the pointee cannot be written |
| `const *T` | constant pointer to mutable `T` — the pointer cannot be reassigned |
| `&T` | reference, implemented as a pointer |
| `&const T` | reference to a constant referent |
| `const T` | an immutable **binding** |

`const` binds to whatever follows it, so `*const T` and `const *T` differ.
`const T` is a separate use of the keyword: it qualifies the binding, not a
type, and such a binding must be initialised and cannot be assigned afterwards.

Functions decay to pointers: a function name may be used where a `*fn(...) ...`
is wanted, and `&f` is accepted as the same pointer. A function pointer is
called as `p(1)`, with no dereference written. When `f` is overloaded, the
target type selects the overload.

`N` may be any constant integer expression; literals and their arithmetic are
folded. A non-constant length is an error.

**Answered.** `&x` yields `*T`, and a `*T` converts to a `*const T` implicitly:
adding const is safe, so it needs no cast. The reverse is not a conversion --
writing through a `*const T` is refused -- and `const *T` (a constant pointer)
still exists only as the type written in a declaration.

#### Operators, as specified

C's operator set with C's precedence and associativity. Two consequences worth
recording:

* `&&` and `||` **short-circuit**, so they lower to basic blocks and a `phi`
  rather than to LLVM's `and`/`or`, which always evaluate both sides.
* Bitwise operators bind **looser** than comparisons, as in C. So
  `3 & 1 == 1` is `3 & (1 == 1)`.

#### Overflow and division by zero, as specified

| Build | Integer overflow | Division by zero |
| ----- | ---------------- | ---------------- |
| debug (default) | panic | panic |
| `--release` | undefined | undefined |

The panic target is `std.panic`, which `standard/std/util.ftm` defines (the
message on standard error, then abort). The guards still call
`runtime/builtins.c`'s `__builtin_panic`, which does the same thing; the two
names exist because the guard is emitted by code generation, which cannot depend
on a module the program may not have imported.

Unsigned arithmetic is well defined as wrapping, so guarding it would reject
valid programs; only signed operations are checked.

#### Comparison across arithmetic types, as specified

Comparison is the **only** operation that mixes arithmetic types. All six
comparison operators accept operands of different types, widths, and
signedness, including floats against integers, and compare them **by
mathematical value**.

The signed/unsigned case cannot be done with one instruction, because neither
operand's signedness describes the relationship: widening both to signed makes
a large unsigned value look negative, and widening to unsigned does the same to
a negative signed value. The rule the compiler implements is:

* if the signed operand is **negative**, the unsigned operand is larger, so the
  answer is known without examining it (and equality is false either way);
* otherwise the signed operand is non-negative, both fit a common unsigned type,
  and an unsigned comparison is exact.

So `-1i8 < 1u32` is **true**, where C — which widens to unsigned — says false.

Arithmetic is unaffected: `+` and friends still require exactly matching types.

#### Builtins, as specified

Builtins are callable from source and need no declaration. The compiler holds a
table of names and signatures, so adding one is a single table entry rather
than a change in the resolver, the checker, and the code generator separately.

| Builtin | Signature | Purpose |
| ------- | --------- | ------- |
| `__builtin_panic` | `(*const c8) void` | abort with a message; the compiler's stand-in for `std.panic` |
| `str` | `(any) String` | render a value as text |
| `repr` | `(any) String` | render a value unambiguously |

`__builtin_panic` is what debug-mode overflow and division guards call, because
code generation cannot depend on a module the program may not have imported;
`std.panic` in `standard/std/util.ftm` is the same operation for a program to
call. `str` and `repr` are lowered by the compiler: a type that declares
`__str__`/`__repr__` is asked for it, and everything else is written down by
rules about what the value means (see `language/08-expressions.md`).

`std.String` exists. A string literal is `[N]const c8` and constructs a
`std.String` where that type is required, so a literal is constructible,
indexable and printable through the standard module.

#### Overloading and default arguments, as specified

Two functions may share a name when their **parameter types** differ. The
return type is not part of a signature, so two functions differing only in it
could never be told apart at a call and are rejected as duplicates.

A call picks a candidate by **exact** parameter match, since there are no
implicit conversions. The one preference the language has is that an
**unsuffixed integer literal prefers `int`**, so `f(int)` beats `f(u8)` for
`f(5)`. When several candidates fit equally well the call is **ambiguous and
reported**, rather than resolved by declaration order — which would silently
change meaning when a function is added.

A function name used where a function pointer is wanted is resolved from the
**target type**: `p *fn(int) int = &f;` selects `f(int) int` among the
overloads.

Overloaded functions are emitted under mangled symbols; a function that is not
overloaded keeps its plain name, which is what a C function's symbol must be.

A parameter may have a default, written `name type = expr`. Defaults must come
**last**, as in C++, and are evaluated **at the call site**, so a default is
equivalent to writing the argument at every call.

#### Type declarations and member functions, as specified

```text
type Point {
    x int;
    y int;
}

fn show(this *const Point) void {
    C.printf(b"(%d, %d)\n", this->x, this->y);
}

p Point = .{42, 37};
p.show();
```

A `type` declaration names a nominal type with named fields, in declaration
order — which is the layout, because a positional initialiser assigns by it.
Two declarations with the same fields are still different types.

**There is no visibility.** A field whose name begins with `_` is private by
convention only; the compiler does not enforce it.

A function is a **member function** when its first parameter is named `this`
and is a pointer. `value.method(args)` calls it with the receiver's address,
taken automatically, so a method works on a value and on a pointer alike.
`this` is a **keyword** so that this determination cannot depend on which names
a program happens to use.

`pointer->field` is sugar for `(*pointer).field`. A field is an lvalue, so
`this->x = v` writes it — unless the pointee is `const`, which is what makes
`*const T` meaningful on a receiver.

`C.printf(...)` and `p.show()` are written identically, so **name resolution**
decides: if the object names an imported module the member is that module's
symbol, and otherwise it is a field of a value.

**`.{ ... }` fills fields, and never calls `__init__`.** The brace form is the
positional/designated initialiser for a type's fields: a type that declares an
`__init__` still has its braces write the fields directly, which is what makes
`t T = .{1, 2};` usable without a constructor and what makes a constructor's
own work (validation, allocation) something the call form `T(1, 2)` asks for.
A type with no `__init__` is constructed by braces only. A constructor is
identified by its receiver, so several types may each have one and each gets its
own symbol.

**Answered:** a member function is declared outside the type block, with the
receiver as its first parameter named `this`; a type block holds fields and
nested types. That is settled, not open.

#### Modules, as specified

A module's name is its file's stem: `standard/C.ftm` is module `C`. A module's
declarations are **private by default**; `export` makes one visible to
importers, and nothing else is reachable. Every kind of declaration can be
exported -- a function, a type (fields, width and alignment included, template
parameters and specialisations), an enum (its base type and the folded value of
every member), an interface (its method list, order included), a trait (its
name; the requirements travel with the declaring module's tree), a constant (its
value and its type), a module-level variable (one object for the program, read
and written by the importer through its symbol), an `extern` variable (a name, a
type and a linker symbol), and a macro (`export define`; a macro is expanded
before parsing, so the modules are read at the token level -- the macros its
body uses travel with it, and a local `define` shadows an imported one). A
`@deprecated` declaration says so in the importing module too, and a declaration
`@enable_if` removed is not exported at all.

`import C;` names a module rather than a path. The loader looks beside the
importing file first, then in `standard/`. Imports are followed **transitively**,
so a module that imports another has its own imports available.

There are three spellings, and they differ in *what the importing file can
name*, not in what is loaded:

| Spelling | What the file may write |
| -------- | ----------------------- |
| `import C;` | `C.printf` |
| `import C as Libc;` | `Libc.printf` |
| `from C import printf, puts;` | `printf`, `puts` |
| `from C import *;` | every export of `C` |

An alias is only a local spelling: the module is still loaded under its own
name, so two files may give it different local names.

A name is written as an identifier, or in quotes when it could not be — because
it collides with an operator or a keyword, or is not an identifier at all:

```text
import C;
import "my-module";
```

The two spellings denote the **same** name. The quotes are an escape hatch for
the lexer, not a qualifier.

**A plain `from M import *` does not re-export.** It brings names into the file
and stops there; re-exporting is written `export from M import *;`, which is
what lets a module be assembled out of others:

```text
// standard/C.ftm, the declarations in full
export from C.stdio import *;
export from C.stdlib import *;
export from C.string import *;
export from C.ctype import *;
export from C.math import *;
export from C.assert import *;

link m dynamic from default;
```

`C` declares no function itself, so `C.printf` exists only because `C.stdio`
exports it and `C` forwards it. A plain `from M import *` would *not* forward
anything: it is a local convenience, and what a module publishes is always
written with `export`.

#### Link instructions, as specified

`link` says what the produced binary must be linked against. It is an item, at
the same level as `import`, so the instruction applies to the whole module and
does not depend on which function happens to call something.

```text
link m dynamic from default;
link my_lib static from "/path/to/my/lib";
link "gcc_s" dynamic from default;
```

All three parts are required:

| Part | Values |
| ---- | ------ |
| name | an identifier, or a quoted name that is not one |
| kind | `dynamic` (a shared object) or `static` (an archive) |
| `from` | `default` (the linker's own path) or a directory in quotes |

Nothing is optional, because both omissions would be consequential: a library
linked the wrong way is a run-time bug, not a compile-time one, and saying
`default` explicitly means a line always states where its library comes from.

The name is written **without** the `lib` prefix or any suffix, so `link m`
means `-lm`. A quoted name is the same name, not a literal file name; a
quoted name that could not be an identifier is still spelled the usual way at
link time.

A module's link instructions apply to whatever imports it. That is what retired
the hardcoded `-lm` the driver used to pass: `standard/C.ftm` now says

```text
link m dynamic from default;
```

and every program that calls `C.sqrt` is linked against `libm` without writing
anything itself.

An external function is declared with an ABI:

```text
@extern_C export fn printf(fmt *const c8, ...) int;
```

`@extern_C` selects the C calling convention, `export` makes it importable,
and `...` makes it variadic. A declaration with a semicolon and no braces has
no body: it is implemented elsewhere, and code generation emits an LLVM
declaration for it. Omitting the return type means the function returns nothing,
which is how a C `void` function is written.

#### Numeric literals, as specified

| Form | Type |
| ---- | ---- |
| `42` | `int`, or the type its context requires |
| `42u8` `42i64` `42usize` | the named type |
| `42u` `42i` | `uint` (`u32`) and `int` (`i32`) |
| `1.5` | `double` |
| `1.5f` | `float` |
| `1e10` `1.5e-3` | an exponent, which makes the literal a float |
| `0xFF` `0o17` `0b1010` | hex, octal, and binary, each with any suffix |
| `1_000_000` `0xFFFF_FFFF` | `_` between digits, ignored |

A separator must sit **between two digits**: one at the start, at the end, or
doubled is not part of the number, so `1_` is an integer followed by an
identifier and `_1000` is a name.

Digits of a radix literal belong to the number, so `0xFFu8` splits into `0xFF`
and `u8` rather than at the first letter.

#### Character and string literals, as specified

A character literal's type is its **width prefix**:

| Form | Type | Value |
| ---- | ---- | ----- |
| `'x'` | `c8` | one code unit |
| `u16'x'` | `c16` | one code unit |
| `u32'中'` | `c32` | one code unit |

`c8`, `c16`, and `c32` are based on `u8`, `u16`, and `u32` respectively and are
**distinct types**: none takes part in arithmetic, and each converts with `as`.
`char` is an **alias for `c8`**.

A literal whose code point does not fit its type is an error rather than a
truncation, and the diagnostic names the prefix to write instead:

```text
error[S0051]: the character `'中'` does not fit in `c8`; write it as `u16'中'`
```

A **string literal** is an array of characters whose element width comes from
the same prefix, and it **carries a terminating NUL** which counts in the
length:

| Form | Type | Contents |
| ---- | ---- | -------- |
| `"ab"` | `[3]const c8` | `a`, `b`, NUL |
| `u16"ab"` | `[3]const c16` | same, two bytes per element |
| `b"ab"` | `[2]const c8` | `a`, `b` — **no** NUL |

This is the whole difference between the two forms, and it is what makes an
ordinary string usable as a C string directly: `C.printf("hi")` needs no cast,
because `"hi"` decays to a `*const c8` that is already terminated.

Escapes are decoded in characters and strings alike: `\n`, `\t`, `\r`, `\0`,
`\xHH`, and `\u{...}`. The decoder lives in `fallc-ast` so that the type
checker's range check and code generation cannot come to disagree about what a
literal means.

A suffix is the literal's **type, not a conversion**. So `x u8 = 5u32;` is a
type mismatch rather than a silent truncation, and `300u8` is an error rather
than 44. This is the difference from treating `5u8` as `5 as u8`.

A float needs a decimal point or an exponent: `1f` is not a float, as in C,
because the suffix selects a float's type rather than making an integer into
one.

`byte` is not a valid suffix, since it takes no part in arithmetic. A plain
literal can still initialise one — `b byte = 1;` — because `byte` is an
acceptable context for an unsuffixed literal. Nor is a character type: a
character comes from a character literal and its prefix, not from a suffix.

An unsuffixed integer literal adopts its context's type, which is what makes
`return 7;` work in a `u8` function. A float literal does **not** adopt its
context: `1.5` in a `float` position would silently lose precision.

#### Comments, as specified

C style: `//` to the end of the line, and `/* */` which does **not** nest, so
the first `*/` closes it.

This was a genuine gap discovered late: every example in the repository had
comments from the start, and they only "worked" because those files had never
been compiled. `standard/C.ftm` was the first commented file to actually reach
the lexer.

#### The temporary C bindings

`standard/C.ftm` gathers the C bindings (`standard/C/stdio.ftm` and its
siblings), which declare a large part of the C standard library. The language
now has a standard library of its own (`std.Array`, `std.String`, `std.Box`
and the rest), so the C bindings are a compatibility layer rather than the only
library; they remain written in the language and kept as a separate module.

The bindings make one assumption: that a C `char *` is `*const c8` and a C
`size_t` is `usize`. Types with no equivalent — `FILE`, `va_list` — are
declared with `*c8` where a pointer appears, or omitted. Function pointers now
have their own type (4.3), so they no longer need that workaround.

An ordinary string literal satisfies a `*const c8` parameter directly, because
it carries its own terminator and decays to a pointer. `C.printf("hi")` needs no
cast and no `b` prefix.

#### Conversions as specified

There are **no implicit conversions** between values. A value must already have
the required type wherever it is used. The exceptions are these, and only these:

* a truthiness test in a condition, which accepts a numeric value and treats
  non-zero as true;
* an array decaying to a pointer to its first element, which is what lets a
  string literal reach a `*const c8` parameter;
* adding `const` to an indirection, because that only takes away the right to
  write;
* a **byte pointer** conversion: `*T` and `*byte` are interchangeable in both
  directions, without a cast.

The last of these is the one exception that does not preserve shape, and it is
deliberate: `*byte` is the language's untyped pointer, and it is what raw memory
and the C library traffic in. Every other pair of pointers to different types
still needs an explicit `as`, or every type error would become a memory error.

The `const` qualifiers still have to be respected across it. `*T` may become
`*const byte`, but a `*const T` may not become a `*byte` — that would hand out a
writable pointer to constant data. The const-dropping direction is written with
`as`.

Conversion is otherwise always written explicitly, as `expr as Type`. Legal
today: between any two numeric types, between `byte` and any numeric type, and
between an indirection and one of the same shape modulo `const`. A cast to a
type with no meaning is refused rather than guessed at.

#### Binding declarations as specified

| Form | Meaning |
| ---- | ------- |
| `name type;` | zero-initialised |
| `name type = expr;` | initialised from an expression |
| `name type = .{ ... };` | designated initialiser |
| `name auto = expr;` | type deduced from the initialiser |
| `name auto;` | **error** — nothing to deduce from |
| `name auto = .{ ... };` | **error** — a designated initialiser has no type of its own |

`.{` is a **single token**, so `.` stays unambiguous as member access; a space
between the two characters splits them back apart. Both `auto` failures report
the same diagnostic code, since they are the same failure.

#### Control-flow semantics as specified

* `if (c) { } else if (c) { } else { }`
* `while (c) { } else { }`, `for (i; c; u) { } else { }`, `for (e T : range) { } else { }`
  — the `else` clause runs on **normal termination only**. `break`, `return`,
  and `raise` skip it; `continue` does not count as an interruption.
* `switch (e) { case a: ... case c, d: ... default: ... }` — labels may appear
  in any position and must be **constants**; cases **do not fall through**
  unless the arm ends with `fallthrough;`.
* `for (e T : range)` — the range form of C++'s range-for, and now answered.
  The loop is *desugared* into ordinary code while the file is parsed:
  `auto it = r.begin(); auto end = r.end(); for (; it != end; ++it) { T e = *it; ... }`
  (the range is bound to a temporary only when it is not already a place, so it
  is evaluated once). Every one of the five operations is an ordinary one, so a
  type is iterable by declaring them: `std.Array` and `std.String` do, and the
  iterators they hand out are plain pointers (`*T`, `*char`), which is the top
  of the hierarchy `std/iterator.ftm` declares: `Iterable → InputIterator →
  ForwardIterator → BidirectionalIterator → RandomAccessIterator`. There is no
  `ContiguousIterator`: adjacency is a claim about addresses, and no operation on
  a position states it. `x &int` binds the element, `x int` a copy, `x auto` a copy as well
  (`auto` deduces the value, not the reference). The `else` clause follows the
  same rule as the other loops.
* `raise "type": "what";` — the type is required and the message optional
  (`raise "Type";`); both are `std.String` and copied into the exception. Bare
  `raise;` re-raises what an `except` caught.
* `try { } except { } else { } finally { }` — `else` and `finally` are
  optional, and any number of `except` clauses may follow, tried top to bottom.
  A bare `except` catches everything; `except ("KeyError")` / `except ("A", "B")`
  catches only those names, compared with the exception's `name()`. An exception
  no clause matches keeps propagating (this try's `finally` runs first). A try
  whose clauses are all named needs a function that can raise. The only capture
  form is an ordinary typed declaration inside an `except` block.
* `except` after a return type: the function may raise. `except(cond)` claims it
  may raise only when `cond` holds; the claim is folded at each call site with
  the arguments, and folded to "may raise" when they are not constants.
  `call()??` re-raises at the mark -- it means `try { call() } except { raise; }`
  -- so a `try` with an `except` around it in the same function catches it; only
  when nothing there does, the exception leaves the function. A call to a
  may-raise function has to be inside a `try` with an `except`, or followed by
  `??`; both `raise` and `??` are legal inside such a `try` or in an `except`
  function.
* A construction or another *implicit* member call is a call site too: the
  initialiser of `T x = expr??;` carries the propagation marker, because there
  is no call to hang it on. Assignments, operators, indexing, conversions,
  `__bool__`, `__call__` and `str`/`repr` follow the same rule -- each is
  protected where it is written.

### 3. Name resolution — `crates/fallc-sema/src/resolve.rs`

| # | Question | Blocks |
| - | -------- | ------ |
| 3.1 | Which declarations bind names | resolve |
| 3.2 | Scoping model: block-scoped, function-scoped, or both | **Answered.** Block-scoped. Each block, `if`/loop body, `for` statement, `switch` arm block and bare `{ ... }` is a scope. An inner scope **shadows** an outer name (`x int = 1; { x int = 2; }` is two variables), and declaring the same name twice in *one* scope is an error. A parameter is in the function's outermost scope, so a top-level local may not reuse its name. |
| 3.3 | Declare-before-use; may items forward-reference each other? | resolve |
| 3.4 | Shadowing: allowed, warned, or forbidden? | resolve |
| 3.5 | Modules/namespaces and path resolution | resolve |
| 3.6 | Import/export semantics | **Answered.** Module name = file stem; private by default; `export` exposes. Three spellings: `import C;`, `import C as Libc;`, `from C import a, b;`, `from C import *;`, and `import "path";` names a file directly. A star import is re-exported, and both named and star imports accept every kind of export -- function, type, enum, interface, trait, constant, variable and macro. A type's methods are reachable by name in a module that imports it, because the definition table is what method dispatch and interface satisfaction read. |
| 3.7 | Case sensitivity of names | resolve |

### 4. Type system — `crates/fallc-sema/src/types.rs`, `typeck/`

#### Primitive types as specified

| Type | Meaning |
| ---- | ------- |
| `i8` `i16` `i32` `i64` `i128` | signed integers |
| `u8` `u16` `u32` `u64` `u128` | unsigned integers |
| `int` | alias for `i32` |
| `uint` | alias for `u32` |
| `isize` | alias for `i64` |
| `usize` | alias for `u64` |
| `float` | 32-bit float |
| `double` | 64-bit float |
| `byte` | **distinct type** based on `u8` |
| `char` | **alias** for `c8` |
| `c8` `c16` `c32` | **distinct types** based on `u8`, `u16`, `u32` |

Aliases resolve to their targets, so `int` and `i32` are the same type and lower
identically; diagnostics print the canonical spelling.

`byte` is *not* an alias. It is a distinct type that shares `u8`'s eight-bit
representation, and the two are incompatible without an explicit conversion —
in both directions. The shared representation is a lowering detail.

The character types are distinct in the same way, each sitting on the integer of
its width. They take no part in arithmetic and convert with `as`. Their width is
what the prefix of a literal names, which is how the same character can be held
in one, two, or four code units without a cast.

`char` was `[4]byte`, then withdrawn, then a UTF-32 code point; it is now an
alias for `c8`, which supersedes all three.

Still open below: whether these names can be shadowed by user types.

| # | Question | Blocks |
| - | -------- | ------ |
| 4.1 | Primitive types | **Answered.** `i8`–`i128`, `u8`–`u128`, `float`, `double`, `bool`, plus `isize`=`i64`, `usize`=`u64`, `int`=`i32`, `uint`=`u32`. `byte` is distinct, as are `c8`/`c16`/`c32`. |
| 4.2 | Arrays, slices, and tuples | **Answered.** `[N]T` exists, with decay to `*T` in an argument position; **slices and tuples are not provided**, and records (structs) are the only aggregate. Indexing takes `usize` (unsuffixed literals adopt it), and `[N]T`/`*T` are unchecked; a type may declare any index type through `__getitem__`. |
| 4.2a | Pointer arithmetic | **Answered.** `p ± n` with `n: usize` (unsuffixed literals adopt it), scaled by the pointee's size; `p - q` is an `isize` element count; `++`/`--` move one element and `+=`/`-=` are their assigning forms. Unchecked, C semantics: inside an array and one past its end is defined. Pointers already compare by address. |
| 4.3 | Function types and how they are written | **Answered.** `fn(T) R` is a function type, `*fn(T) R` a function pointer, and `fn "C" (T) R` a C-ABI function type. Functions decay to pointers (`&f` is accepted), a pointer is called as `p(1)`, and an overload is chosen from the target type. |
| 4.4 | Nominal types (struct/record/enum/...) | **Answered.** A nominal type is identified by its name, not by its fields; a field may name the type through a pointer or a reference, and by-value self-containment is refused. Layout follows C: each field at the next multiple of its alignment, the whole padded to its own. |
| 4.4b | Nested types | **Answered, aliases only.** `type Name = Other;` inside a `type` body declares a type owned by the outer one, named `Outer<args>.Name`; it is not a field and takes no storage. It belongs to the *member*, so `Cell<int>.Element` and `Cell<u8>.Element` differ. Nested structs, unions and enums are refused with a diagnostic until their construction rules exist. `std.Array` names its four iterator types this way. |
| 4.4a | `@align(N)` on a field | **Answered.** `@align(N)` is a postfix declaration modifier, written after the type: `b int @align(16);`. `N` is a compile-time constant and a positive power of two (`S0137`, `S0138`), it may not be weaker than the declaration's own alignment (`S0139`), the field and everything after it move to the next multiple, and the type's own alignment becomes the widest field's. On a variable it raises that variable's storage alignment; `@align(T)` means `@align(alignof(T))`. `sizeof`/`alignof` and code generation agree by construction: the field's slot is aligned in the representation, so no size is computed twice. Unions take the request from any member. Documented in `02-types.md`. |
| 4.5 | Generics and inference rules | types, typeck |
| 4.6 | References/pointers and the ownership or borrowing rules | types, typeck, ir |
| 4.6a | `type(expr)` — the type of an expression | **Answered.** `decltype`'s rule. A bare name or member access gives the entity's *declared* type; anything else gives the expression's: `&T` when it names storage, `T` when it is a value. References collapse (`&(&T)` is `&T`). Usable wherever a type is written, including a parameter that takes its type from an earlier one; refused in a `type` declaration's fields, whose layout is fixed before any expression is checked. |
| 4.7 | Conversions | **Answered.** No implicit value conversions except truthiness, array decay, function-to-pointer decay (`&f` is accepted), adding `const`, and the byte-pointer conversion. Explicit conversion is `expr as Type`. |
| 4.7a | Which `as` conversions are legal, and the result of a lossy one (`300 as u8`) | **Answered.** Numeric-to-numeric, `byte`-to-numeric, indirections of the same shape modulo `const`, and the byte-pointer conversion. A lossy conversion **wraps** to the target width: `300 as u8` is 44. |
| 4.7c | Comparison across arithmetic types | **Answered.** All operators, all widths, by mathematical value. See above. |
| 4.7b | Truthiness details beyond "non-zero is true" | **Answered.** Numeric non-zero is true and `bool` is itself; a non-null pointer is true; a reference is tested through its referent; an array is always true (it is viewed as a pointer). |
| 4.7d | traits — named sets of required operations | **Answered.** `traits Name(it T) { ++it, *it -> !void, it != it -> bool }`: requirements are *expressions* against a parameter, each optionally followed by `-> !void` (yields a value), `-> T` (yields exactly `T`), or `-> value` (has this compile-time value). A trait that builds on another says so as a requirement — `InputIterator(type(it)) -> true` — because a requirement another trait holds is a requirement like any other; there is no inheritance clause. Conformance is *structural*: a type models a trait exactly when every requirement compiles for it, so nothing declares conformance. A query is an expression of type `bool` folded where it stands; its arguments are *types* only, and `type(expr)` names the type of a value. Requirements are checked where a trait is asked about a type, never at the declaration. The iterator hierarchy is `Iterable → InputIterator → ForwardIterator → BidirectionalIterator → RandomAccessIterator`; there is no `ContiguousIterator`. |
| 4.8 | Numeric promotion and literal defaulting | **Answered.** An integer literal takes its context's type, so `return 7;` in a `u8` function is a `u8`, and `x auto = 7;` in that function makes `x` a `u8`; a wholly uncontextualised integer literal is `int`. There is no numeric promotion: arithmetic requires matching types, and comparison is the only operation that mixes them (4.7c). |
| 4.9 | Is there a bottom/never type, and what does it mean for control flow? | **Answered.** There is no bottom/never type. |
| 4.10 | Where, if anywhere, is type inference allowed? | **Answered.** Only `auto`: a binding's initialiser and a function's return type. There is no inference beyond `auto`. |
| 4.11 | Mutability/constness rules | **Answered.** `const` is shallow: a const object may not call a method whose receiver is `this &T`; `mutable` fields are the exception. `* & && const volatile` are prefix type qualifiers; `static`, `mutable` and `alignas` are postfix declaration modifiers. |

### 5. Intermediate representation — LLVM

The IR is LLVM IR, so the instruction set itself is not an open question:
LLVM defines it. What remains open is the *lowering* — how each Fallthrough
construct becomes LLVM IR. Directory: `crates/fallc-ir/src/codegen/`.

| # | Question | Blocks |
| - | -------- | ------ |
| 5.1 | For each AST construct, the LLVM IR it lowers to | codegen |
| 5.2 | The LLVM type each Fallthrough type maps to | **Answered.** A struct maps to the LLVM struct its fields give, and `sizeof`/`alignof` agree with that layout including padding. Integers map to `iN`, `float`/`double` to `f32`/`f64`, and `[N]T` to an LLVM array. |
| 5.3 | Whether integers are signed, and where signedness selects the instruction (`sdiv` vs `udiv`) | types.rs, codegen |
| 5.4 | Aggregate passing/returning conventions (by value vs by pointer) | types.rs, codegen |
| 5.5 | Globals and constants | **Answered.** A module-level variable is one object initialised once and emitted as a global; an external variable (`@extern_C`) is emitted as a declaration of the library's symbol, not a definition; a `const` binding supplies a compile-time constant where one is required. |
| 5.6 | **`fallthrough`** and control-flow lowering | **Answered.** if/else → blocks and `br`; loop `else` = normal-termination edge, `break` = end block; `switch` arms get an automatic break, `fallthrough` branches to the next arm. |
| 5.7 | Calling conventions and the `main` ABI | codegen |
| 5.8 | Evaluation order and short-circuit semantics (LLVM guarantees no evaluation order, so if Fallthrough does, lowering must encode it) | codegen |
| 5.9 | Integer overflow and division by zero | **Answered.** Debug: `std.panic`. Release: undefined. |
| 5.10 | Debug information (`!dbg` metadata) | codegen |

### 6. Driver and toolchain — `crates/fallc/src/`

| # | Question | Blocks |
| - | -------- | ------ |
| 6.1 | Source file extension | **Answered.** `.ft` is a program or one translation unit of one; `.ftm` is a module, which may be imported and may not be compiled as a program by itself. A `.ft` file is not importable. |
| 6.2 | Module-to-file mapping | **Answered.** A module's name is its file's stem, and a dot is a directory: `std.string` is `std/string.ftm`. A name quoted in an `import` denotes the same name as one written as an identifier. |
| 6.3 | Flag set | **Answered.** `--emit=exe\|obj\|static\|shared\|llvm\|ast\|types\|tokens\|all`, `-o`, `-O0`–`-O3`, `-Os`, `-Oz`, `--release`, `--check`, `--keep-temporaries`, `-Wverbose`, `--color=<when>`, and warning control: `-W<group>`, `-Wno-<group>`, `-Wall`, `-Werror`. The groups are the eleven in `language/11-diagnostics.md`, which is the authority for them. The compiler targets the host only: there is no cross-compilation or target-selection flag. |
| 6.4 | Code generation backend | **Answered.** LLVM, via `llc`-free object emission and a C compiler as the linker driver. |
| 6.5 | Compiling imported modules' bodies | **Answered.** Each module is resolved, type checked, and lowered alongside the unit, so a function a module defines is emitted. |
| 6.6 | Multi-file compilation and the One Definition Rule | **Answered.** Several inputs are one program: each is a translation unit, and they are linked. A symbol may be defined once across the program and declared any number of times. |
| 6.7 | Optimisation levels | **Answered.** `-O0`–`-O3`, `-Os`, `-Oz` as a bare `-O`/`-O<level>` flag, passed to LLVM's `default<O..>` pipeline. Independent of `--release`, which decides what the source means rather than how hard LLVM works. |

### Known gaps

These are not open questions with an unknown answer; they are things the
specification deliberately does not provide, or that the implementation does
not cover yet, recorded so that they are not mistaken for working:

* **No slices or tuples.** Records (structs) are the only aggregate. `[N]T`
  exists and decays to `*T`; there is no `[]T` value type and no tuple type.
* **Host-only targeting.** The compiler emits for the host it runs on; there
  is no cross-compilation and no target-selection flag.
* **No inference beyond `auto`.** `auto` is the only deduced type: a binding's
  initialiser and a function's return type. Nothing else is inferred.
* **No bottom type.** There is no `never` type; `return`, `raise`, `break`,
  `continue` and `fallthrough` are statements, not expressions with a type.
* **Macros carry no type safety.** A macro is expanded as tokens before it is
  checked: the expansion is checked, but the macro itself has no signature and
  no parameter types.
* **A `.{ ... }` initialiser is not accepted as a `push` argument.** It needs a
  type from its context, and an argument position does not give one; bind it to
  a name first.

## The standard library

The library is written in the language itself, under `standard/`, and its
documentation is [`collections.md`](collections.md) (the two containers and the
other `std` entry points), [`library.md`](library.md) (everything else), and
[`c-bindings.md`](c-bindings.md) (the `import C;` surface). What exists:

| Module | Contents | State |
| ------ | -------- | ----- |
| `std.string` | `String`, `Exception`, and the string algorithms | complete |
| `std.array` | `Array<T>` and its four iterator names | complete |
| `std.algorithm` | the free ordering algorithms (`sort`, `sort_by`, `is_sorted`, `reverse`, `fill`, `find`, `index_of`, `count_of`, `swap`), over a `*T` range or a whole container | complete |
| `std.memory` | `Allocator`, `DefaultAllocator`, `Box<T>`, `Rc<T>`/`RcWeak<T>`, `Arc<T>`/`ArcWeak<T>` | complete |
| `std.util` | `alloc`/`allocate`/`calloc`/`realloc`/`free`, `panic`, `fake<T>`, `move<T>`, `Unit`, `__ft_moves<T>` | complete |
| `std.iterator` | the five iterator traits, as queries | complete |
| `std.io` | `File` (a path or a C stream, `OpenMode`, `SeekFrom`), `out`/`err`/`in`, `print`/`println`/`input` and the byte-level members; `std.print`/`std.input` as short forms over the standard streams | complete |
| `std.tree` | `Compare`, `Less`/`Greater`, `RBTree`, `TSet`/`TMap`/`TMSet`/`TMMap`, iterators | complete |
| `std.hash` | `Hashable`, `Hash<T>`, `HashTable`, `HSet`/`HMap`/`HMSet`/`HMMap`, iterators | complete |
| `std.deque` | `Deque`, `Queue`, `Stack`, forward-only iterators | complete |
| `std.heap` | `Heap`, `PriorityQueue` | complete |
| `std.sync` | `Mutex`, `Lock`, `CondVar`, `Thread`, `sleep`, `Atomic<T>` | complete, except that `Atomic` is mutex-backed |
| `std.os` | `pid`, `getenv`/`has_env`/`set_env`, `exit`, `cwd`/`chdir`, `temp_dir`/`home`, `monotonic_ns`, `sleep_ms`, `platform`/`arch`; Linux/POSIX, macOS and Windows backends | complete |
| `std.random` | `Random` (MT19937, entropy seeding, `uniform`, `uniform_real`, `normal`, `bernoulli`, `shuffle`) and the global `std.random` | complete |
| `std.fs` | `Path`, joining, splitting, existence, `remove`, `rename`, and `file(mode)` for opening the path as a `std.File` | complete |
| `std.math` | generic `min`/`max`/`clamp`/`abs`/`square`, and the C library at `f64` | complete |
| `meta` | the compiler macros `$meta.FILE`, `$meta.LINE`, `$meta.PLATFORM`, `$meta.WINDOWS` and the rest; a module of its own, not part of `std` | complete |

What the library does not have, recorded so that it is not mistaken for
working:

* **The tree and hash containers are not safe to copy.** `TMap`, `TSet`, `HMap`,
  `HSet`, `RBTree` and `HashTable` (and the `Queue`/`Stack` wrappers) have no copy
  control, so `n TMap = m;` copies their pointers member-wise and the two values
  free one buffer. Move them (`std.move`) or hold them through a pointer. `Heap`
  and `Deque` copies and assignments are deep; `File` is move-only with a
  destructor.
* **`(*it).data()` on a map iterator is an internal error**; bind the value
  first (`v String = *it; v.data()`).
* **A `Lock` guard warns as unused** (`S0140`): the warning does not know that
  the binding's destructor is the point. Write `guard Lock = Lock(&m);` and read
  the warning as noise for now.
* **`IOError` carries the path, not the system's error number**, so a program
  cannot tell "not found" from "permission denied" without asking the system
  itself.
* **`std.fs` provides no `mkdir`, `read_dir`, or stat**; a program that needs
  them declares the C calls itself.

* **`Atomic<T>` is mutex-backed**, so it is atomic but not lock-free; a
  genuinely lock-free one needs a language-level atomic builtin.
* **`Path` does not normalise.** It splits on `/`; `..` and `.` are left as
  written, and the file system is the only party that can say what a path
  means.
* **Hashed containers have no iteration order**, and a tree's entries with
  equal keys have no stable order among themselves.

## Answered

The rows above record the settled answers; the normative text for each is in
the volume under `language/` that it references.

### Later additions

Twelve decisions were taken after the first round and are implemented. Each has
its normative text in `language/`, and a test that pins what a program can
observe.

| # | Addition | Settled rule |
| - | -------- | ------------ |
| 66 | 类型特征改为标准库类型家族 + 模板特化：`IsInteger`/`IsFloating`/`IsNumeric`/`IsPointer`/`IsReference`/`IsVoid`/`IsBool`/`IsCharacter` 由 `std.type_traits` 的 `VALUE` 常量成员回答，程序也可定义同样形状的谓词；`IsArray`（偏特化 `IsArray<[]T>`）与 `IsSame`（偏特化 `IsSame<A, A>`）也用类型家族表达；移动是否可能抛出由 `std.util.move_can_raise<T>()` 回答，常量位置使用包装宏 `$MOVES(T)`（展开为 `except(..)` 查询） | 完成（`trait_specialisation` 测试；`traits`/`specialisation`/`multi_file`/`type_traits` 通过） |
| 1 | Template default arguments | `template <T type = int, n int = 4>`; a parameter with a default must be followed only by parameters that have one; explicit argument, then deduction, then the default. |
| 2 | Default arguments from earlier parameters | A function's default argument may use the parameters before it and is evaluated only when the argument is omitted; a template's default argument may use the template parameters before it. |
| 3 | Keyword arguments | `.name = value` after every positional argument, in any order, skipping defaulted parameters; the same form is accepted by `.{..}` initialisers and by `T(...)` constructor calls, and the parameter names of a function in another module travel with its signature. |
| 4 | `type(auto)` | `decltype(auto)`: the expression's own type, so a place stays a `&T` and `const` is kept; allowed for a binding and for a return type. |
| 5 | Omitted return type | A definition without one means `auto`; a declaration (including `@extern_C`) must write its type, `void` included. |
| 6 | Lambdas and closures | `lambda(args) -> expr` and `lambda(args) { ... }`; `lambda` is reserved; a parameter may omit its type and is instantiated per call; a call of a lambda needs parentheses around it; names are captured by reference; a lambda that captures nothing and does not raise *is* a function and decays to `*fn(...) ...`; any other lambda used as a value is a **closure** -- a value of an anonymous aggregate type whose fields are the captured addresses -- which may be stored in an `auto` binding, passed to a template, returned, captured by another closure, and called wherever it is visible, and whose body has its own locals; a lambda body may raise, which makes its calls throwing calls that have to be handled. |
| 7 | `interface` | Declared exactly like `traits` -- the same parenthesised parameter list (`interface ICount(it T)`, the first parameter is the subject, later ones are type parameters), and the same body of restrictions. A restriction that states a result type (`it.next() -> int`) is a method-table entry: its name is the member called, its parameter types are the types the arguments name (`it.add(*int)`), and `??` marks a method that may raise (`it.save(int)?? -> void`); a restriction with no result type (`it.begin() -> !void`) is a check and not callable through the box. Boxing checks the same restrictions a trait query checks (one body, two uses), reports S0177 naming the restriction and the reason, and a value is a heap copy plus a method table, so a method that writes its receiver writes the copy; virtual calls are by name and by position (parameters have types, not names); `interface I;` is a forward declaration. An interface value may also become a narrower interface (methods matched by name, the table rebuilt from the source's entries), interfaces work inside template bodies, an interface method may return an aggregate passed out of line, and a throwing entry makes every virtual call of it a throwing call. A reference to an interface is an interface value for the purpose of a call -- it dispatches through the box behind it -- while a concrete value is *not* a reference to a box: `&value` where `&ICount` is wanted is refused, and the program boxes first (`b ICount = value; f(&b)`). |
| 8 | Template specialisation | `template <> type xxx<int> { ... }` and partial `template <T type> type xxx<*T> { ... }`, with the pattern written after the family's name; a specialisation replaces the type's fields; the most specific match wins, an ambiguous one is reported, and a specialisation travels with its exported family, so an importer builds the specialised member and uses it in methods written against the family's parameter; a method written for one concrete member is callable across a module too. |
| 9 | Forward declarations | `type Foo;`, `union U;`, `enum E;` (with its base), `traits T(it int);`, `interface I;`; an incomplete type may only be written behind a pointer or a reference, and a definition later in the file completes it. |
| 10 | Multi-line strings | `""".."""` may span lines, decodes escapes (raw strings are what `r` is for), and trims no indentation. |
| 11 | `@enable_if`, `static_assert`, `__setup__` | `@enable_if(cond)` removes a declaration from consideration instead of failing, per instantiation when it is written on a template and per member when it is written on a method of a type family; `static_assert` is checked at instantiation inside a template and immediately outside; one `__setup__` block per module runs before `main`, dependencies first and the main module last, cannot raise, and returns nothing. |
| 12 | Function parameter packs | The last parameter may be a pack: `values ...Ts` (a type pack, one type per argument), `values ...T` (one type), `values ...int` (a stated element type), and `values ...&Ts` for reference elements. The call chooses the arity and the element types, and the declaration is expanded per call into a fixed-parameter instance whose symbol carries both, so every arity is its own function and zero elements is a valid call; a pack is not a value -- the body may use it only as `sizeof...(values)`, as an argument expansion `f(values...)`, or in a comma fold `(statement, ...)`, which is a statement. |
| 13 | `thread_local` | A declaration modifier written after the type on a module-level binding or a static local: the object is one per thread, and importing modules see it through the same TLS storage. |
| 34 | 新增 `std.time`（Duration/Instant/SystemTime/DateTime + POSIX/Windows 后端） | 完成（time 测试） |
| 36 | `std.sync` 扩展：`RwLock`/`Semaphore`/`Once`/`Barrier` | 完成（sync_ext 测试） |
| 40 | 新增 `std.encoding`（hex/base64/percent）、`std.unicode`（UTF-8 编解码/校验/码点计数）、`std.strings`（join/repeat/replace/pad/case）、`std.grid`（行主序二维数组） | 完成（utilities 测试） |
| 59 | 新增 `std.math` 扩展（hypot/cbrt/trunc/fract/sign/lerp/degrees/radians/log2/log10/双曲函数/is_nan/is_infinite/is_finite/gcd/lcm/isqrt/is_even/is_odd/is_power_of_two；实数常量以函数给出） | 完成（numerics 测试） |
| 60 | 新增 `std.complex`（加减乘除、共轭、模、幅角、乘方、比较、`__str__`） | 完成（numerics 测试） |
| 61 | 新增 `std.matrix`（构造/零/单位阵、元素访问、加减乘、缩放、转置、比较；不提供求逆/分解并明确说明） | 完成（numerics 测试） |
| 62 | 新增 `std.test`（`$CHECK`/`$CHECK_EQ`/`$CHECK_EQ_REAL`/`$CHECK_EQ_TEXT`/`$FAIL` 宏 + 带位置的断言函数） | 完成（testing 测试） |
| 63 | 新增 `std.debug`（`$ASSERT_DEBUG`/`$DEBUG_UNREACHABLE`、`print_trace`、`breakpoint`；`DEBUG_ASSERT` 名称改为 `ASSERT_DEBUG` 以避开宏注册异常） | 完成（testing 测试） |
| 64 | 新增 `std.prelude`（常用类型与函数的重导出；函数仍需模块名限定，与语言导入规则一致） | 完成（testing 测试） |
| 65 | 平台后端补齐并做跨平台类型检查：`process_sys.windows`（CreateProcessA + POSIX 形状状态）、`net_sys.windows`、`dynlib_sys.windows`、`term_sys.windows`、`time_sys.windows` | 完成（`cross_target`：Windows 主机双检通过） |
| 54 | 新增 `std.crc`（CRC-8/16/32 标准变体 + 标准向量） | 完成（crypto 测试） |
| 55 | 新增 `std.crypto`（SHA-256、HMAC-SHA-256，标准向量验证） | 完成（crypto 测试） |
| 56 | 新增 `std.uuid`（v4 生成、文本格式化/解析、version/variant、比较） | 完成（crypto 测试） |
| 57 | 新增 `std.rand_dist`（分布函数名；`sample_` 前缀避开 UFCS 与 Random 方法冲突） | 完成（crypto 测试） |
| 58 | 修复 `std.fmt` 十六进制无符号值溢出（`{:08x}` 对 CRC-32 之类大于 `i32` 的值报“不是整数”） | 完成（fmt 行为 + crypto 测试） |
| 53 | 新增 `std.net`（IPv4 TCP connect/listen/accept/read/write，POSIX+winsock 后端，回环测试） | 完成（system 测试） |
| 47 | 新增 `std.process`（spawn/wait，POSIX fork+exec / Windows CreateProcessA） | 完成（system 测试） |
| 48 | 新增 `std.term`（ANSI 颜色/样式 + supports_color） | 完成（system 测试） |
| 49 | 新增 `std.dynlib`（dlopen/dlsym、LoadLibrary/GetProcAddress） | 完成（system 测试） |
| 50 | 新增 `std.signal`（Signal 枚举，install/restore/signal_self） | 完成（system 测试） |
| 51 | 新增 `std.compress`（adler32 + zlib stored 块 deflate/inflate） | 完成（system 测试） |
| 52 | 新增 `std.regex`（回溯匹配器，明确不支持的语法在 compile 时报错） | 完成（system 测试） |
| 44 | 新增 `std.csv`（RFC4180 风格解析/写回，`parse_rows`/`write_rows`） | 完成（serialization 测试） |
| 45 | 新增 `std.toml`（注释/表/键值子集，`TomlDocument` + `get`，不支持项明确报错） | 完成（serialization 测试） |
| 46 | 新增 `std.xml`（节点文档 + 句柄、元素/属性/文本/实体、序列化，CDATA/DTD 明确不支持） | 完成（serialization 测试） |
| 43 | 新增 `std.json`（索引式 JsonDocument + 可复制 JsonValue 句柄、解析/读取/序列化、`\u` 转义） | 完成（json 测试） |
| 42 | 新增 `std.iter`（区间位置工具与谓词 `Even`/`Odd`）与 `std.range`（`Range`，含不整除步长的终点语义） | 完成（iter_range 测试） |
| 41 | 修正：可抛函数返回聚合体（`Bytes` 等）时的移动构造未初始化成员导致崩溃；`(x as T).method()` 接收者无类型记录时按值临时处理 | 完成（utilities/encoding 测试覆盖） |
| 39 | 新增 `std.Bitset`、`std.Ring<T>`、`std.List<T>`（含 ListIterator 供 for 遍历） | 完成（collections_extra 测试） |
| 38 | 新增 `std.concurrent`：`Promise`/`Future`、`Channel`、`ThreadPool`（构造后 start，worker 用指针接收者） | 完成（concurrent 测试） |
| 37 | 新增 `std/allocators.ftm`（`Allocator`/`DefaultAllocator` 迁入 + `Arena`），`std.memory` 转发导出 | 完成（allocators 测试） |
| 35 | 新增 `std.log`（级别过滤、`set_target`、与 print 相同的文本规则） | 完成（log 测试） |
| 33 | `std.Any`（文本+类型名的动态值）、`Pair`/`Triple`、`Variant2` | 完成（any/tuple_variant 测试） |
| 32 | `std.BasicString<Char>` 通用字符串；`String16`/`String32` 别名（`String` 仍是优化过的 `char` 实现）；别名构造调用与值→引用构造实参 | 完成（basic_string 测试） |
| 31 | 新增 `std.Bytes`（`Array<u8>` 的包装）与 `String` 双向 `as` 转换 | 完成（bytes 测试） |
| 30 | 格式化规则独立成 `std/fmt.ftm`（`Formattable`、`format_value`、`render`/`rendered`、`apply_spec`），`std.io` 的 print 以它为基；规格 `[[fill]align][width][.precision][type]` | 完成（format/io 测试） |
| 29 | 语言加 `?T`（`Optional<T>` 语法糖）并与异常协同：present/value/value_or/reset/some/none，缺值抛 `NoneError` | 完成（optional 测试） |
| 28 | `std.algorithm` 改为按迭代器类型的单模板（`@enable_if` + iterator traits），删除按 Array/String/Deque 的穷举重载；Deque 迭代器补齐随机访问操作 | 完成（algorithm/deque/examples 测试） |
| 27 | `print`/`println` 恢复为可变参数包模板（删除 1..8 元穷举）；修复合成 span 跨实例化撞车导致的包展开崩溃 | 完成（io/parameter_packs 测试） |
| 26 | `Atomic<T>` 增加 `AtomicOrder`（5 种序）与带默认参数的 order 形参，默认 `SEQ_CST` | 完成（sync 测试） |
| 25 | `std.random` 公开面：删 `next_u32`、只留 `__call__`；补 exponential/lognormal/poisson/binomial/geometric/choice/weighted_choice | 完成（random 测试） |
| 23 | 内建宏统一为 `$__builtin_*` 前缀，`meta.ftm` 只做包装 | 完成（`meta_macros.rs`） |
| 24 | `String.__as__` 转换（int/usize/f64，失败抛 ValueError）；`try_init`/`parse_*` 不再导出 | 完成（`small_string.rs`/`io.rs`） |
| 20 | 模块限定调用与普通调用走同一套选择逻辑（导入即前置声明） | 完成 |
| 21 | `uniform` 单模板 + `if (IsFloating(T)) @constexpr`；方法模板返回类型按自身推导替换 | 完成 |
| 22 | `std.fs` 的 `mkdir`/`read_dir`/`size`/`is_file`/`is_directory`（POSIX/Darwin/Win32） | 完成（fs 测试 + `FALLC_TARGET_OS` 检查） |
| 17 | 转发引用（`&&T` 按值类别推导 + 引用折叠）与 `std.forward<T>` | 完成（`crates/fallc/tests/forwarding.rs`） |
| 18 | `MoveCanRaise(T)` 特征；`__ft_moves` 变成库内模板 | 完成 |
| 19 | `IOError` 携带 errno 与系统文本（`std.error` 三平台） | 完成（io 测试 `an_io_error_carries_the_system_error_number`） |
| 15 | 内建类型特征（`IsInteger`/`IsFloating`/`IsSame` 等）与 `@enable_if` 类型分派 | 完成（`crates/fallc/tests/type_traits.rs`） |
| 16 | `uniform` 按实参类型分派、`os.system`、`CondVar.wait` 谓词形式 | 完成（random/os/sync 测试） |
| 14 | Platform files | A module may have one implementation per platform, named `name.<platform>.ftm`; the loader prefers it for the target platform while the module name stays the same. It is how threads, the entropy source and the process calls are POSIX on Unix and Win32 on Windows without a preprocessor. `FALLC_TARGET_OS` overrides the platform for checking another system's files. |
| 67 | 声明关键字由属性表达：`alignas`/`constexpr`/`export`/`extern`/`thread_local`/`static` 是普通标识符，存储类、对齐、导出、常量性、ABI 与分支权重分别由函数属性（`@inline`/`@noreturn`/`@deprecated`/`@enable_if`/`@constexpr`/`export`/`@extern_C`）、变量属性（`@inline`/`@deprecated`/`@align(N)`/`@constexpr`/`export`/`@static`/`@thread_local`/`@uninitialized`/`@maybe_unused`）与分支属性（`@likely`/`@unlikely`、`if (cond) @constexpr`）表达；另有 `@must_use`/`@noinline`/`@cold`；`export` 覆盖全部可导出实体（含伞模块与宏）；`@constexpr` 变量要求常量初始化器，局部 `const` 允许运行期初始化 | 完成（`attribute_semantics` 测试、关键字测试、`import std` 与 C 互操作探针） |
| 68 | 协程：`@coroutine` 标记的函数返回 `std.Coroutine<Send, Yield, Return>`，`yield expr` 是表达式（交出 `Yield`、产出下一次 `resume` 送进的 `Send`），`resume` 把协程运行到下一次挂起并返回 `?Yield`（`Send` 为 `void` 时不带参数），`result()` 总是 `except`（未结束时抛 `NoneError`，因抛异常结束时重抛该异常）。无栈实现：入口函数建帧（`malloc` + 零初始化 + 标记"未启动"）、把参数复制进帧，机器是一个以帧为唯一参数的状态机，`yield` 存 payload、记下一状态号并返回，`resume` 每次从帧记录的状态继续；变量存放在帧里，因此跨挂起存活；`yield` 的值按 `Return` 相同的隐构/隐转规则检查。`resume`/`result` 由标准库声明、编译器按协程类型各生成一份实现 | 完成（`examples/`、`docs/language/05-coroutines.md` 的实测示例：`echo(12)` 依次得 0/3/7/none 与 12；`String`、结构体、`u64`→`f64` 载荷均已实测） |
| 69 | 析构修正：一个类型声明了 `__drop__` 时，它的字段由它自己释放，不再补一遍逐字段析构（此前 `?String`、`Optional<T>` 等会把同一个缓冲区释放两次）；`Optional<T>` 的移动构造与移动赋值改为真正转移（`util.move`），不再"深拷贝后把源标记为空" | 完成（`String` 协程与 `?String` 程序实测） |
| 70 | `Coroutine` 改为内建类型：库里只剩类型定义，`resume`/`result` 不再声明，由编译器在 `Coroutine` 接收者上按 `Send`/`Yield`/`Return` 直接回答并记录到 `coroutine_calls`。`Yield` 为 `void` 时 `resume` 返回 `bool`（`true ≡ ?Yield 的 Some(())`，`false ≡ none`），因为 `?void` 无法表示；`Send` 决定是否带实参、`Yield` 决定回答什么，两者正交；`Return` 可为 `void`，此时 `result()` 仍 `except` 但不带值 | 完成（12 个用例：`final`/`early2`/`str4`/`nt1`/`nt3`/`v2`/`vy3`/`vy4`/`rv2`/`rv3` 及 `std.go` 三个） |
| 71 | 协程体可 `raise`：异常存进帧的 exception 槽（帧新增第 5 个字段），协程体 escape 时把 tag 置为 `PAYLOAD_RAISED` 并 `ret void`，`result()` 读该槽重新抛出；`@coroutine` 函数的**调用**不再算抛异常调用（入口只分配帧、不执行函数体） | 完成（`rz4`：`yield 1` → `raise` → `resume` 得 none → `result()??` 抛 `Boom`） |
| 72 | 与协程相关的既有缺陷：类型特化整体失效（`local_families` 漏登记）；只有特化的家族无法构造成员（新增 `instantiate_from_specialisation`）；跨模块推导读不到家族实参（改读 nominal 的 `template_arguments`）；`@enable_if` 丢弃调用方替换与自由函数调用不过滤候选；`void` 被当成未解析的模板实参导致 `Coroutine<void,…>` 的成员从不实例化；`__drop__` 与逐字段析构重复释放；`Optional` 移动构造未真转移；混合类型载荷应按位存取而非数值转换 | 完成（examples 全过、文档块 74/78 与基线一致） |
| 73 | `std.go`：`go(task Coroutine<void,void,void>) void` 交给全局执行器，`settle() int` 跑到队列空并回答失败数；失败任务打印到 stderr 后继续；中断即不再调度，无取消 API；`go.ftm` 内不导出的 `TaskQueue` 作为模块级变量，`go`/`settle` 只是它的包装。通信由用户写出参 + `std.sync.Event`（参考 Python `asyncio.Event`：`set`/`clear`/`is_set`/`wait`，manual reset） | 完成（`go3` 交错调度、`go4` 失败汇报、`ev` 出参通信） |
| 76 | 执行器改为独立线程 + 有锁队列：`TaskQueue` 由 `Mutex` 保护，`CondVar` 唤醒；首次 `go` 启动调度线程，线程自行循环「取队首 → resume → true 则追加队尾 / false 则取 result」；`settle()` 只等待队列空且无在飞任务，它自身不做调度；失败打印到 stderr 后继续。队列操作全部持锁，`_busy` 记录在飞任务，使空队列仍能被 settle 正确等待 | 完成（`go3` 交错调度连续 5 次结果一致；`go4` 失败汇报；`ev`/`gz2`/`nm` 均通过） |
| 75 | 挂起点上限：规范只说"有上限、超出编译期报错"，上限由实现决定且**必须至少 1024**；本实现为 1024（`MAX_COROUTINE_STATES`）。恰好 1024 个挂起点可编译并运行，1025 个报 I0002 | 完成（1024 通过、1030 报错，实测） |
| 74 | 协程成员的链接归属：`std.go` 里有一个不被调用的 `placeholder` 协程，使库对象自带 `resume`/`result` 的定义；两处库编译循环（`compile_module_library` 与 `lower_with_modules` 的模块循环）此前先取走 `coroutine_frames` 再发射，顺序反了，导致定义从不生成；成员定义改为 `WeakAny`，因为符号只由 `Send`/`Yield`/`Return` 决定，且两者都只读帧的固定头部，程序的强定义覆盖库的弱定义。由此 `import std.go` 但从不 `go` 的程序链接正常，`settle()` 是 No-OP | 完成（`nm`：`idle failures=0`；`go3`/`go4`/`ev`/`gz2` 均正常） |
