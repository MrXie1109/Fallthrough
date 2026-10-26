# 07 Expressions

> [中文](07-expressions.zh.md)

## Scope

This volume specifies the forms and evaluation of expressions: names, fields,
subscripts, calls, conversions, operators, assignment, and the special methods that
these constructs invoke.

## Basic forms

| Form | Semantics |
| ---- | ---- |
| `name` | the name of a binding, constant or module member |
| `a.b` | field access, or module-member access (determined by name resolution) |
| `p->b` | equivalent to `(*p).b`; the left side **must** be a pointer, or reach a pointer through `__pointer__` |
| `a[i]` | subscript; the semantics are specified by the type or by built-in rules |
| `f(a, b)` | call |
| `obj.m(a)` | member function call, with the receiver as the first argument |
| `p(1)` | call through a function pointer, with no dereference written |
| `f<int>(a)` | call with explicit template arguments |
| `T(args)` | construction call |
| `f"…{e}…"` | interpolated string; a call to `std.format`, see below |

### Interpolated strings

2.1.1 An interpolated string `f"…"` denotes `std.format(fmt, e1, e2, …)`, where `fmt` is
the literal's text with each placeholder replaced by `{}` and `e1…` are the placeholder
expressions in the order written. The type of the whole is the type `format` returns,
which is the standard string.

```ft
n int = 42;
s String = f"n is {n}";          // std.format("n is {}", n)
h String = f"{n:#x}";            // std.format("{:#x}", n)
```

2.1.2 A placeholder is `{expr}` or `{expr:spec}`. The `spec` is the ordinary format
specification ([06 Exceptions](06-exceptions.md) is not involved): `{:>8}`, `{:08}`,
`{:.2f}`, `{:#x}` and the conversions `{!s}`/`{!r}` behave exactly as they do in a
written format string, because that is what they become.

2.1.3 `{{` and `}}` are one literal brace, so `f"{{x}}"` is `{x}` and not a placeholder.

