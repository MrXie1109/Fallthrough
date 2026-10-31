# 03 Declarations

> [中文](03-declarations.zh.md)

## Scope

This volume specifies declaration forms at module level and block level: variables and
constants, initialization, declaration modifiers, attributes, macros, `traits`,
`interface`, templates and type families, and the positions that require a compile-time
constant. Function declarations are given in [04 Functions](04-functions.md).

## Declaration prefix

2.1 A declaration's prefix **must** be written in this order; any other order does not
conform to this standard:

```text
@attributes …              declaration attributes, one per line (see section 9)
template <…>               template header
fn / type / union / enum / interface / traits / NAME …
```

2.2 Storage class, alignment and constant-ness are not a prefix: they are
attributes, written before a function declaration or after a variable, constant or
field declaration (see section 9). `mutable` is a field modifier, written after the
field's type.

2.3 A declaration and the visibility order of names in a scope are independent (see
section 8 on forward declarations and [09 Scopes](10-scopes.md)).

## Bindings

3.1 A binding consists of a name, a type and an optional initializer.

3.2 A binding with no initializer is zero-initialized bitwise. Consequently a legal
initial state exists for every type.

3.3 A binding qualified by `const` **must** have an initializer.

3.4 `auto` infers the type from the initializer:

```ft
import C;

fn main() int {
    v int = 5;
    p *int = &v;
    n auto = 7;
    r auto = *p;        // a value: auto infers the value
    C.printf("%d %d\n", n, r);
    return 0;
}
```

This program prints `7 5`: `n` is an `int`, and `r` is the `int` the pointer names rather
than the pointer itself.

3.4.1 `auto` infers the type of the **value**: when the initializer is a reference, the
result is the type of the referenced value.

3.4.2 When the initializer is a literal that takes its type from context, `auto` takes
that context type.

3.4.3 `type(auto)` infers the type of the **expression itself**, consistently with
`type(expr)`; a place remains a reference.

3.4.4 `auto` may be used only for bindings with an initializer and for function return
types. Parameter types **must** be written.

3.4.5 `auto` in a modifier matches by level of indirection: the `*` in `x *auto = e;`
consumes one level of pointer or reference on `e`, and `auto` takes the remaining type;
`x &auto = e;` requires `e` to be a place.

## Declaration modifiers

4.1 Storage class, alignment and constant-ness at declaration level are
attributes; export is the keyword `export`. Their positions and meanings are in
section 9:

```ft
import C;

count int @static = 0;          // static storage duration
seen int @thread_local = 0;     // one object per thread
buf [16]byte @align(64);        // raise the alignment
export fn visible() void { }    // exported function

fn main() int {
    count = 3;
    buf[0] = 1;
    C.printf("%d %d %d\n", count, seen, buf[0] as int);
    return 0;
}
```

This program prints `3 0 1`: `count` keeps the value assigned to it, `seen` starts at zero,
and `buf` is writable at its first byte.

4.2 Type qualifiers `*`, `&`, `&&`, `const`, `volatile` are written before the type;
`mutable` is written after a field's type and says the field may be written inside a
`const` member function.

