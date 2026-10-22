# Appendix B Special Methods

> [中文](appendix-b-special-methods.zh.md)

## B.1 Scope

This appendix gathers every special method the language calls on a fixed construct:
its name, when it is called, the signature convention and the constraints. The
normative text is [07 Expressions §8](07-expressions.md#special-methods); this
appendix is a reference table and creates no rule of its own.

## B.2 General rules

B.2.1 A special method is a member function spelled `__name__` whose **first parameter
must be named `this`** and whose type **must be a reference**: `this &T` or
`this &const T` ([04 Functions §5](04-functions.md#member-functions)). A pointer
receiver does not conform to this standard.

B.2.2 Whether a special method can raise (`except`) is part of its type. Where the
language calls one implicitly, the call-site duty of [06 Exceptions](06-exceptions.md)
applies.

B.2.3 For use across modules the method **must** be `export`ed with the type; an
exported type without the method has no such method for importers
([08 Modules](08-modules.md#the-export-table)).

B.2.4 `__init__` is explicit construction only; `__im_init__` also allows implicit
initialisation. `__as__` is explicit conversion only; `__im_as__` also allows implicit
conversion. Declaring `__im_init__` also declares an `__init__`, and declaring
`__im_as__` also declares an `__as__`
([07 Expressions §8.6](07-expressions.md#implicit-initialisation-and-implicit-conversions)).

B.2.5 A method with an `i` form (`__iadd__` etc.) serves compound assignment; one with
an `r` form (`__radd__` etc.) serves a binary operation whose left operand has no such
method. These are not separate table entries; they are derived from the base method by
the call rules (see B.6).

## B.3 Construction, assignment and destruction

| Special method | Called when | Signature convention |
| -------------- | ----------- | -------------------- |
| `__init__` | explicit construction `T(args)` | receiver `this &T`, then the arguments |
| `__im_init__` | construction, and implicit initialisation in a target-type position: initialiser, assignment, argument, return value, field | as `__init__` |
| `__assign__` | assignment to an existing value: `t = v;` | receiver `this &T`, then the source value |
| `__drop__` | a value's scope ends or it is overwritten/removed | receiver `this &T`, no other parameter |
| `__getitem__` | `t[i]` | receiver `this &T` or `this &const T`; returning a reference makes `t[i] = v` writable |
| `__setitem__` | `t[i] = v` when the type declares it | receiver `this &T`, an index and a value |

B.3.1 A type that declares no constructor is built with a field initialiser (`T{…}`,
`.{…}`), which does not call one.

B.3.2 A type without `__drop__` is destroyed through its fields' destructors; a type
that owns a resource **should** declare it.

B.3.3 Move construction is an `__init__`/`__im_init__` whose argument is an rvalue
reference (`&&T`); move assignment is an `__assign__` with an rvalue-reference argument.

## B.4 Conversion and text

| Special method | Called when | Signature convention |
| -------------- | ----------- | -------------------- |
| `__as__` | explicit conversion `expr as T` | receiver `this &const T` (the source); second parameter is a **selector** pointing at the target type, used only for selection |
| `__im_as__` | conversion, and implicit conversion in a target-type position | as `__as__` |
| `__str__` | `str(x)` | receiver `this &const T`, returns a string |
| `__repr__` | `repr(x)` | as above |
| `__format__` | library formatting (`std.fmt`) | receiver `this &const T`, returns a string |

B.4.1 A conversion cannot be selected by its result type, so the target is named by the
selector parameter: `*double` selects `double` (`const` is ignored when comparing).

B.4.2 `str`/`repr` are built-in functions: when a type declares the method it is
called; otherwise the implementation renders the value by its category
([07 Expressions §8.3](07-expressions.md#str-and-repr)).

## B.5 Operators

| Special method | Operator | Notes |
| -------------- | -------- | ----- |
| `__add__` `__sub__` `__mul__` `__div__` `__mod__` `__pow__` | `+ - * / % **` | `/` and `%` truncate |
| `__and__` `__or__` `__xor__` `__shl__` `__shr__` | `& \| ^ << >>` | bitwise |
| `__neg__` `__pos__` `__not__` | unary `- + !` | `__not__` is logical `!`; bitwise `~` has its own form |
| `__iadd__` `__isub__` … | `+= -= …` | compound assignment; without it, `a = a op b` |
| `__eq__` `__ne__` `__lt__` `__le__` `__gt__` `__ge__` | `== != < <= > >=` | result type comes from the method; a `bool` in a condition |
| `__cmp__` | `<=>` | negative/zero/positive |
| `__inc__` `__dec__` | `++a` / `--a` and postfix forms | the operand **must** be a place |
| `__deref__` | `*a` | returns a reference |
| `__pointer__` | `a->field` | returns the object to visit next; chainable |
| `__call__` | `a(args)` | |
| `__bool__` | `a` in a condition | should produce a `bool` |

B.5.1 When the operands differ in type, or the type is nominal, the operation goes to
the left operand's special method; when it has none, the right operand's `r` form is
tried.

B.5.2 Two identical nominal types are not compared automatically or field by field: a
type that wants comparison **must** declare the method.

## B.6 Derived forms

| Form | Meaning | When considered |
| ---- | ------- | --------------- |
| `__iadd__` etc. | in-place form of compound assignment | `+=` before `a = a + b`; declaring it means "modify this" |
| `__radd__` etc. | swapped binary form | when the left operand's type has no `__add__` |
| postfix `++`/`--` | the value before the modification | `__inc__`/`__dec__` serve both; the language keeps the copy |

B.6.1 The result of a compound assignment is always the target's type; the value an `i`
form returns is discarded, because the operation is a modification of the target.

## B.7 Constraints

B.7.1 A special method **must** be a member function (first parameter `this`).

B.7.2 `this` **must** be a reference: `this &const T` for a read-only receiver,
`this &T` for a writable one.

B.7.3 `__as__`/`__im_as__` **must** have a second parameter, the target-type selector.

B.7.4 The operand of `__inc__`/`__dec__` and of the assignment forms **must** be a
place.

B.7.5 Special methods take part in overload resolution like ordinary member functions;
one type may declare several `__assign__`/`__as__`/`__init__` for different sources,
and the arguments select one.

B.7.6 A special method may raise: an `except` method called implicitly creates the same
call-site duty as an explicit call ([06 Exceptions §7](06-exceptions.md#implicit-call-sites)).
