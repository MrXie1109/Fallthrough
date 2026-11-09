# `std.reply`

A value one task produces and another waits for.

`go` carries no value back, so a task that has something to report is given a
place to put it and a way to say it has. `Reply` is both, in one object:

    @coroutine
    fn fetch(url String, reply &Reply<String>) std.Coroutine<void, void, void> {
        reply.set(content_of(url));
    }

    inbox Reply<String>;
    go(fetch(url, inbox));
    String page = inbox.wait()??;

# One value, then closed

A reply carries one value. `set` stores it and wakes everyone waiting; a
second `set` is a fault in the program and aborts, because the second value
would silently replace the first and the task that wrote it would think it had
been read. `wait` answers that value, or awaits it if it is not there yet.

# Waiting is for threads, not for coroutines

`wait` blocks the calling *thread* on a condition variable, so it belongs to
the side that is not running the scheduler: `main` waiting for a task to
report. A coroutine that blocks on `wait` stops the very thread that would run
the task which is supposed to set it, and the program deadlocks. A coroutine
that must wait for a value polls `ready()` and yields:

    yield from inbox.await();
    String page = inbox.get();

## Declarations

### `type Reply {`

A value that arrives once, and the waiters it wakes.

```
export
template <T type>
type Reply {
    // Guards `_value` and `_closed`.
    _mutex Mutex;
    // Wakes the waiters of `set`.
    _arrived CondVar;
    // The value, absent until `set` is called.
    _value Optional<T>;
    // Whether `set` has been called.
    _closed bool;
}
```

### `fn __init__(this &Reply<T>) void`

Creates an empty reply.

```
export
template <T type>
fn __init__(this &Reply<T>) void
```

### `fn __drop__(this &Reply<T>) void`

```
export
template <T type>
fn __drop__(this &Reply<T>) void
```

### `fn set(this &Reply<T>, value T) void`

Stores `value` and wakes everyone waiting for it.

Called once, by the task that produces the answer.

```
export
template <T type>
fn set(this &Reply<T>, value T) void
```

### `fn wait(this &Reply<T>) T except`

Waits for the value and returns it.

The wait happens on the same mutex the answer is stored under, so a `set` that
races this call is either seen before the wait or wakes it.

```
export
template <T type>
fn wait(this &Reply<T>) T except
```

### `fn await(this &Reply<T>) Coroutine<void, void, void>`

Waits for the value without holding the scheduler.
/
/ This is the coroutine's `wait`: `yield from reply.await()` suspends the
/ task until the value is there, and the executor goes on with the others
/ meanwhile. It is spelled `yield from` rather than called, because waiting is
/ a suspension and the only suspension a coroutine has is `yield`.
/
/ The check is a poll: each turn looks at `_closed` and hands the turn on. A
/ scheduler with timers would be woken instead; this one has no way to know
/ when a reply will arrive, so the task keeps taking its turn until it does.

```
@coroutine
export
template <T type>
fn await(this &Reply<T>) Coroutine<void, void, void>
```

### `fn ready(this &const Reply<T>) bool`

Whether the value has arrived.

A coroutine waiting for it polls this and yields; a thread waits with `wait`.

```
export
template <T type>
fn ready(this &const Reply<T>) bool
```

### `fn value(this &const Reply<T>) T except`

The value, or `none` when it has not arrived.

```
export
template <T type>
fn value(this &const Reply<T>) T except
```

### `fn get(this &const Reply<T>) T`

The value, for a caller that has already established it arrived.

Reading before `set` is a fault in the program, not a condition to handle, so
it aborts rather than raising.

```
export
template <T type>
fn get(this &const Reply<T>) T
```
