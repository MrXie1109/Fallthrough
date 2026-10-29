# `std.concurrent`

Concurrency: a promise and its future, a channel, and a thread pool.

The three answer different questions:

* `Promise<T>`/`Future<T>` -- one thread produces a value, another waits for
  it. The promise sets it once; the future gets it once and may wait.
* `Channel<T>` -- many values in order from one side to the other, with
  `send`, `recv` and `close`.
* `ThreadPool` -- a fixed set of threads taking jobs from a queue, so a
  program does not create one thread per task.

    channel Channel<int> = Channel<int>();
    producer->send(1);
    value int = channel.recv()??;

# Sharing

A promise and its future keep one control block alive with a reference count;
either may die first. A channel is a plain object -- the caller keeps it alive
for as long as both sides use it, which is what `&channel` into a thread's
context does. A pool owns its threads and stops them when it is destroyed.

## Declarations

### `type PromiseControl {`

------------------------------------------------------------- promise/future
The state a promise and a future share.

The value is stored behind a pointer so that a type with no default
constructor still works: it is allocated when the promise is set, and the
last owner frees it.

```
export
template <T type>
type PromiseControl {
    // How many owners there are: the promise and every future.
    _refs Atomic<int>;
    // Guards the value and the readiness flag.
    _mutex Mutex;
    // Wakes the future when the value arrives.
    _arrived CondVar;
    // Whether the value has been set.
    _ready bool;
    // The value, or null before it is set.
    _value *T;
}
```

### `fn new_control() *PromiseControl<T>`

Creates the shared state.

```
template <T type>
fn new_control() *PromiseControl<T>
```

### `fn release_control(control *PromiseControl<T>) void`

Drops one reference, freeing the state when the last one goes.

- `control`: the shared state to drop a reference to; it must be a live
  block and never null, because the count lives inside it

```
template <T type>
fn release_control(control *PromiseControl<T>) void
```

### `type Promise {`

The writing end: one value, set once.

```
export
template <T type>
type Promise {
    // The state shared with the future.
    _control *PromiseControl<T>;
}
```

### `fn __init__(this &Promise<T>) void`

Creates a promise with a fresh state.

```
export
template <T type>
fn __init__(this &Promise<T>) void
```

### `fn future(this &Promise<T>) Future<T>`

The future that will receive the value.

```
export
template <T type>
fn future(this &Promise<T>) Future<T>
```

### `fn set(this &Promise<T>, value T) void except`

Sets the value and wakes the future.

Setting a promise twice is a no-op: the first value is the answer, and a
second one would silently break the rule that a future waits for one.

- `value`: the value the future will receive; it is copied into a fresh
  allocation, and on a second call it is ignored and never stored

```
export
template <T type>
fn set(this &Promise<T>, value T) void except
```

### `fn __drop__(this &Promise<T>) void`

Releases the promise's reference.

```
export
template <T type>
fn __drop__(this &Promise<T>) void
```

### `type Future {`

The reading end: waits for the value the promise sets.

```
export
template <T type>
type Future {
    // The state shared with the promise.
    _control *PromiseControl<T>;
}
```

### `fn __init__(this &Future<T>) void`

Creates an unbound future; `Promise.future()` is what fills it in.

```
export
template <T type>
fn __init__(this &Future<T>) void
```

### `fn __init__(this &Future<T>, control *PromiseControl<T>) void`

Binds a future to an existing state, taking one reference.

- `control`: the state to bind to, or null for an unbound future; the
  reference this future holds is taken by the caller, not counted here

```
export
template <T type>
fn __init__(this &Future<T>, control *PromiseControl<T>) void
```

### `fn ready(this &Future<T>) bool`

Whether the value has arrived.

```
export
template <T type>
fn ready(this &Future<T>) bool
```

### `fn get(this &Future<T>) T except`

Waits for the value and returns it.

```
export
template <T type>
fn get(this &Future<T>) T except
```

### `fn __drop__(this &Future<T>) void`

Releases the future's reference.

```
export
template <T type>
fn __drop__(this &Future<T>) void
```

### `type Channel {`

------------------------------------------------------------------- channel
A queue one side writes and the other reads, in order.

`send` appends and wakes a reader; `recv` waits while the queue is empty and
raises `ChannelClosed` when the channel is closed and empty; `close` stops
new values and wakes every waiter.

```
export
template <T type>
type Channel {
    // Guards the queue and the closed flag.
    _mutex Mutex;
    // Wakes a reader when a value arrives, and every reader on close.
    _filled CondVar;
    // The values waiting to be read.
    _queue Array<T>;
    // Whether `close` has been called.
    _closed bool;
}
```

