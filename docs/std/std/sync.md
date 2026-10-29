# `std.sync`

Synchronisation: a mutex, a condition variable, an atomic value, threads, and
a way to sleep.

# What is underneath

The operating system provides all of it, and *which* operating system is a
file: `sync_sys.ftm` names the POSIX calls (`pthread_mutex_t`,
`pthread_cond_t`, `pthread_create`), and `sync_sys.windows.ftm` names the
Windows ones (`SRWLOCK`, `CONDITION_VARIABLE`, `CreateThread`). The loader
reads the file for the target platform, so `Mutex`, `CondVar` and `Thread` are
the same types with the same methods everywhere.

A `Mutex` holds the platform's object as storage of the right size and
alignment, and every operation hands its address to the platform module -- so
a lock is the system's lock, not a second implementation of one that would
have to be believed.

The storage is zeroed when it is created, which is what
`PTHREAD_MUTEX_INITIALIZER` is on POSIX and what `SRWLOCK_INIT` is on Windows;
a mutex that is copied after being used is not a mutex, so the types here are
not copyable and a program passes them by reference.

# The atomic value is locked, not lock-free

`Atomic<T>` serialises its own reads and writes behind a mutex of its own. It
is atomic in the sense that matters -- no read can see half of a write, and no
two increments lose each other -- and it is not lock-free. A lock-free version
waits for a language intrinsic that compiles to the platform's own atomic
instructions; the interface here is what a program would use either way.

## Declarations

### `export type Mutex {`

A mutual-exclusion lock.

`pthread_mutex_t` is forty bytes and eight-byte aligned on the platforms this
library targets, which is why the first field is a word: it is what makes the
aligned address the C library expects.

```
export type Mutex {
    // The first word, which aligns the object, and the bytes after it.
    // The size is the largest of the platforms: glibc's pthread_mutex_t is 40
    // bytes, Darwin's 64, and Win32's SRWLOCK one word. The first word is where
    // the lock lives on every one of them.
    _first u64;
    _rest [64]byte;
}
```

### `export type CondVar {`

A condition variable, in the storage a `pthread_cond_t` needs.

```
export type CondVar {
    _first u64;
    _rest [56]byte;
}
```

### `export type Lock {`

Locks a mutex when created and unlocks it when destroyed.

The mutex is taken by *pointer* so that this holds the caller's mutex rather
than a copy: `guard Lock = Lock(&mutex);` locks for the length of the guard's
scope, however the scope is left.

```
export type Lock {
    _mutex *Mutex;
}
```

### `export fn __init__(this &Mutex) void {`

Creates the mutex. The zeroed storage is an unlocked mutex, which is what
`PTHREAD_MUTEX_INITIALIZER` is on this platform.

```
export fn __init__(this &Mutex) void {
    // Every byte, not just the first: the C library reads its type field out of
    // the middle of the object, and a byte left as the stack had it is exactly
    // how a lock comes back `EINVAL`. What a zeroed one means is the platform's
    // answer, so the platform module makes the final call.
    this._first = 0;
    for (index usize = 0; index < 64; index += 1) {
        this._rest[index] = 0 as byte;
    }
    sync_sys.mutex_init(&this._first as *byte);
}
```

### `export fn lock(this &Mutex) void {`

Locks the mutex, waiting while someone else holds it.

A lock never fails on a mutex that was not corrupted, so a failure aborts:
returning with the mutex unlocked would be the one thing the caller cannot
allow.

```
export fn lock(this &Mutex) void {
    if (!sync_sys.mutex_lock(&this._first as *byte)) {
        util.panic("Mutex lock failed");
    }
}
```

### `export fn unlock(this &Mutex) void {`

Unlocks the mutex.

```
export fn unlock(this &Mutex) void {
    if (!sync_sys.mutex_unlock(&this._first as *byte)) {
        util.panic("Mutex unlock failed");
    }
}
```

### `export fn __im_init__(this &Lock, mutex *Mutex) void {`

