# 05 Coroutines

> [中文](05-coroutines.zh.md)

## Scope

This volume specifies coroutines: functions that may suspend themselves, hand a value
to whoever resumes them, and continue when they are resumed. It specifies the attribute
that declares one, the result type it returns, the `yield` expression, the two members a
caller uses to run it, and how a coroutine body becomes a state machine.

## Declaring a coroutine

### Form

```ft
import std;

@coroutine
fn echo(n int) std.Coroutine<int, int, int> {
    i int = 0;
    while (i < n) {
        i += yield i;
    }
    return i;
}
```

2.1 A function marked `@coroutine` **must** have the return type
`std.Coroutine<Send, Yield, Return>`. Any other return type is an error.

2.2 The three type arguments are the coroutine's contract, and each answers a
different question. None of them implies anything about the other two.

| Type | What it is | Where it appears |
| ---- | ---------- | ---------------- |
| `Send` | what a `resume` hands in, and what a `yield` produces as its value | the argument of `resume`; the value of a `yield` expression |
| `Yield` | what a `yield` hands out, and what a `resume` answers | the operand of `yield`; the optional `resume` returns |
| `Return` | what the body's `return` gives, and what `result` answers | the operand of `return`; the value `result` returns |

2.3 A body's `return` gives its `Return` type, not the coroutine object: `echo`
above returns an `int`, and the object is what *calling* `echo` produces.

2.4 Any of the three may be `void`, and each `void` means "nothing travels" in that
one position:

| Position | `void` means |
| -------- | ------------ |
| `Send` | the coroutine takes nothing in; `resume` is called with no argument |
| `Yield` | a `yield` hands nothing over; `resume` answers nothing |
| `Return` | the body returns nothing; `result` answers nothing |

2.5 A coroutine takes no other attributes that change its shape, and may not be
`@extern_C`: a coroutine object is a language value, not a C ABI type.

## The `yield` expression

3.1 `yield expr` hands `expr` to the resumer and produces the `Send` the *next*
`resume` brings. It is an expression, so it may stand anywhere an expression may, and
its value is used like any other:

```ft
i += yield i;
```

3.2 The operand is checked against `Yield` exactly as a `return` value is checked
against `Return`: the type's own constructor answers when the expression is not already
that type, and otherwise it is converted to it. So `yield 300` in a coroutine whose
`Yield` is a type with an `__init__` calls that constructor, the same way `return 300`
in a function of that type would.

3.3 A bare `yield;` hands nothing over and is allowed only where `Yield` is `void`. It
is still a suspension: the coroutine stops there and a later `resume` continues it.

3.4 `yield` outside a `@coroutine` body is an error.

3.5 A suspension point may be reached many times. A `yield` inside a loop suspends on
each pass, and each `resume` continues after the `yield` that stopped it — not after
the first one.

3.6 A coroutine has a bounded number of suspension points, and exceeding the bound is an
error at compile time. The bound is the implementation's to choose, and it **must** be at
least 1024; this implementation's bound is 1024.

## Running a coroutine

4.1 Calling a `@coroutine` function creates the coroutine and returns it. **The body
does not run at that point.** The call allocates the frame and stores the arguments in
it, and that is all.

4.2 What a `resume` takes and what it answers are two separate questions. `Send`
decides the first and `Yield` decides the second; a coroutine may take nothing in and
still hand a value out, or take a value in and hand nothing out.

4.3 `resume` runs the coroutine up to its next `yield`. It answers `?Yield` — that
`yield`'s value, or `none` when the coroutine has finished — and it takes the `Send`,
written as the argument. The four combinations are:

| `Send` | `Yield` | The call | It answers |
| ------ | ------- | -------- | ---------- |
| a type | a type | `c.resume(v)` | `?Yield` |
| a type | `void` | `c.resume(v)` | nothing |
| `void` | a type | `c.resume()` | `?Yield` |
| `void` | `void` | `c.resume()` | nothing |

An argument given to a coroutine that takes nothing in, or omitted from one that takes a
value, is an error: the call does not shape up.

4.4 `resume` never raises. A coroutine that has finished answers `none`, and one that
has not started answers `none` too — there is no way for `resume` itself to fail.

4.5 `result()` returns the body's `Return` value. It is always `except`: asking a
coroutine that has not finished raises `NoneError`, and a coroutine that ended by
raising re-raises that exception. A coroutine whose `Return` is `void` answers nothing;
`result()` is still `except` and still has to be dealt with.

