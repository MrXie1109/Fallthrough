# 06 Exceptions

> [中文](06-exceptions.zh.md)

## Scope

This volume specifies the exception model: the declaration that a function may raise,
`raise`, the clauses of `try`, the obligations at a call site, the propagation of
exceptions and the order of destruction, and the strategy of the standard library
shipped with the implementation for insufficient resources.

## Model

2.1 An exception is **part of the return value** specified by this standard, not a
separate control-flow mechanism.

2.2 The return value of a function that may raise is a tagged union:

- for a function returning `T`: `{ ok, value T, exception }`;
- for a function returning no value: `{ ok, exception }`.

2.3 An exception consists of two string components: a **type name** and a **message**.
The types of both are specified by the standard library.

2.4 The implementation **must** guarantee: a call site that may raise adds only one tag
test; raising requires no unwind tables and does not change the ABI of functions that do
not raise.

2.5 When an exception leaves a scope, the bindings already constructed in that scope
**must** be destroyed in the order specified in [09 Scopes](09-scopes.md).

## Declaring that a function may raise

3.1 `except` is written after the return type.

```ft
fn risky(n int) int except {
    if (n < 0) {
        raise "ValueError": "negative";
    }
    return n * 2;
}
```

3.2 A condition may follow `except`: a constant expression stating that the function may
raise only when the condition holds.

```ft
fn check(n int) int except(n < 0) { ... }
```

3.3 The condition is written in terms of the parameters. At the call site, the condition
is folded into a constant using the arguments: when all arguments are constants, the
implementation **must** decide on the spot; when the arguments are not constants, the
call is treated as possibly raising.

3.4 Any operation in a function body that may raise (a direct `raise`, a call that may
raise, an implicit call that may raise) requires the function to declare `except`;
otherwise the implementation **must** report a diagnostic.

## Raising

4.1 `raise "Type": "message";` raises an exception; `raise "Type";` omits the message;
`raise;` re-raises the exception currently being handled.

4.2 The type name and the message **must** be convertible to the standard library's
string type; a string literal performs that conversion automatically.

4.3 The message may be text constructed from run-time values.

4.4 `raise` may appear only in a function that may raise.

4.5 A bare `raise;` may appear only inside an `except` block that is handling an
exception; its type and message remain unchanged.

## Catching

### Forms

```ft
try {
    ...
} except {
    ...
} else {
    ...
} finally {
    ...
}
```

| Clause | Semantics |
| ---- | ---- |
| `try` | the protected statement block |
| `except` | one or more `except` clauses, tried in written order, the first match runs; a bare `except` catches everything and a parenthesised list catches only the names listed (5.6) |
| `else` | executed when the `try` block raised nothing |
| `finally` | executed whether or not an exception is raised |

5.2 An `except` block obtains the current exception through the standard library's
exception accessor. The type of that value and the way it is accessed are specified by
the standard library.

5.3 A `try` without an `except` is merely a scope.

5.4 A `try` with only a `finally` does not catch exceptions; the exception propagates
outward, and the `finally` is executed first.

5.5 The execution order of `else` and `finally` is: `try` succeeds → `else` → `finally`;
on a raise, → `except` → `finally`.

### Named handlers

5.6 A `try` may be followed by **any number** of `except` clauses, tried top to bottom;
the block of the first matching clause runs:

```ft
try {
    ...
} except ("KeyError") {
    ...
} except ("IOError", "FileError") {
    ...
} except {
    ...                         // everything else
} finally {
    ...
}
```

A clause is written in one of two ways:

- `except ("ZeroError", "KeyError") { .. }`: catches only the exceptions named. The
  names are string literals and are compared against the exception's **type name**
  (`name()`).
- `except { .. }`: a bare clause, catching every exception.

5.7 An exception that no clause matches is not caught by this `try`: it keeps
propagating outward, this `try`'s `finally` runs first, and `else` does not run. A
clause written after a bare `except` never runs.

5.8 When no clause is bare the capture is incomplete, so it only makes sense in a
function that may raise; writing such a `try` in a function that cannot raise is a
diagnostic the implementation **must** report (a bare `except` clause is needed for a
complete capture). Inside the blocks the handler still counts as protection, and an
unmatched exception leaves the function as 5.7 says.

## Evaluation-site obligations

6.1 Evaluating an **expression** that may raise **must** satisfy one of the following
(not only function calls: operators, subscripts, construction, explicit and implicit
conversions and assignment are covered too):

1. it is inside a `try` block with an `except`;
2. `?!` is written after the expression that may raise, handing the exception to the
   caller of this function.

6.2 `?!` is a postfix expression: `x?!` is equivalent to
`try { x } except { raise; }`.

6.3 `?!` affects only the current call site; an enclosing `try` in the same function may
still catch the exception.

6.4 When an expression that may raise is left unhandled, the implementation **must**
report a diagnostic.

6.5 `?!` on an expression that cannot raise is a warning (`S0083`), not an error: within
a template instantiation, the same mark may be used for both raising and non-raising
expressions.

## Implicit call sites

7.1 The following constructs call member functions and are therefore also call sites:
construction, assignment, operators, subscripts, type conversion, `__bool__` in a
condition, `__call__`, `str()`, `repr()`.

7.2 `?!` on an initializer protects the entire initialization, including a construction
call that the implementation inserts for the initialization.

7.3 `?!` on an assignment belongs only to the right-hand expression; when the assignment
operator itself may raise, the assignment is an unhandled call site.

7.4 The call site of any other implicit call is the position of that expression; when
the entire statement must be protected, a `try` region **must** be used.

## Uncaught exceptions

8.1 An exception that a program does not catch **must** be reported by the implementation
to standard error, and the program must terminate with exit code 70 after the report.

8.2 The format of the report **should** contain the exception type name and the message.

## Insufficient resources (strategy of the standard library shipped with the implementation)

9.1 The behavior of the standard library's containers and strings on allocation failure
is specified by the standard library; this standard does not require their member
functions to be marked `except`.

9.2 The implementation **must** guarantee that a failure to construct the exception
record itself does not produce undefined behavior; the specific replacement strategy is
specified by the standard library.

9.3 Recoverable errors raised by a program itself are the intended use of exceptions
specified by this standard; the strategy for insufficient resources is not part of the
language semantics.