Creates a lock on `mutex`, which is held until the lock goes.

```
export fn __im_init__(this &Lock, mutex *Mutex) void {
    mutex->lock();
    this._mutex = mutex;
}
```

### `export fn __drop__(this &Lock) void {`

Releases the mutex the lock held.

```
export fn __drop__(this &Lock) void {
    if (this._mutex != nullptr) {
        this._mutex->unlock();
        this._mutex = nullptr;
    }
}
```

### `export fn __init__(this &CondVar) void {`

Creates the condition variable, in the same zeroed state
`PTHREAD_COND_INITIALIZER` has.

```
export fn __init__(this &CondVar) void {
    this._first = 0;
    for (index usize = 0; index < 56; index += 1) {
        this._rest[index] = 0 as byte;
    }
    sync_sys.cond_init(&this._first as *byte);
}
```

### `export fn wait(this &CondVar, mutex *Mutex) void {`

Waits until the condition is signalled, releasing the mutex while it waits.

The mutex is taken by pointer because it has to be *the* mutex the waiter
locked: a condition variable and a mutex are one pair, and the library
releases and reacquires exactly the one it is given.

```
export fn wait(this &CondVar, mutex *Mutex) void {
    if (!sync_sys.cond_wait(&this._first as *byte, &mutex->_first as *byte)) {
        util.panic("CondVar wait failed");
    }
}
```

### `export fn wait(this &CondVar, mutex &Mutex, predicate *fn() bool) void {`

Waits until `predicate` is true, re-checking after every wake.

The loop is the whole point: a condition variable may wake for no reason, and
the state the predicate reads is protected by `mutex`, which is held while
the predicate is asked.

```
export fn wait(this &CondVar, mutex &Mutex, predicate *fn() bool) void {
    while (!predicate()) {
        this.wait(&mutex);
    }
}
```

### `export fn notify_one(this &CondVar) void {`

Wakes one waiter.

```
export fn notify_one(this &CondVar) void {
    if (!sync_sys.cond_signal(&this._first as *byte)) {
        util.panic("CondVar signal failed");
    }
}
```

### `export fn notify_all(this &CondVar) void {`

Wakes every waiter.

```
export fn notify_all(this &CondVar) void {
    if (!sync_sys.cond_broadcast(&this._first as *byte)) {
        util.panic("CondVar broadcast failed");
    }
}
```

### `export type Thread {`

A thread of the operating system, started from a function.

The function has the shape the system calls it with: one address in, one word
out. A program that wants to pass several values gives them one home and
passes its address, which is what the `*byte` is for; the returned word is a
thread's exit value, and a function that has nothing to report returns `0`.

```
export type Thread {
    // The platform's handle. POSIX's `pthread_t` and Windows' `HANDLE` are both
    // one word, so the storage is a pointer on both and the platform module is
    // what knows which word it is.
    _handle *byte;
}
```

### `export fn __init__(this &Thread, entry *fn(*byte) uint, argument *byte) void except {`

Starts `entry` on a new thread, with `argument` handed to it.

```
export fn __init__(this &Thread, entry *fn(*byte) uint, argument *byte) void except {
    this._handle = nullptr;
    if (!sync_sys.thread_start(&this._handle, entry, argument)) {
        raise "ThreadError": "the thread could not be started";
    }
}
```

### `export fn join(this &Thread) void except {`

Waits for the thread to finish.

```
export fn join(this &Thread) void except {
    if (this._handle == nullptr) {
        return;
    }
    if (!sync_sys.thread_join(this._handle)) {
        raise "ThreadError": "the thread could not be joined";
    }
    this._handle = nullptr;
}
```

### `export fn sleep(milliseconds int) void {`

Suspends the calling thread for `milliseconds`.

```
export fn sleep(milliseconds int) void {
    if (milliseconds <= 0) {
        return;
    }
    sync_sys.sleep_ms(milliseconds as usize);
}
```