2.1.4 The call may raise, so an interpolated string is a call site like any other: it
needs a `try`, a `??` or a `!!` ([06 Exceptions](06-exceptions.md#evaluation-site-obligations)).
The literal carries no mark of its own — where the failure goes is the program's choice,
written at the use.

2.1.5 Each placeholder is a full expression, evaluated left to right, once each. Its
result is rendered by the value's `__format__`, or by `str` when it has none
([§8](#special-methods)).

2.1.6 `std.format` comes from the standard library, so a file that writes an interpolated
string **must** import it (`import std;` or `import std.fmt;`). Without the import the
name `format` resolves to nothing, and the program is rejected as it would be for any
other unknown name.

## Conversions

### `as`

3.1.1 A type conversion **must** be written explicitly: `expr as T`.

3.1.2 Legal conversions:

| From | To |
| -- | -- |
| any numeric type | any numeric type |
| `byte` | any numeric type, and the reverse |
| a pointer or reference | a pointer or reference of the same shape |
| a pointer to `byte` | a pointer to any type, and the reverse |
| `bool` | any integer type (`true` is 1, `false` is 0) |

3.1.3 A lossy numeric conversion wraps to the target width; this is the semantics of the
conversion, not an error.

3.1.4 Adding `const` requires no `as`; removing `const` **must** be written `as`.

3.1.5 A nominal type may declare `__as__` to define a conversion, see Section 8.

## `sizeof` and `alignof`

4.1 The results of `sizeof(T)` and `alignof(T)` are `usize` constants, and the operand
**must** be a type.

4.2 `sizeof...(pack)` is the number of elements in a parameter pack, not a number of
bytes.

4.3 `sizeof`/`alignof` may appear in a position that requires a compile-time constant.

## Operators

### Conditional operator and comma

5.1.1 `cond ? a : b` evaluates only the selected branch.

5.1.2 `a, b` is evaluated from left to right, and the result is `b`.

### Unary operators and increment/decrement

| Form | Semantics |
| ---- | ---- |
| `-x`, `+x`, `!x`, `~x` | negation, unary plus, logical not, bitwise complement |
| `&x` | address-of, result `*T`; the operand **must** be a place |
| `*p` | dereference, result `&T`; the operand **must** be a pointer |
| `++x`, `x++` | increment; the postfix result is a copy of the value before increment |
| `--x`, `x--` | decrement |

5.2.1 The operand of `++`/`--` **must** be a place. For a nominal type, the operation
lands on `__inc__`/`__dec__`.

5.2.2 A function name is the exception to address-of: both `&f` and a bare `f` produce a
function pointer.

### Assignment

5.3.1 `a = b` is right associative; the target **must** be a place.

5.3.2 Compound assignments include `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`,
`<<=`, `>>=`, `**=`.

5.3.3 For a nominal type, `=` calls `__assign__`; a compound assignment first calls a
method such as `__iadd__`, and when none exists it is equivalent to `a = a op b`.

5.3.4 When the target type is a reference, assignment writes through the reference into
the referenced storage.

## Evaluation order and undefined behavior

6.1 Arguments are evaluated from left to right; the comma operator is evaluated from
left to right; `&&`, `||` and `?:` short-circuit.

6.2 The same argument is evaluated only once.

6.3 Within one full expression, when no order is specified between two modifications of
one value, the behavior is undefined behavior.

6.4 Out-of-bounds pointer access, dereferencing a null pointer, reading an inactive
member of a union, and integer overflow (in `--release` mode) are undefined behavior.

## Compile-time queries

7.1 The following expressions are evaluated at compile time and may appear in a position
that requires a constant:

```ft
sizeof(T)
alignof(T)
sizeof...(pack)
except(expr)
TraitName(TypeArgs…)
```

7.2 `except(expr)` only analyses whether the expression may raise; it does not evaluate
it.

7.3 The rules for trait queries and built-in type traits are given in
[03 Declarations](03-declarations.md#traits).

## Special methods

### Call table

The language calls member functions of fixed names at fixed constructs. The following
table lists all extension points.

| Special method | When it is called |
| -------- | -------- |
| `__init__` | a value is **explicitly** constructed: `T(args)` |
| `__im_init__` | a value is constructed, and the construction may also happen implicitly in a target-type position |
| `__assign__` | assignment to an existing value |
| `__drop__` | the value's scope ends, or the value is overwritten |
| `__getitem__` | `t[i]` |
| `__setitem__` | explicit subscript assignment |
| `__add__`, `__sub__`, `__mul__`, `__div__`, `__mod__`, `__pow__` | the corresponding binary operator |
| `__and__`, `__or__`, `__xor__`, `__shl__`, `__shr__` | the bitwise operators |
| `__neg__`, `__pos__`, `__not__` | the corresponding unary operator |
| `__iadd__`, etc. | compound assignment; falls back to `a = a op b` when not declared |
| `__eq__`, `__ne__`, `__lt__`, `__le__`, `__gt__`, `__ge__` | the comparison operators |
| `__cmp__` | `a <=> b` |
| `__inc__`, `__dec__` | `++a`/`--a` and the postfix forms |
| `__deref__` | `*a`: returns a reference |
| `__pointer__` | `a->field` |
| `__call__` | `a(args)` |
| `__bool__` | `a` in a condition |
| `__as__` | explicit conversion `x as Target` |
| `__im_as__` | a conversion that may also happen implicitly in a target-type position |
| `__str__`, `__repr__` | `str(x)`, `repr(x)` |
| `__format__` | standard library formatting |

### The three built-ins

8.2.1 Basic types have built-in `__init__`, `__assign__` and `__drop__`: `__init__` is a
single store, `__assign__` is a single store, and `__drop__` does nothing.

8.2.2 A type that owns heap resources **should** declare these three methods; when they
are not declared, the implementation copies member by member, and the program is
responsible for the resource semantics.

### `str` and `repr`

8.3.1 `str(x)` and `repr(x)` are built-in functions whose result is the standard
library's string type.

8.3.2 When a type declares `__str__`/`__repr__`, the built-in function **must** call it.

8.3.3 When the operand is a reference, it is rendered as the referenced value.

8.3.4 When no method is declared, rendering follows the default rules specified by the
standard library; a type that cannot be rendered **must** report a diagnostic.

### Operators and comparison

8.4.1 When the two operand types differ or the type is nominal, the operation lands on a
special method of the left operand; when none exists, the reflected form on the right
operand (`__radd__`, etc.) is tried.

8.4.2 Scalar comparison is built in; numeric values of different widths are compared by
mathematical value.

8.4.3 Two identical nominal types **must not** be compared automatically or member by
member; when comparison is needed, methods such as `__eq__` **must** be declared.

8.4.4 The return type of a comparison method is determined by the method; when used as a
condition it **should** be `bool`.

### `__as__`

8.5.1 The second parameter of `__as__` is a **selector**: a pointer to the target type,
used to select the target type and never used otherwise.

8.5.2 A conversion cannot be distinguished by return type for overloading, so the target
type **must** be written in the selector.

8.5.3 `const` is ignored when comparing selectors.

### Implicit initialisation and implicit conversions

8.6.1 `__init__` is used only by the **explicit** construction form `T(args)`. When the
source value is not of the target type, declaring `__init__` alone does not make an
initialisation, assignment, argument, return value or field initialisation happen
implicitly.

8.6.2 `__im_init__` has the same form as `__init__`, but it additionally makes that
construction available for **implicit initialisation** in a target-type position. The
explicit form `T(args)` may select `__im_init__` as well.

8.6.3 `__as__` is used only by the explicit conversion `expr as T`. `__im_as__` has the
same form (declared on the source type, with a selector parameter pointing at the
target type) and additionally allows the source value to be converted implicitly in a
target-type position. The explicit form `expr as T` may select `__im_as__` as well.

8.6.4 An implicit conversion in a target-type position is chosen in this order, and
each step must yield a unique candidate:

1. the built-in implicit adjustments and integer promotion listed in Section 12;
2. an `__im_init__` of the target type;
3. an `__im_as__` of the source type.

When there is no candidate or the candidate is not unique, the implementation **must**
report a diagnostic.

8.6.5 When an implicit construction or conversion can raise, the call-site duty is the
same as for an explicit call; see [06 Exceptions](06-exceptions.md).

### Visibility

8.7.1 For use across modules, special methods **must** be `export`ed as well.

8.7.2 A missing `__drop__` produces no diagnostic; the consumer treats the type as
having no destructor.

## Special-method reference

Every special method, its call site, signature convention and constraints is in
[Appendix B](appendix-b-special-methods.md).

## Folding and pack subscript

9.1 `(values + ...)` is a fold, combining the elements of the parameter pack in order.

9.2 `values[i]` takes an element from the pack, and `i` **must** be a constant.

9.3 The complete rules for folding, pack counting and pack subscripts are given in
[`../templates.md`](../templates.md).