4.3 The rules of `@align` are in [02 Types](02-types.md#layout).

## Module-level variables

5.1 A module-level binding declares an object unique within the program; it is
initialized once, and its lifetime extends to the end of the program.

5.2 The initializer of a module-level variable **must** be a compile-time constant.
Without an initializer the value is zero.

5.3 A module-level variable may be assigned.

5.4 A module-level variable not marked `export` is visible only in the module that
defines it; the rules for `export` are given in
[08 Modules](09-modules.md#the-export-table).

### External variables and functions

5.5 A module-level variable marked `@extern_C` declares an object provided by an external
library; an analogous function declaration is marked the same way. There is no `extern`
keyword: the attribute says the name comes from elsewhere, and it is written after the
type in the same place as every other declaration attribute.

```ft
import C;

@extern_C
fn getenv(name *const c8) *c8;

fn main() int {
    path *c8 = getenv("PATH");
    C.printf("%d\n", (path != nullptr) as int);
    return 0;
}
```

This program prints `1` where `PATH` is set in the environment: `getenv` is looked up in
the C library by its own name, with the C calling convention, because `@extern_C` says so.

5.6 An external variable **must not** have an initializer; reading or writing the name
accesses the storage provided by the library.

5.7 `@extern_C` specifies both the calling convention and the source of the symbol: the
name is passed to the linker unmangled, and no definition is emitted. A function declared
without a body is external for the same reason.

5.8 `export` makes an external variable or function visible to importers.

## Module-level constants

6.1 A module-level constant is a module-level binding whose type contains `const`:
`LIMIT const usize = 1024;`.

6.2 A module-level constant **must** be initialised with a compile-time constant: its
value is part of the program image and it may be used where the language requires a
compile-time constant (see section 14).

6.3 A local `const` binding only says the value cannot be written after the
declaration; its initialiser need not be constant, and the value is computed when
control reaches the declaration.

6.4 `@constexpr` on a binding is the stronger promise: it **must** have an initialiser,
and that initialiser **must** be a compile-time constant; the binding is also `const`
(see section 9).

6.5 Constants and variables may both be exported: `export` goes before the
declaration (`export LIMIT const usize = 8;`, `export counter int = 0;`).

## `__setup__`

7.1 A module has **at most** one `__setup__` function, declared
`fn __setup__() void`.

```ft
import C;

counter int = 0;

fn __setup__() void {
    counter = 41;
}

fn main() int {
    C.printf("%d\n", counter + 1);
    return 0;
}
```

This program prints `42`: `main` reads `counter` after the setup function has run, so the
assignment in `__setup__` is what `main` sees.

7.2 It runs once, before `main`, in import-dependency order: the `__setup__` of an
imported module runs before that of the importer.

7.3 It is a function in every respect, so its body is written as any other `void`
function's: it declares bindings, calls functions, and may end early with `return;`. It
**must not** return a value or `raise`, and takes no parameters.

7.4 `__setup__` is an ordinary identifier. It names the module's setup function only when
the declaration is exactly `fn __setup__()`; any other signature is an ordinary
function that nothing calls.

## Forward declarations

8.1 A nominal type may be declared first and defined later:

```ft
import C;

type Node;
union Value;
enum Color u32;
traits Walkable(it int);

type Node {
    value int;
    next *Node;
}

fn main() int {
    n Node = .{7, nullptr};
    C.printf("%d %d\n", n.value, (n.next == nullptr) as int);
    return 0;
}
```

This program prints `7 1`: `Node` is named before its fields are written, and by the time a
value is made the type is complete.

8.2 Before the definition appears, the type is incomplete: it may be used only in
pointers, references and declarations; defining a value, `sizeof`, `alignof`, member
access and passing by value **must** require a complete type.

8.3 A declaration and a definition are two appearances of the same name; two definitions
do not conform to this standard.

8.4 A forward declaration of an `enum` **must** write the base type (`int` when
omitted); the members appear with the definition.

8.5 Only a definition is exported across modules. A type seen by an importer **must** be
complete.

8.6 An `interface` is forward-declared in the same way as `traits`.

## Attributes

### Form and position

9.1 An attribute is written `@name` or `@name(args)`. A declaration may carry several
attributes; their order is irrelevant.

9.2 Attributes fall into four categories, each with its own position:

| Category | Position |
| -------- | -------- |
| functions | before the declaration, including before `template <…>` |
| variables, constants, fields | after the name and type; before `=` when there is an initialiser |
| branches | after the condition and before the block: `if (cond) @likely {`, `else @unlikely {`, `case 1 @likely:` |
| types, type families, traits, interfaces, enums, aliases | before the declaration |

9.3 An attribute in a position that does not allow it, an unknown name, a duplicate
attribute, or a wrong number or kind of arguments is an error; an implementation
**must not** silently ignore it.

### Function attributes

| Attribute | Meaning |
| --------- | ------- |
| `@inline` | ask for inlining |
| `@noinline` | ask against inlining |
| `@noreturn` | the function does not return normally; its return type **must** be `void` |
| `@must_use` | the call's result **must** be used |
| `@cold` | the function is rarely called; code generation may place it on a slow path |
| `@deprecated`, `@deprecated("message")` | warn at each use |
| `@enable_if(cond)` | the declaration does not exist when the condition is false; the condition **must** be a compile-time constant |
| `@constexpr` | the function may be called in a constant position (section 14) |
| `@extern_C` | use the C ABI; the symbol is not mangled |

### Variable and constant attributes

| Attribute | Meaning |
| --------- | ------- |
| `@inline` | allow the same variable to be defined in several translation units |
| `@deprecated`, `@deprecated("message")` | warn at each use |
| `@align(N)` | alignment requirement, see [02 Types](02-types.md#layout) |
| `@constexpr` | needs a compile-time-constant initialiser; the binding is also `const` |
| `@static` | static storage duration (local) |
| `@thread_local` | one object per thread |
| `@uninitialized` | no zero fill and no empty constructor; **no** initialiser allowed; locals only |
| `@maybe_unused` | do not report the never-read warning |

### Types, branches and modules

9.4 Types, type families, traits, interfaces, enums and type aliases are exported by
`export` and may carry `@deprecated`; a type declaration may also carry `@must_use`, which says
that the result of a function returning that type **must** be used.

9.5 Branch attributes:

| Attribute | Position | Meaning |
| --------- | -------- | ------- |
| `@likely` | `if`, `else`, `case` | this branch is usually taken |
| `@unlikely` | `if`, `else`, `case` | this branch is rarely taken |
| `@constexpr` | after the condition of an `if` only | the condition **must** be a compile-time constant and the branch not taken is not checked |

9.6 Exporting a module or a macro uses the keyword too: `export from std.array import
*;` re-exports the umbrella module's names; `export define NAME …` makes a macro
visible to importers.

9.7 `export` may mark every exportable entity: functions, variables,
constants, types, type families, traits, interfaces, enums, umbrella re-exports and
macro definitions.

## Macros

10.1 A macro is token substitution, defined at module level:

```ft
import C;

define MAX(a, b) ((a) > (b) ? (a) : (b))
define SHOW(x) #x
export define VERSION 3

fn main() int {
    C.printf("%d\n", $MAX(3, 7));
    C.printf("%s\n", $SHOW(hello));
    C.printf("%d\n", $VERSION);
    return 0;
}
```

This program prints `7`, then `hello`, then `3`: `MAX` is the conditional expression it
was defined as, `#x` turns the argument into the text `hello`, and `VERSION` is the
number it was defined as. Every use carries the `$` that section 10.4 requires.

10.2 The definition of `define NAME body` extends to the end of the logical line; a `\`
at the end of the line continues the definition onto the next line.

10.3 `export define` makes a macro visible to importers.

10.4 A macro invocation **must** carry `$`: `$NAME` or `$NAME(args)`. A name without `$`
is not a macro invocation.

10.5 Expansion is recursive and subject to an implementation-defined depth limit. On
exceeding the limit, the implementation **must** report a diagnostic.

10.6 `#param` converts an argument into a string literal; `a ## b` pastes two adjacent
tokens into one.

10.7 `...` declares variadic parameters; the variadic arguments are accessed through
`__VA_LIST__`.

10.8 `define` is not a keyword: it is recognised only at the start of a logical line
(`export define` likewise).

10.9 A macro name may be module-qualified: after `import meta;`, `$meta.FILE` expands to
the `FILE` exported by `meta`. A dot belongs to the name only when the whole dotted
string is a macro name.

10.10 The implementation provides built-in macros: `$__builtin_FILE`,
`$__builtin_LINE`, `$__builtin_COLUMN`, `$__builtin_PLATFORM`, `$__builtin_ARCH`,
`$__builtin_OS_WINDOWS`, `$__builtin_OS_POSIX`, `$__builtin_DATE`,
`$__builtin_TIME`. They expand to literals and do not depend on any module.

## Traits

### Definition

11.1.1 `traits Name(params) { requirements }` declares a named set of requirements.

11.1.2 The parameters are value parameters (name first, type after). A name that appears
in a parameter type is a **type variable** of the trait; a value parameter is a place in
the requirements.

11.1.3 A requirement is an expression; requirements are separated by commas. Each
requirement may carry a result constraint:

- `-> !void`: the result exists and is not `void`;
- `-> T`: the result type is exactly `T`;
- `-> value`: the compile-time value of the result is exactly that value.

11.1.4 A method call in a requirement may carry type arguments (`it.push(int)`). The
argument type **must** be a type.

11.1.5 `??` written after a call in a requirement means that the operation **may
raise**.

11.1.6 There is no inheritance clause between traits; a hierarchy is expressed by
writing the preceding requirement as a requirement, for example
`InputIterator(type(it)) -> true`.

11.1.7 `export` makes a trait queryable by importers.

### Queries

11.2.1 Writing a trait as a function yields a compile-time `bool`:
`InputIterator(IntIter)`.

11.2.2 The arguments of a query **must** be types. The type of a value is written
`type(expr)`.

11.2.3 A trait query folds to a constant, but its type is an ordinary `bool`. It may
therefore appear in a constant position, and may also be stored in a `bool` variable.

11.2.4 Querying an undeclared trait, or supplying the wrong number of arguments, is an
error.

### When requirements are checked

11.3.1 Requirements are checked only when the trait is queried; they are not checked at
the declaration.

11.3.2 A trait that is never queried incurs no run-time or code-generation cost; a
requirement written incorrectly in it is not reported at the declaration.

### Built-in type traits

11.4.0 A type trait answers a question about **the type itself** -- whether it belongs
to a category, whether two types are equivalent, whether moving a value may raise --
and not "which members does it have, which operations can it perform". The latter is
what a program declares with `traits` (the standard library's `InputIterator`,
`Allocator` and so on).

11.4.1 The category predicates are defined by the standard library with **type families
and template specialisation**, not by the compiler. `std.type_traits` declares one type
family per predicate: the generic member answers `false`, and an explicit
specialisation for the types that satisfy the predicate answers `true`.

```ft
import C;

template <T type>
type IsInteger {
    VALUE const bool = false;
}

template <>
type IsInteger<int> {
    VALUE const bool = true;
}

fn main() int {
    C.printf("%d %d\n", IsInteger(int) as int, IsInteger(f64) as int);
    return 0;
}
```

This program prints `1 0`: `IsInteger(int)` selects the specialisation, whose `VALUE` is
`true`, and `IsInteger(f64)` falls back to the general one, whose `VALUE` is `false`.

11.4.2 The query `IsInteger(int)` instantiates the family; the specialisation machinery
(a full or a partial specialisation, the most specific one winning) chooses the member
that answers, and the constant member `VALUE` folds to the answer, so the query is
still a compile-time constant. A program may declare its own predicate in the same
shape: a family whose body declares a `VALUE const bool` constant turns that name into
a query.

11.4.3 The following queries are defined by the standard library's
`std.type_traits`, available with `import std` or `import std.type_traits`:

| Query | Meaning |
| ---- | ---- |
| `IsInteger(T)` | is an integer type |
| `IsFloating(T)` | is a floating-point type |
| `IsNumeric(T)` | is an integer or a floating-point type |
| `IsPointer(T)`, `IsReference(T)` | is a pointer / reference |
| `IsVoid(T)`, `IsBool(T)`, `IsCharacter(T)` | respectively |

11.4.4 Two predicates are written with a partial specialisation, also in
`std.type_traits`:

| Query | Meaning | How |
| ---- | ---- | ---- |
| `IsArray(T)` | is a fixed-size array type | the partial specialisation `IsArray<[]T>` matches an array of any length |
| `IsSame(A, B)` | the two types are equivalent | the partial specialisation `IsSame<A, A>` binds both positions to one type |

11.4.5 Whether moving a `T` may raise is a question about the `except` marker of the
move constructor `T` would select: `std.util.move_can_raise<T>()` returns `bool`. It
cannot be used directly as the condition of `if (..) @constexpr`, because a function
call in the condition is not folded -- what folds is the `except(..)` query in the
function's body. `std.util` therefore provides a wrapper macro:

```ft
import C;
import std;
import std.util;

template <T type>
fn move_path() int {
    if ($MOVES(T)) @constexpr {
        C.printf("move may raise, so the element is copied\n");
    } else {
        C.printf("move cannot raise, so the element is moved\n");
    }
    return 0;
}

fn main() int except {
    move_path<int>();
    move_path<String>();
    return 0;
}
```

This program prints `move cannot raise, so the element is moved` twice: neither `int` nor
`String` has a move that can raise, so both take the moving path.

`$MOVES(T)` expands to the query itself, `except(T(util.fake<&&T>()))`, so the
condition is foldable where it is written. It needs `import std.util;`.

11.4.6 The result of every type trait is a compile-time constant, usable in
`if (cond) @constexpr` and in `@enable_if`. Every other type requirement in the
standard library (iterators, allocators, formattable types and so on) is a library
`traits` declaration, not a type trait.

## Interface

### Definition and semantics

12.1.1 `interface Name(it T, …) { requirements }` declares a run-time boxed type.

12.1.2 The declaration is written in the same way as `traits`; the difference lies in
use: a `traits` name is used only for queries, while an `interface` name is a type.

12.1.3 When a value of a concrete type is assigned to an interface type, the
implementation **must** check that the value of that type satisfies every requirement of
the interface; when it does, the implementation creates a copy on the heap and generates
a method table.

12.1.4 A method called through an interface **must** be called virtually through its
method-table entry.

12.1.5 An interface value may be converted to an interface with fewer methods and the
same signatures.

### Method table

12.2.1 A requirement that can state its result type is an entry of the method table.

12.2.2 The method name is the member name called in the requirement; the parameter types
are the types explicitly written in the requirement; the result type is given by `-> T`;
`??` means that the method may raise.

12.2.3 A requirement that cannot state its result type is only a check, not a
method-table entry; calling it through an interface is not a construct permitted by this
standard.

12.2.4 The order of the method table is the order in which the requirements are written.

## Templates

### Parameters

13.1.1 A template is introduced by `template <params>`.

| Spelling | Meaning |
| ---- | ---- |
| `T type` | type parameter |
| `N int` | value parameter; the argument **must** be a constant |
| `Args ...type`, `values ...int` | parameter pack; must be the last parameter, and accepts zero or more arguments |

13.1.2 Each distinct combination of arguments produces a separate instantiation. A
different value argument is a different instance.

13.1.3 Template arguments **must** be constants, and may be written explicitly or
deduced at the call site.

13.1.4 A template body interprets names in the module that defines it. When an importer
declares a type with the same name, causing an instantiation ambiguity, the
implementation **must** report a diagnostic and must not merge them silently.

### Default arguments

13.2.1 A parameter may carry a default value: `= type` for a type parameter, `=
expression` for a value parameter.

13.2.2 A parameter with a default value **must** follow the parameters without default
values; a parameter pack is not subject to this rule.

13.2.3 A default value may use the parameters before it.

### Specialisation

13.3.1 A member of a type family may be fully specialised (`template <>`) or partially
specialised (the pattern contains parameters).

13.3.2 On matching, the most specific pattern wins. When two patterns contain neither the
other, the result is ambiguous, and the implementation **must** report a diagnostic.

13.3.3 A specialisation replaces only the fields; the methods of the type family are
substituted with the member's arguments.

13.3.4 A specialisation is visible across modules together with the exported type
family.

### A family's methods

13.4.1 A method of a type family is a template function whose receiver writes the family
name and its arguments (`this &Vec<T>`).

13.4.2 The `template` header of a method **must not** be omitted.

13.4.3 When a method declared for a concrete member has the same name as a family
method, the method of the concrete member takes precedence.

13.4.4 The family and the method must each be `export`ed for an importer to use them.

## Compile-time constant positions

14.1 The value in the following positions **must** be known at compile time:

- `N` in an array length `[N]T`;
- template arguments;
- the condition of `if (cond) @constexpr`;
- the initializer of a `const` binding;
- the value of an `enum` member;
- a `case` label;
- the condition of `static_assert`.

14.2 A call standing in one of these positions **must** call a `@constexpr fn`, and its
arguments **must** be constants.

14.3 `@constexpr` is a promise on the declaration; the particular call that stands in a
constant position **must** be evaluable, and every value read during evaluation **must**
be a constant; otherwise the implementation **must** report a diagnostic.

14.4 Constant evaluation of a template function is expression-level: folding, pack
counts, literals and operations on them may be evaluated directly; a template function
body containing statements is not a constant expression.

14.5 A function not marked `constexpr` may be folded by the implementation, but that is
an optimization and not a promise of the language; a program must not rely on its
happening or not happening.