### `export enum AtomicOrder int {`

The ordering an atomic operation asks for.

The names are C++'s: `RELAXED` promises nothing about other memory,
`ACQUIRE`/`RELEASE` order one direction, `ACQ_REL` both, and `SEQ_CST` is the
single total order a program gets unless it says otherwise.

```
export enum AtomicOrder int {
    // No ordering beyond the atomicity of the operation itself.
    RELAXED,
    // Later reads see writes released before the corresponding release.
    ACQUIRE,
    // Earlier writes are visible to a reader that acquires after it.
    RELEASE,
    // Both of the above.
    ACQ_REL,
    // The default: one total order over all sequentially consistent operations.
    SEQ_CST,
}
```

### `DEFAULT_ATOMIC_ORDER const AtomicOrder = AtomicOrder.SEQ_CST;`

The order an atomic operation gets when its caller does not name one.

A named constant rather than the enum member at each default: a default that
crosses a module boundary is folded where it is declared, and a constant is
the shape that folding reads.

```
DEFAULT_ATOMIC_ORDER const AtomicOrder = AtomicOrder.SEQ_CST;
```

### `fn ensure_order(order AtomicOrder) void`

Checks that an order is one of the five this library defines.

The current implementation is a mutex, whose lock and unlock are already
sequentially consistent: the parameter is accepted and validated, and the
operation runs as `SEQ_CST` whatever was asked. That is stronger than any
argument here, so a program that passes a weaker order is still correct; the
parameter is what the real atomic path will read.

- `order`: the order the caller asked for; it is validated and otherwise
  ignored, because the mutex already gives `SEQ_CST`

```
fn ensure_order(order AtomicOrder) void
```

### `type Atomic {`

A value whose reads and writes are atomic.

The mutex is the atomicity: a read takes it, copies the value, and gives it
back; an update takes it, examines and stores, and gives it back. Nothing
observes a half-written value, and two updates never lose each other.

```
export
template <T type>
type Atomic {
    _mutex Mutex;
    _value T;
}
```

### `fn __im_init__(this &Atomic<T>, value T) void`

Creates the value.

- `value`: the value to store; its storage moves into the atomic

```
export
template <T type>
fn __im_init__(this &Atomic<T>, value T) void
```

### `fn load(this &Atomic<T>, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T`

Returns the value.

- `order`: the order requested; it is validated, and the read is `SEQ_CST`
  whatever is asked

```
export
template <T type>
fn load(this &Atomic<T>, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T
```

### `fn store(this &Atomic<T>, value T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) void`

Stores a new value.

- `value`: the value to store; its storage moves into the atomic, so the
  caller does not read it afterwards
- `order`: the order requested; it is validated, and the write is `SEQ_CST`
  whatever is asked

```
export
template <T type>
fn store(this &Atomic<T>, value T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) void
```

### `fn fetch_add(this &Atomic<T>, delta T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T`

Adds `delta` and returns the value it had before.

- `delta`: the amount to add, of the atomic's own type; a negative value
  subtracts, and an unsigned type wraps on overflow
- `order`: the order requested; it is validated, and the update is `SEQ_CST`
  whatever is asked

```
export
template <T type>
fn fetch_add(this &Atomic<T>, delta T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T
```

### `fn fetch_sub(this &Atomic<T>, delta T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T`

Subtracts `delta` and returns the value it had before.

- `delta`: the amount to subtract, of the atomic's own type; a negative value
  adds, and an unsigned type wraps on underflow
- `order`: the order requested; it is validated, and the update is `SEQ_CST`
  whatever is asked

```
export
template <T type>
fn fetch_sub(this &Atomic<T>, delta T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T
```

### `fn compare_exchange(this &Atomic<T>, expected T, wanted T) bool`

Stores `wanted` when the value still equals `expected`, returning whether it
did.

- `expected`: the value the atomic must still hold; it is compared and not
  written back, so a failed exchange leaves the caller's copy alone
