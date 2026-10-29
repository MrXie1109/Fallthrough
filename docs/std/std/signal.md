# `std.signal`

Signal handlers.

    installed Handler = std.install(2, &on_interrupt)??;

The C runtime's `signal` is the smallest portable way to say "call this
function when this signal arrives": it returns the previous handler, which the
caller may put back. A handler must be quick and must not allocate; what it
should do is set a flag the main loop reads.

# Portable subset

The constants below are the signals every platform here numbers the same way
(`SIGINT` and `SIGTERM`); `SIGKILL` cannot be caught anywhere, and asking to
is a call that returns the previous handler without ever running one.
`std.os` supplies the process id `signal_self` delivers to.

## Declarations

### `export type Handler {`

The handler a program installs.

```
export type Handler {
    // The function, or null for the platform's default.
    _function *fn(int) void;
    // The signal number it is for.
    _number int;
}
```

### `export enum Signal int {`

A signal number.

```
export enum Signal int {
    // The hang-up signal, `SIGHUP`.
    HUP,
    // The interrupt signal, `SIGINT`.
    INT,
    // The termination signal, `SIGTERM`.
    TERM,
    // The user signal 1, `SIGUSR1` on the platforms here.
    USR1,
}
```

### `fn signal_number(sig Signal) int`

The number the C library uses for a signal.

- `sig`: the signal to name

```
fn signal_number(sig Signal) int
```

### `export fn install(sig Signal, handler *fn(int) void) Handler except {`

Installs `handler` for `number`, returning the previous handler.

The returned handler can be put back by installing it again; a null function
in it means the platform's default, which `restore` installs.

```
export fn install(sig Signal, handler *fn(int) void) Handler except {
    if (handler == nullptr) {
        raise "SignalError": "a handler cannot be null; use restore";
    }
    previous *fn(int) void = signal(signal_number(sig), handler);
    installed Handler = .{ previous, signal_number(sig) };
    return installed;
}
```

### `export fn restore(sig Signal) bool {`

Puts the default handler back for `number`.

```
export fn restore(sig Signal) bool {
    signal(signal_number(sig), nullptr);
    return true;
}
```

### `export fn is_default(this &const Handler) bool {`

Whether a handler is the platform's default (null).

```
export fn is_default(this &const Handler) bool {
    return this._function == nullptr;
}
```

### `export fn number(this &const Handler) int {`

The signal a handler record is for.

```
export fn number(this &const Handler) int {
    return this._number as int;
}
```

### `export fn function(this &const Handler) *fn(int) void {`

The function a handler record holds, or null for the default.

```
export fn function(this &const Handler) *fn(int) void {
    return this._function;
}
```

### `export fn signal_self(sig Signal) bool {`

Sends `number` to this process, as a program does to try its own handler.

```
export fn signal_self(sig Signal) bool {
    return kill(os.pid(), signal_number(sig)) == 0;
}
```
