# Appendix B Special Methods

> [中文](appendix-b-special-methods.zh.md)

## B.1 Scope

Every special method the language calls on a fixed construct, as a table. The normative
text is [07 Expressions §8](07-expressions.md#special-methods); this appendix is a
reference and creates no rule of its own.

## B.2 How to read the tables

The columns are the same throughout:

| Column | Meaning |
| ------ | ------- |
| **Form** | the methods, i.e. the names a type may declare |
| **Meaning** | what the language is asking the type to do |
| **Prototype** | the recommended declaration: receiver, parameters and return type |
| **Call site** | the construct that reaches it |
| **Notes** | when it is chosen, what happens without it, what it must not do |

### Conventions

B.2.1 `this` **must** be the first parameter of a special method and **must** be a
reference: `this &T` to write, `this &const T` to read only. A pointer receiver does not
conform ([04 Functions §5](04-functions.md#member-functions)).

B.2.2 A prototype is a **recommendation**. The receiver's `const`, the parameter count
and the return type are what the calling rule actually reads; where the return type
matters it is said in the notes.

B.2.3 `__init__` is explicit construction only; `__im_init__` also serves implicit
initialisation. `__as__` is explicit conversion only; `__im_as__` also serves implicit
conversion. Declaring an `im` form declares the plain one as well
([07 Expressions §8.6](07-expressions.md#implicit-initialisation-and-implicit-conversions)).

B.2.4 For a type to have a special method in another module, the method **must** be
`export`ed together with the type ([08 Modules](08-modules.md#the-export-table)).

B.2.5 A method that may raise is declared `except`, and the call-site duty of
[06 Exceptions](06-exceptions.md) then applies wherever the language calls it implicitly.

## B.3 Construction, assignment and destruction

| Form | Meaning | Prototype | Call site | Notes |
| ---- | ------- | --------- | --------- | ----- |
| `__init__(this &T)` | default construction | `fn __init__(this &T) void` | `T x;` | Supplied by default: built-in members to zero, declared members by their own default constructor, nothing under `@uninitialized` (B.4.1) |
| `__init__(this &T, other &const T)` | copy construction | `fn __init__(this &T, other &const T) void` | `T b = a;`, `f(a)` by value | Supplied by default: member by member |
| `__init__(this &T, other &&T)` | move construction | `fn __init__(this &T, other &&T) void` | `T b = move(a);` | Supplied by default: member by member, and the source is **not** cleared (B.4.2) |
| `__init__(this &T, args…)` | explicit construction | `fn __init__(this &T, n int) void` | `T(a, b)`, `T x(a, b);` | Not supplied; must be declared |
| `__im_init__(this &T, args…)` | implicit initialisation | `fn __im_init__(this &T, text *const c8) void` | `T x = value;`, argument, return value, field | Also declares `__init__`; the `im` form is what makes the conversion implicit |
| `__assign__(this &T, other &const T)` | copy assignment | `fn __assign__(this &T, other &const T) &T` | `a = b;` | Supplied by default: member by member |
| `__assign__(this &T, other &&T)` | move assignment | `fn __assign__(this &T, other &&T) &T` | `a = move(b);` | Supplied by default: member by member, source **not** cleared (B.4.2) |
| `__drop__(this &T)` | destruction | `fn __drop__(this &T) void` | end of scope, overwrite, `erase`/`pop` | Supplied by default: the members' own destructors run. Declare it to free what the type owns |
| `__getitem__(this &T, i usize)` | subscript | `fn __getitem__(this &T, i usize) &T` | `t[i]` | Returning a reference makes `t[i] = v` writable; that is how a type gets a writable subscript |

## B.4 Default forms

B.4.1 A type that declares no constructor still has the default `__init__(this &T)`:
each member is initialised in turn — a built-in member to zero, a member of a declared type
by *its* default constructor, so nesting works to any depth. A value declared with
`@uninitialized` is not initialised at all. `T{…}` and `.{…}` are **not** constructor
calls: the fields are written directly and no `__init__` runs.

B.4.2 The default copy and move forms copy member by member. The move forms **do not
clear the source** — an `int` member keeps its value, a pointer member keeps pointing
where it pointed. Clearing a moved-from value is the source type's own business, and only
a declared move form can do it. Reading a moved-from value is defined (it is whatever the
move left behind) and the implementation **should** warn `S0148` (`use-after-move`).

B.4.3 Declaring one of the five forms replaces the default of that signature only; the
other four stay as described. A default form whose members cannot be default-constructed,
copied, moved or assigned is ill-formed when it is used, and is not reported for a type
that never uses it.

B.4.4 The rvalue reference `&&T` is produced by the standard library's `move`. It is
declared in `std.util`, which the umbrella `std` does not re-export: a file that uses it
**must** write `from std.util import move;`.

## B.5 Conversion and text

| Form | Meaning | Prototype | Call site | Notes |
| ---- | ------- | --------- | --------- | ----- |
| `__as__(this &const T, sel *U)` | explicit conversion | `fn __as__(this &const T, sel *double) double` | `x as U` | The second parameter is a **selector**: it names the target type and is never read |
| `__im_as__(this &const T, sel *U)` | implicit conversion | `fn __im_as__(this &const T, sel *double) double` | argument, initialiser, return value | Also declares `__as__`; this is what makes the conversion implicit |
| `__str__(this &const T)` | the text a reader sees | `fn __str__(this &const T) String` | `str(x)` | Default: `repr(x)` |
| `__repr__(this &const T)` | the text a program writes | `fn __repr__(this &const T) String` | `repr(x)` | Default: `<Type object at 0x…>` |
| `__format__(this &const T)` | library formatting | `fn __format__(this &const T) String` | `std.format("{}", x)` | Default: `str(x)`; `{!s}` and `{!r}` choose at the call site |
| `__hash__(this &const T)` | hash | `fn __hash__(this &const T) usize` | hash containers | Not supplied: a type used as a key must declare it |

B.5.1 A conversion cannot be chosen by its result type, so the target is named by the
selector: `*double` selects `double`. `const` is ignored when comparing.

## B.6 Operators and iteration

| Form | Meaning | Prototype | Call site | Notes |
| ---- | ------- | --------- | --------- | ----- |
| `__add__` `__sub__` `__mul__` `__div__` `__mod__` `__pow__` | arithmetic | `fn __add__(this &const T, other &const T) T` | `+ - * / % **` | `/` and `%` truncate |
| `__and__` `__or__` `__xor__` `__shl__` `__shr__` | bitwise | `fn __and__(this &const T, other &const T) T` | `& \| ^ << >>` | |
| `__neg__` `__pos__` `__not__` | unary | `fn __neg__(this &const T) T` | `-a`, `+a`, `!a` | `__not__` is logical not; bitwise `~` is a separate form |
| `__eq__` `__ne__` `__lt__` `__le__` `__gt__` `__ge__` | comparison | `fn __eq__(this &const T, other &const T) bool` | `== != < <= > >=` | The result type is the method's; a `bool` when used as a condition |
| `__cmp__(this &const T, other &const T)` | three-way | `fn __cmp__(this &const T, other &const T) int` | `<=>` | Negative, zero or positive |
| `__iadd__` … `__ishr__` | compound assignment | `fn __iadd__(this &T, other &const T) &T` | `a += b` | Without it, `a = a + b`. The expression's value is what was stored |
| `__radd__` … `__rshr__` | swapped operands | `fn __radd__(this &const T, other &const T) T` | `a + b` when `a`'s type has no `__add__` | Tried only when the left operand has no such method |
| `__inc__` `__dec__` | increment, decrement | `fn __inc__(this &T) &T` | `++a`, `a++`, `--a`, `a--` | Prefix yields the new value, postfix the old; the operand **must** be a place |
| `__deref__(this &const T)` | dereference | `fn __deref__(this &const T) &U` | `*a` | Must return a reference |
| `__pointer__(this &const T)` | member through a pointer | `fn __pointer__(this &const T) &U` | `a->field` | Returns the object to visit next; chainable |
| `__call__(this &T, args…)` | call as a function | `fn __call__(this &T, n int) int` | `a(args)` | |
| `__bool__(this &const T)` | truth | `fn __bool__(this &const T) bool` | `if (a)`, `!a`, `a && b` | **Should** produce a `bool` |
| `__iter__` `__next__` | iteration | see [std.iter](../standard/) | `for (x T : a)` | |

## B.7 Operator index

The tables above are indexed by method, which answers "what does this name do". This one
is indexed by **what is written**, which answers "which method does the compiler look for".

| Written | Method | Written | Method |
| ------- | ------ | ------- | ------ |
| `a + b` | `__add__` / `__radd__` | `a += b` | `__iadd__`, else `__add__` |
| `a - b` | `__sub__` / `__rsub__` | `a -= b` | `__isub__`, else `__sub__` |
| `a * b` | `__mul__` / `__rmul__` | `a *= b` | `__imul__`, else `__mul__` |
| `a / b` | `__div__` / `__rdiv__` | `a /= b` | `__idiv__`, else `__div__` |
| `a % b` | `__mod__` / `__rmod__` | `a %= b` | `__imod__`, else `__mod__` |
| `a ** b` | `__pow__` / `__rpow__` | `a **= b` | `__ipow__`, else `__pow__` |
| `a & b` | `__and__` / `__rand__` | `a &= b` | `__iand__`, else `__and__` |
| `a \| b` | `__or__` / `__ror__` | `a \|= b` | `__ior__`, else `__or__` |
| `a ^ b` | `__xor__` / `__rxor__` | `a ^= b` | `__ixor__`, else `__xor__` |
| `a << b` | `__shl__` / `__rshl__` | `a <<= b` | `__ishl__`, else `__shl__` |
| `a >> b` | `__shr__` / `__rshr__` | `a >>= b` | `__ishr__`, else `__shr__` |
| `-a` | `__neg__` | `+a` | `__pos__` |
| `!a` | `__not__` | `~a` | bitwise not |
| `++a`, `a++` | `__inc__` | `--a`, `a--` | `__dec__` |
| `a == b` | `__eq__` | `a != b` | `__ne__` |
| `a < b` | `__lt__` | `a <= b` | `__le__` |
| `a > b` | `__gt__` | `a >= b` | `__ge__` |
| `a <=> b` | `__cmp__` | | |
| `*a` | `__deref__` | `a->field` | `__pointer__` |
| `a[i]` | `__getitem__` | `a(args)` | `__call__` |
| `str(a)` | `__str__`, else `__repr__` | `repr(a)` | `__repr__` |
| `a as T` | `__as__` / `__im_as__` | `if (a)` | `__bool__` |
| `T x;` | `__init__(this &T)` | `a = b` | `__assign__` |
| `T(args)` | `__init__(this &T, args…)` | end of scope | `__drop__` |

B.7.1 The `r` form is tried only when the left operand's type has no plain form: `a + b`
with `a int` and `b T` reaches `T.__radd__(b, a)`, and its `this` is `b`, the right
operand.

B.7.2 The `i` form is tried first for a compound assignment. Without it the language uses
`a = a + b`, which needs `__add__` and `__assign__`.

## B.8 Constraints

B.8.1 A special method **must** be a member function: its first parameter is `this`.

B.8.2 `this` **must** be a reference — `&T` to write through, `&const T` not to.

B.8.3 `__as__` and `__im_as__` **must** have the selector as their second parameter.

B.8.4 The operand of `__inc__`, `__dec__` and the assignment forms **must** be a place.

B.8.5 Two identical nominal types are not compared member by member: a type that wants
comparison **must** declare it ([07 Expressions §8.4](07-expressions.md#operators-and-comparison)).

B.8.6 Special methods take part in overload resolution like any member function, so one
type may declare several `__init__`, `__assign__` or `__as__` for different sources.

B.8.7 The `i` and `r` forms are not separate entries in the tables above; they are the
base method under the compound-assignment and swapped-operand rules.