- `wanted`: the value stored when the comparison matches; it moves into the
  atomic, and is untouched when it does not

```
export
template <T type>
fn compare_exchange(this &Atomic<T>, expected T, wanted T) bool
```

### `fn compare_exchange(`

Stores `wanted` when the value still equals `expected`, under the given order.

- `expected`: the value the atomic must still hold; it is compared and not
  written back, so a failed exchange leaves the caller's copy alone
- `wanted`: the value stored when the comparison matches; it moves into the
  atomic, and is untouched when it does not
- `order`: the order requested; it is validated, and the exchange is
  `SEQ_CST` whatever is asked

```
export
template <T type>
fn compare_exchange(
    this &Atomic<T>,
    expected T,
    wanted T,
    order AtomicOrder
) bool
```

### `fn exchange(this &Atomic<T>, wanted T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T`

Exchanges the value for `wanted`, and returns what it held.

- `wanted`: the value to store; its storage moves into the atomic, and the
  value it replaced is returned as a copy
- `order`: the order requested; it is validated, and the exchange is
  `SEQ_CST` whatever is asked

```
export
template <T type>
fn exchange(this &Atomic<T>, wanted T, order AtomicOrder = DEFAULT_ATOMIC_ORDER) T
```

### `export type RwLock {`

---------------------------------------------------------------- read-write
A lock that many readers may hold at once, and a writer alone.

The readers' count and the writer's flag are protected by the mutex; the
condition variable is where a waiter sleeps. A waiting writer blocks new
readers only after it has begun waiting -- writers are not starved, and
readers are not blocked by a writer that is merely thinking about writing.

```
export type RwLock {
    // Guards the counters.
    _mutex Mutex;
    // Wakes the other side when the counters change.
    _changed CondVar;
    // How many readers hold the lock.
    _readers uint;
    // Whether a writer holds the lock.
    _writer bool;
    // How many writers are waiting for it.
    _waiting_writers uint;
}
```

### `export fn __init__(this &RwLock) void {`

Creates an unlocked read-write lock.

```
export fn __init__(this &RwLock) void {
    (&this._mutex)->__init__();
    (&this._changed)->__init__();
    this._readers = 0;
    this._writer = false;
    this._waiting_writers = 0;
}
```

### `export fn lock_shared(this &RwLock) void {`

Takes the lock for reading, waiting while a writer holds it.

```
export fn lock_shared(this &RwLock) void {
    this._mutex.lock();
    while (this._writer || this._waiting_writers > 0) {
        this._changed.wait(&this._mutex);
    }
    this._readers += 1;
    this._mutex.unlock();
}
```

### `export fn unlock_shared(this &RwLock) void {`

Releases a read lock.

```
export fn unlock_shared(this &RwLock) void {
    this._mutex.lock();
    if (this._readers > 0) {
        this._readers -= 1;
    }
    if (this._readers == 0) {
        this._changed.notify_all();
    }
    this._mutex.unlock();
}
```

### `export fn lock(this &RwLock) void {`

Takes the lock for writing, waiting while anyone holds it.

```
export fn lock(this &RwLock) void {
    this._mutex.lock();
    this._waiting_writers += 1;
    while (this._writer || this._readers > 0) {
        this._changed.wait(&this._mutex);
    }
    this._waiting_writers -= 1;
    this._writer = true;
    this._mutex.unlock();
}
```

### `export fn unlock(this &RwLock) void {`

Releases the write lock.

```
export fn unlock(this &RwLock) void {
    this._mutex.lock();
    this._writer = false;
    this._changed.notify_all();
    this._mutex.unlock();
}
```

### `export type Semaphore {`

---------------------------------------------------------------- semaphore
A counter that tasks may take from and give back.

`acquire` waits while the count is zero; `release` gives one back and wakes a
waiter. The count is protected by the mutex, and a conditional variable wakes
one waiter per release.

