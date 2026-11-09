# `std.go`

`std.go`: handing a coroutine to a global executor.

A coroutine whose three types are all `void` is a sequence of suspension
points and nothing else:

    @coroutine
    fn worker(n int) std.Coroutine<void, void, void> {
        i int = 0;
        while (i < n) {
            i += 1;
            yield;              // hand the executor on to the next task
        }
    }

`go` puts one in a global queue and returns at once; `settle` runs the queue
until it is empty. Between them they are the whole of the executor:

    go(worker(5));
    go(worker(5));
    settle();

# What "running" means here

`resume` runs a task to its next `yield` and answers whether it suspended.
`settle` takes one task at a time from the front of the queue, resumes it,
and puts it back at the back when it suspended. So the tasks interleave at
their `yield`s, and the whole thing is one thread taking turns -- not
concurrency. That is enough for a program that wants to do several things at
once without a thread per thing.

# Communication

`go` carries no values. A task that has something to report is given a place
to report it, and a way to say it has:

    fn fetch(result &String, ready &Event) std.Coroutine<void, void, void> {
        result = http_get(url);
        ready.set();
    }

    fn main() int except {
        result String;
        ready Event;
        go(fetch(result, ready));
        ready.wait();
        use(result);
        return 0;
    }

The parameter is written `&String` and the argument is written `result`: a
value in parameter position satisfies a reference parameter, and the address
is what is passed.

# Interrupting

There is no cancel and no handle. A task stops being run when `settle` stops
taking it from the queue -- which is what happens when `settle` returns and
the program ends. A task that wants to stop early stops itself, by returning.

## Declarations

### `type Task = Coroutine<void, void, void>;`

The one kind of task this executor runs: a coroutine with nothing to send,
nothing to yield, and nothing to return.

```
export
type Task = Coroutine<void, void, void>;
```

### `type TaskQueue {`

The queue one thread fills and another drains.

Every operation happens with `_mutex` held, and `_work` wakes the scheduler
when a task arrives or a task finishes. `_running` says the scheduler has
taken a task and not yet put it back or dropped it, which is what lets
`settle` know that an empty queue still has work in flight.

```
type TaskQueue {
    _items Queue<Task>;
    _mutex Mutex;
    _work CondVar;
    _busy bool;
}
```

### `fn __init__(this &TaskQueue) void`

```
fn __init__(this &TaskQueue) void
```

### `fn __drop__(this &TaskQueue) void`

```
fn __drop__(this &TaskQueue) void
```

### `fn push(this &TaskQueue, task &Task) void`

Adds a task at the back, taking the task and leaving the caller with nothing.

A coroutine owns its frame, so the queue has to take it: a copy would leave
the caller and the queue holding one frame and both would release it. Taking
it is spelled by moving the task into the queue and clearing the caller's
copy, because a reference parameter names the caller's storage.

```
fn push(this &TaskQueue, task &Task) void
```

### `fn pop(this &TaskQueue, task &Task) bool`

Takes the task at the front, or answers `false` when there is none.

```
fn pop(this &TaskQueue, task &Task) bool
```

### `fn requeue(this &TaskQueue, task &Task) void`

Puts a suspended task back at the back without a wake-up of its own: its turn
comes again after the tasks already waiting.

```
fn requeue(this &TaskQueue, task &Task) void
```

### `fn release(this &TaskQueue) void`

Records that the task taken has finished.

```
fn release(this &TaskQueue) void
```

### `fn take(this &TaskQueue, task &Task) bool`

Waits until there is a task, takes it, and records that it is in flight.

```
fn take(this &TaskQueue, task &Task) bool
```

### `fn wait_idle(this &TaskQueue) void`

Waits until nothing is queued and nothing is in flight.

```
fn wait_idle(this &TaskQueue) void
```

### `tasks TaskQueue;`

The queue the scheduler drains and `go` fills.

A module-level variable is one object for the whole program, so every module
that calls `go` is submitting to the same executor -- which is what makes an
executor *global* rather than one per caller.

```
tasks TaskQueue;
```

### `started bool = false;`

Whether the scheduler has been started.

```
started bool = false;
```

### `failed int = 0;`

How many tasks have ended by raising, since the program started.

```
failed int = 0;
```

### `fn scheduler(_ *byte) uint`

The scheduler: takes one task at a time from the queue and runs it to its next
suspension, putting it back when it suspends and taking its answer when it
finishes. It runs for as long as the program does.

```
fn scheduler(_ *byte) uint
```

### `scheduler_thread Thread;`

The thread the scheduler runs on.

```
scheduler_thread Thread;
```

### `fn go(task &Task) void`

Hands `task` to the global executor and returns at once.

The first call starts the scheduler thread; the task is queued and the
scheduler picks it up. `settle` is what waits for the queue to drain.

- `task`: the coroutine to run; the call takes it and the caller does not see
  it again

```
export
fn go(task &Task) void
```

### `fn settle() int`

Waits until no task is queued and none is running, and answers how many tasks
failed since the program started.

This call does no scheduling of its own: the scheduler thread runs the tasks,
and this waits for it to run out of work. Newly scheduled tasks are waited
for too, because the condition is the queue and the in-flight task rather than
a count.

```
export
fn settle() int
```

### `fn placeholder() Coroutine<void, void, void>`

A coroutine of the type this executor runs, declared here so that this module
carries `resume` and `result` of that type.

Those two are emitted by whichever module knows a coroutine's frame, and the
executor is library code: without a coroutine of its own it would refer to
two symbols only a *program* could define, and a program that schedules
nothing would fail to link. Nothing ever runs this one; it is here so that the
module which runs tasks also says how a task of that shape is run.

```
@coroutine
fn placeholder() Coroutine<void, void, void>
```
