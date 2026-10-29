# `std.debug`

Debugging helpers: assertions, unreachable points, a breakpoint instruction.

    $ASSERT_DEBUG(count > 0);
    debug.printtrace(&String("reached the parser"));

# Always on

These checks are not compiled out: this language has no separate debug build,
and a module that silently changed what a program does under `--release` would
be worse than one whose cost is stated. `trace` writes to standard error and
is meant to be deleted, not to be left in a hot loop.

## Declarations

### `export fn assert(condition bool, message *const char, file *const char, line int) void except {`

Raises `DebugError` when `condition` is false.

```
export fn assert(condition bool, message *const char, file *const char, line int) void except {
    if (condition) {
        return;
    }
    raise "DebugError": "assertion failed: " + String(message)
        + " (" + String(file) + ":" + str(line) + ")";
}
```

### `export fn unreachable(message *const char, file *const char, line int) void except {`

Raises `DebugError` with `message`, for a place that should not be reached.

It is a raise rather than `abort`: the caller may catch it and report a bug
instead of a crash, which is what the program around it would want.

```
export fn unreachable(message *const char, file *const char, line int) void except {
    raise "DebugError": "unreachable: " + String(message)
        + " (" + String(file) + ":" + str(line) + ")";
}
```

### `export fn printtrace(message &const String) void {`

Writes `message` and a newline to standard error.
Named `printtrace` rather than `trace`: `std.log` already has a `trace`
pack template, and a program that imported both would reach whichever the
overload resolution saw first.

Through the C runtime rather than `std.io`: the debug module is what a
broken program reaches for, and it should not depend on the formatting and
file layers when the reason for the trace may be one of those layers.

```
export fn printtrace(message &const String) void {
    try {
        C.fprintf(C.stderr, "%s\n", message.data());
    } except {
    }
}
```

### `export fn breakpoint() void {`

Stops the program in a debugger when one is attached, and aborts otherwise.

`abort` is the C runtime's: a debugger breaks on it, and without one it ends
the process instead of returning to a program that has no next step.

```
export fn breakpoint() void {
    C.abort();
}
```

### `export define ASSERT_DEBUG(condition) std.assert(condition, #condition, $__builtin_FILE, $__builtin_LINE)??`

A condition that must hold.

```
export define ASSERT_DEBUG(condition) std.assert(condition, #condition, $__builtin_FILE, $__builtin_LINE)??
```

### `export define DEBUG_UNREACHABLE(message) std.unreachable(#message, $__builtin_FILE, $__builtin_LINE)??`

A place that must not be reached.

```
export define DEBUG_UNREACHABLE(message) std.unreachable(#message, $__builtin_FILE, $__builtin_LINE)??
```