```
export type Semaphore {
    // Guards the count.
    _mutex Mutex;
    // Wakes one waiter per release.
    _available CondVar;
    // How many permits are free.
    _count uint;
}
```

### `export fn __init__(this &Semaphore, initial uint) void {`

Creates a semaphore with `initial` permits.

```
export fn __init__(this &Semaphore, initial uint) void {
    (&this._mutex)->__init__();
    (&this._available)->__init__();
    this._count = initial;
}
```

### `export fn acquire(this &Semaphore) void {`

Takes one permit, waiting while none is free.

```
export fn acquire(this &Semaphore) void {
    this._mutex.lock();
    while (this._count == 0) {
        this._available.wait(&this._mutex);
    }
    this._count -= 1;
    this._mutex.unlock();
}
```

### `export fn release(this &Semaphore) void {`

Gives one permit back.

```
export fn release(this &Semaphore) void {
    this._mutex.lock();
    this._count += 1;
    this._available.notify_one();
    this._mutex.unlock();
}
```

### `export fn available(this &Semaphore) uint {`

The number of free permits, without waiting.

```
export fn available(this &Semaphore) uint {
    this._mutex.lock();
    count uint = this._count;
    this._mutex.unlock();
    return count;
}
```

### `export type Once {`

-------------------------------------------------------------------- once
A function that runs once, however many callers reach it.

The state is `0` (not started), `1` (running) and `2` (finished). A caller
that finds it running waits for the one that runs it; a caller that finds it
finished returns at once.

```
export type Once {
    // Guards the state.
    _mutex Mutex;
    // Wakes waiters when the run finishes.
    _finished CondVar;
    // Which of the three states it is in.
    _state int;
}
```

### `export fn __init__(this &Once) void {`

Creates a not-yet-run `Once`.

```
export fn __init__(this &Once) void {
    (&this._mutex)->__init__();
    (&this._finished)->__init__();
    this._state = 0;
}
```

### `export fn call(this &Once, action *fn() void) void {`

Runs `action` if no caller has yet run it, and waits otherwise.

```
export fn call(this &Once, action *fn() void) void {
    this._mutex.lock();
    if (this._state == 2) {
        this._mutex.unlock();
        return;
    }
    if (this._state == 1) {
        while (this._state != 2) {
            this._finished.wait(&this._mutex);
        }
        this._mutex.unlock();
        return;
    }
    this._state = 1;
    this._mutex.unlock();
    action();
    this._mutex.lock();
    this._state = 2;
    this._finished.notify_all();
    this._mutex.unlock();
}
```

### `export type Barrier {`

----------------------------------------------------------------- barrier
A meeting point: every party waits until all of them have arrived.

Each round has a generation number. The last party to arrive starts the next
generation and wakes everyone; the rest wait until the number changes, so one
party cannot slip into the next round early.

```
export type Barrier {
    // Guards the counters.
    _mutex Mutex;
    // Wakes the parties of a round.
    _round CondVar;
    // How many parties a round has.
    _parties uint;
    // How many have arrived in this round.
    _waiting uint;
    // Which round it is.
    _generation uint;
}
```

### `export fn __init__(this &Barrier, parties uint) void {`

Creates a barrier for `parties` parties.

```
export fn __init__(this &Barrier, parties uint) void {
    (&this._mutex)->__init__();
    (&this._round)->__init__();
    this._parties = parties;
    this._waiting = 0;
    this._generation = 0;
}
```

### `export fn wait(this &Barrier) void {`

Arrives at the barrier and waits until every other party has arrived.

```
export fn wait(this &Barrier) void {
    this._mutex.lock();
    generation uint = this._generation;
    this._waiting += 1;
    if (this._waiting == this._parties) {
        this._waiting = 0;
        this._generation += 1;
        this._round.notify_all();
        this._mutex.unlock();
        return;
    }
    while (generation == this._generation) {
        this._round.wait(&this._mutex);
    }
    this._mutex.unlock();
}
```