```ft
c auto = echo(12);
y ?int = c.resume(3);   // 0
y = c.resume(4);        // 3
y = c.resume(5);        // 7
y = c.resume(1);        // none
r int = c.result()??;   // 12
```

4.6 The sequence above is normative. A `resume` answers the value the coroutine
suspended with, and the value it hands in is used by the **next** `resume`. That is
what makes `i += yield i` mean what it reads as: the first `yield i` hands out `0`
because `i` is still `0`, and the `3` given to that same call is added when the body
continues.

4.7 `resume` and `result` belong to the compiler. They are the only two members a
coroutine has, and neither is declared in a module: the frame a coroutine runs in and
the states of its machine are known only to the compiler that built them, so a library
body taking `&Coroutine<..>` could not do anything with one. Their shape is read off
`Send`, `Yield` and `Return` as 4.3 and 4.5 state.

4.8 A coroutine object is a value. It may be stored in a binding, passed by reference,
returned from a function and held in a record; it is not copyable, because two copies
would run one machine.

## The state machine

5.1 A coroutine has no stack of its own. It is compiled as one function whose entry
point is a number stored in a frame, and `resume` calls that function again with the
frame. Concretely:

- the **entry** keeps the name the declaration gave it and does three things: it
  allocates a frame, stores the arguments in it, and returns a coroutine object holding
  the frame and the address of the machine;
- the **machine** is a private function taking the frame and nothing else. It reads the
  state number out of the frame and jumps to the continuation that number names; state
  `0` is the body's beginning, and every `yield` gets its own continuation;
- a **`yield`** stores what it hands over in the frame, records which continuation
  comes next, and returns to the resumer;
- **`resume`** calls the machine and then reads what the frame holds.

5.2 The frame is one heap allocation. It holds the `Send` handed in, the payload (what a
`yield` handed out or the body's `return` gave), a tag saying which of the two the
payload is, the state number, and a slot for every binding the body declares.

5.3 The frame holds the coroutine's own variables, so they outlive one activation of
the body. A binding declared in a `@coroutine` body therefore lives in the frame rather
than in the call's stack: `i` in `echo` is live across every `yield`, and a stack slot
would be gone the moment the body returned to its resumer.

5.4 The payload slot is as wide as the larger of `Yield` and `Return`, so a smaller one
is stored widened and read back narrowed. When the two are of different kinds — an
integer yielded and a floating-point value returned — the bytes are stored as the bits
they are and read back as the type the reader wants, not converted in between.

5.5 A frame is freed by the coroutine object's destructor. A coroutine that is never
destroyed leaks its frame, which is the same rule every other owning value follows.

## The executor

6.1 `std.go` runs coroutines whose `Send`, `Yield` and `Return` are all `void`.

```ft
import std;
from std.go import go, settle;

go(worker(1, 3));
go(worker(2, 3));
settle();
```

6.2 `go(task)` appends `task` to a global queue and returns without resuming it. The
first call starts a scheduler thread.

6.3 The scheduler thread runs its own loop: it waits until a task is queued, removes that
task, and resumes it. When `resume` answers `true` the task is appended to the back of
the queue; when it answers `false` the task has finished and is removed for good.

6.4 An exception that ends a task is written to standard error and the task is discarded.

6.5 The scheduler runs one task at a time. Tasks interleave at their `yield`s; they do
not run concurrently, and they do not run on the calling thread.

6.6 `settle()` waits until no task is queued and none is in flight, then returns the
number of tasks that failed since the program started. It schedules nothing itself.

6.7 A task is resumed no more once `settle()` has returned and the program ends. There is
no cancellation and no handle: a task that ends early returns.

## Communication

7.1 `go` carries no value. A task reports through parameters written for that purpose,
which are references:

```ft
fn fetch(result &String, ready &Event) std.Coroutine<void, void, void> {
    result = http_get(url);
    ready.set();
}

result String;
ready Event;
go(fetch(result, ready));
ready.wait();
use(result);
```

7.2 `std.sync.Event` has `set`, `clear`, `is_set` and `wait`. `set` makes `is_set` true
and wakes every waiter; `clear` makes it false; `wait` returns when `is_set` is true. A
waiter's return does not clear the event.

## Restrictions

8.1 A `@coroutine` body may call functions, raise and use `try`. An exception raised in
the body is carried in the frame and re-raised by `result()`.

8.2 An array type may not be a coroutine's `Send`, `Yield` or `Return`. A declaration
that names one is reported.