### `fn __init__(this &Channel<T>) void`

Creates an open channel.

```
export
template <T type>
fn __init__(this &Channel<T>) void
```

### `fn send(this &Channel<T>, value T) void except`

Appends a value and wakes one reader.

- `value`: the value to queue; it moves into the channel, and is not stored
  at all when the channel is already closed (which raises `ChannelClosed`)

```
export
template <T type>
fn send(this &Channel<T>, value T) void except
```

### `fn recv(this &Channel<T>) T except`

Takes the next value, waiting while the queue is empty.

```
export
template <T type>
fn recv(this &Channel<T>) T except
```

### `fn try_recv(this &Channel<T>) ?T`

Takes the next value, or returns an absent optional when there is none.

```
export
template <T type>
fn try_recv(this &Channel<T>) ?T
```

### `fn close(this &Channel<T>) void`

Stops new values and wakes every reader.

```
export
template <T type>
fn close(this &Channel<T>) void
```

### `fn closed(this &Channel<T>) bool`

Whether `close` has been called.

```
export
template <T type>
fn closed(this &Channel<T>) bool
```

### `type Job {`

---------------------------------------------------------------- thread pool
One job: what to call and what to hand it.

```
type Job {
    // The function to call.
    task *fn(*byte) uint;
    // Its argument.
    argument *byte;
}
```

### `export type ThreadPool {`

A fixed set of threads taking jobs from a queue.

```
export type ThreadPool {
    // Guards the queue and the stopping flag.
    _mutex Mutex;
    // Wakes a worker when a job arrives, and every worker on shutdown.
    _jobs CondVar;
    // The jobs waiting to run.
    _queue Array<Job>;
    // The workers.
    _threads Array<Thread>;
    // Whether `shutdown` has been called.
    _stopping bool;
    // How many workers `start` creates.
    _workers usize;
}
```

### `export fn __init__(this &ThreadPool, workers usize) void {`

Creates a pool that will run `workers` workers once `start` is called.

The threads are not started here: a constructor runs on a temporary, and a
worker that was handed the temporary's address would read a pool that moved
when the constructor returned. `start` is called on the object in its final
place.

```
export fn __init__(this &ThreadPool, workers usize) void {
    (&this._mutex)->__init__();
    (&this._jobs)->__init__();
    (&this._queue)->__init__();
    (&this._threads)->__init__();
    this._stopping = false;
    this._workers = workers;
}
```

### `export fn start(this &ThreadPool) void except {`

Starts the workers.

```
export fn start(this &ThreadPool) void except {
    index usize = 0;
    while (index < this._workers) {
        this._threads.push(Thread(worker_entry, &this as *byte)??);
        index += 1;
    }
}
```

### `fn worker_entry(argument *byte) uint`

The workers' body: take jobs until told to stop.

- `argument`: the address of the pool the worker serves, passed through the
  thread as a `*byte` and cast back here; never null

```
fn worker_entry(argument *byte) uint
```

### `fn work(this &ThreadPool) uint`

One worker's loop.

```
fn work(this &ThreadPool) uint
```

### `export fn submit(this &ThreadPool, task *fn(*byte) uint, argument *byte) void {`

Queues a job and wakes one worker.

```
export fn submit(this &ThreadPool, task *fn(*byte) uint, argument *byte) void {
    this._mutex.lock();
    job Job = .{ task, argument };
    this._queue.push(job);
    this._jobs.notify_one();
    this._mutex.unlock();
}
```

### `export fn shutdown(this &ThreadPool) void except {`

Tells every worker to finish the queued jobs and stop, and waits for them.

```
export fn shutdown(this &ThreadPool) void except {
    this._mutex.lock();
    this._stopping = true;
    this._jobs.notify_all();
    this._mutex.unlock();
    for (thread &Thread : this._threads) {
        thread.join()??;
    }
    this._threads.clear();
}
```

### `export fn __drop__(this &ThreadPool) void {`

Stops the pool.

```
export fn __drop__(this &ThreadPool) void {
    this._mutex.lock();
    this._stopping = true;
    this._jobs.notify_all();
    this._mutex.unlock();
    for (thread &Thread : this._threads) {
        try {
            thread.join()??;
        } except {
            // A destructor cannot raise; a thread that could not be joined is
            // left to the process, which is what an unhandled join would do.
        }
    }
    this._threads.clear();
    (&this._queue)->__drop__();
    (&this._threads)->__drop__();
    (&this._mutex)->__drop__();
    (&this._jobs)->__drop__();
}
```
