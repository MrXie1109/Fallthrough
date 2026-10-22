# 04 Functions and Calls

> [中文](04-functions.zh.md)

## Scope

This volume specifies the declaration of functions and member functions, `lambda` and
closures, argument passing at a call, overload selection, default arguments, return type
deduction, move semantics, and evaluation order. Exceptions are part of the obligations
at a call site, see [06 Exceptions](06-exceptions.md).

## Function declarations

### Forms

```ft
fn add(a int, b int) int { return a + b; }
fn declared(a int) int;
export fn visible(a int) int { ... }
@extern_C fn printf(fmt *const c8, ...) int;
@constexpr fn square(n int) int { return n * n; }
```

2.1.1 A function with a body may omit its return type: omitting it is equivalent to
`auto`, and the type is deduced from the `return` statements. When no value is returned,
the return type is `void`, which may also be omitted.

2.1.2 A function without a body **must** write its return type (`void` included).

2.1.3 The type of a function is written `fn(Ps) R`; a function pointer type is written
`*fn(Ps) R`, see [02 Types](02-types.md#function-types).

2.1.4 `export` makes a function visible to importers; `@extern_C` specifies the C ABI
and has no function body; `constexpr` promises that the function may be used in a
constant position.

2.1.5 Functions in the same module may refer to one another regardless of the order in
which they are written.

2.1.6 Variadic parameters `...` are used only for C interoperability; extra arguments in
such a call are not type-checked.

2.1.7 A function may be declared as possibly raising with `except` (and a condition),
see [06 Exceptions](06-exceptions.md).

### Default arguments

2.2.1 A parameter may carry a default value:
`fn greet(name *const c8, punct *const c8 = "!") int`.

2.2.2 A parameter with a default value **must** follow the parameters without default
values.

2.2.3 A default value is an expression, evaluated at the call site, and only when the
argument is omitted. A default value may use the parameters before it.

2.2.4 The type of a default value **must** be compatible with the parameter type; the
check is performed at the declaration.

### Keyword arguments

2.3.1 A call may provide arguments by name, written `.name = value`; keyword arguments
**must** follow all arguments written positionally.

2.3.2 `.name` **must** be a parameter name. The receiver of a member function cannot be
provided by name.

2.3.3 A parameter **must not** receive a value twice: providing the same parameter both
positionally and by name is an error.

2.3.4 An omitted parameter is filled from its default value; a parameter without a
default value cannot be omitted.

2.3.5 In a cross-module call, the default-value record of the declaring module is
retained with the exported signature (see
[08 Modules](08-modules.md#defaults-cross-a-module-boundary)).

## Construction calls

3.1 `T(args)` is a construction call, selecting an `__init__` whose parameter list
matches.

3.2 `T{…}` and `.{…}` are field-wise initializers, specifying fields positionally or
with `.field = value`; an initializer **does not require** `__init__`.

3.3 Both forms are usable in a type context; `.{…}` requires the context to supply a
type.

3.4 When no constructor matches, it is a type error; the implementation **should** also
suggest the initializer spelling.

3.5 The explicit construction form `T(args)` uses `__init__` or `__im_init__`. When the
value of an initialiser, assignment, argument, return or field is not of the target
type, the conversion happens implicitly only if the target type declares a matching
`__im_init__` or the source type declares a matching `__im_as__`; the full rules are in
[07 Expressions](07-expressions.md#implicit-initialisation-and-implicit-conversions).

## Arguments and parameters

### Who takes a reference, who takes a pointer

4.1.1 `&` produces a pointer; a reference is a way of writing a type.

```ft
fn takes_ref(s &const String) int;    // reference: pass the name directly
fn takes_ptr(p *int) int;             // pointer: pass &x
```

4.1.2 In parameter position, a value of type `T` may satisfy a parameter of type `&T` or
`&const T`: the implementation passes the address of the argument. This adjustment holds
**only** in parameter position.

### Compatibility

4.2.1 The argument type **must** be compatible with the parameter type. Apart from the
implicit adjustments listed in Section 12 of [02 Types](02-types.md), no implicit
conversion may occur.

4.2.2 An integer literal takes the parameter's integer type.

4.2.3 When a parameter requires a declared type, a string literal may be constructed
through that type's `__init__`.

### Semantics of aggregate arguments passed by value

4.3.1 A nominal-type parameter passed by value is an **independent copy**: the parameter
is a copy of the caller's argument, and writing to the parameter does not affect the
argument.

4.3.2 The copy **must** be created according to the type's own copy semantics: when the
type declares `__init__(this &T, other &const T)`, the implementation **must** call it;
otherwise it copies member by member.

4.3.3 On function return, the parameter **must** be destroyed according to its type's
destruction semantics (`__drop__` or member destruction).

4.3.4 When a type declares neither a copy constructor nor a destructor, the
implementation copies member by member and produces no diagnostic; the two values may
then share a resource, and the program is responsible for that.

## Member functions

5.1 A function whose first parameter is named `this` is a member function.

5.2 `this` **must** be a reference: `this &T` (writable) or `this &const T` (read-only).
A pointer receiver does not conform to this standard (`S0190`):

```ft
fn length(this &const Point) int { return this.x + this.y; }
fn shift(this &Point, d int) void { this.x += d; }
```

5.3 When a member function is called, the receiver is passed as the first argument. When
the receiver is a value, the implementation takes its address; when the receiver is
already a reference or pointer value, the address it holds is passed.

5.4 Calling a member function through a pointer `p` is `p.m()`, equivalent to
`(*p).m()`.

5.5 `const` is shallow: a `const` object cannot call a member function whose receiver is
`this &T`, but may call one whose receiver is `this &const T`. A `mutable` field is the
exception.

## `lambda` and closures

6.1 `lambda` is an anonymous function expression: `lambda(ps) -> expr` or
`lambda(ps) { … }`.

6.2 A lambda that captures no names and does not raise is equivalent to an ordinary
function, and may be converted to a function pointer.

6.3 Every other lambda is a closure value: an anonymous nominal type whose fields hold
the addresses of the captured variables. A closure has no writable type name, and
**should** be stored with `auto`.

6.4 A closure captures by reference the outer local bindings and parameters used in its
body.

6.5 The parameter types of a lambda may be omitted; when omitted, they are deduced at
each call.

6.6 A lambda literal **must** be parenthesized when called:
`(lambda(x int) -> x)(1)`.

6.7 A lambda body may `raise`; its call site then follows
[06 Exceptions](06-exceptions.md). An exception fully caught in the body does not make
the lambda possibly raising.

6.8 A closure stores addresses and does not extend the lifetime of the captured object.
Returning a closure that captured a local variable yields a dangling reference, and
calling it is undefined behavior.

## Overloading and calls

7.1 One name may have several overloads. At a call, the receiver and the argument types
select a unique match.

7.2 When no unique match can be selected, the implementation **must** report a
diagnostic.

7.3 When a function name is used where a function pointer is required, the target
function type selects the overload.

7.4 Type names and function names do not share a namespace.

## Parameter packs

8.1 The **last** parameter of a function may be a pack: `values ...Ts` or `values ...T`.

8.2 Element type of a pack: with `...Ts` (a type pack), the element type is the type of
each argument; with `...T`, all elements have the same type; with `...int`, every
argument **must** be an `int`.

8.3 No parameter may follow a pack; a pack parameter must not have a default value; a
call that uses a pack accepts no keyword arguments.

8.4 Inside a function body, a pack may be used only in: `sizeof...(values)`,
`f(values...)`, `(statement, ...)`.

8.5 Each call produces a separate instance; the element types and the count take part in
the symbol.

8.6 For a member function, a pack consumes arguments after the receiver.

## Return type deduction

9.1 When the return type is written `auto`, it is deduced from the `return` statements.

9.2 Every `return` that carries a value **must** give the same type.

9.3 When all `return` statements carry no value, the return type is `void`.

9.4 The deduced result is fixed at compile time; the signature of an exported function
includes the deduced result, so the function may be called across modules.

## Move

10.1 Values are divided into named values and dying values (temporaries, call results).

10.2 A dying argument may bind to a `&&T` parameter and be moved; a named value **must
not** be moved automatically.

10.3 After a move the source is in a legal empty state: a null pointer and a length of
zero; it may still be read, assigned and destroyed.

10.4 The choice between move and copy is determined by the type: a type that declares
`__init__(this &T, other &&T)` has move construction, and a type that declares
`__assign__(this &T, other &&T)` has move assignment.

10.5 A type that declares no move operations is copied according to 4.3.1.

## Forwarding references and `std.forward`

11.1 `&&T` in a template parameter position is a forwarding reference: when the argument
is an lvalue, `T` is deduced as `&U`; when the argument is an rvalue, it is deduced as
`U`.

11.2 `&&T` is handled by the reference-collapsing rules: only `&& &&` remains an rvalue
reference.

11.3 `std.forward<T>(value)` yields an lvalue when `T` is a reference, and an rvalue
reference otherwise. Its behavior is specified by the standard library.

## Evaluation order

12.1 Function arguments are evaluated from left to right.

12.2 The comma operator is evaluated from left to right; `&&`, `||` and `?:` are
short-circuit evaluated.

12.3 An argument is **evaluated only once** before it is bound to the parameter,
including the case in which its address must be taken.

## Recursion and forward references

13.1 At module level, all names in the same scope are declared first, and function
bodies are checked afterwards; functions may therefore call one another regardless of
the order in which they are written.

13.2 A type may mention a later-declared type through a pointer or a reference (see
[03 Declarations](03-declarations.md#forward-declarations)).
