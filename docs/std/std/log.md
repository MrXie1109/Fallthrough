# `std.log`

Logging: named levels, one line each, a filter, and a stream.

    std.log.info("starting on {}\n"? no -- the line break is the logger's);
    log.info("starting with {} items", count)??;

A log line is `[LEVEL] text`, written to standard error by default. The
level filter is one number: a message below the current level is dropped
before its text is built, so a disabled debug line costs nothing.

    log.setlevel(std.log.Level.DEBUG);
    log.debug("value = {}", value)??;

The stream is `std.err` unless the program changes it, which is what makes a
test or a daemon able to send logs elsewhere without every call changing.

# Threads

Writing is not synchronised here: a line from two threads may interleave. The
caller that needs whole lines from several threads wraps its logging in a
mutex; a logger with a lock of its own would be the same lock one level up.

## Declarations

### `export enum Level int {`

How loud a message is.

The numbers are ordered: a larger one is more important. `OFF` silences
everything, and is the value `setlevel` takes to do it.

```
export enum Level int {
    // For the finest step-by-step detail.
    TRACE,
    // For development detail.
    DEBUG,
    // For the ordinary running commentary.
    INFO,
    // For something surprising that is handled.
    WARN,
    // For a failure.
    ERROR,
    // Not a message level: silences the logger.
    OFF,
}
```

### `_target *File;`

Where the lines go.

```
_target *File;
```

### `_level int;`

The lowest level that is written.

```
_level int;
```

### `fn __setup__() void`

Points the logger at standard error and the ordinary level.

```
fn __setup__() void
```

### `export fn setlevel(level Level) void {`

Sets the lowest level that is written.

```
export fn setlevel(level Level) void {
    _level = level as int;
}
```

### `export fn level() Level {`

The lowest level that is written.

```
export fn level() Level {
    return _level as Level;
}
```

### `export fn settarget(target *File) void {`

Sends the lines somewhere other than standard error.

```
export fn settarget(target *File) void {
    _target = target;
}
```

### `export fn levelname(level Level) String {`

The text a level writes in front of its lines.

```
export fn levelname(level Level) String {
    if (level == Level.TRACE) {
        return String("TRACE");
    }
    if (level == Level.DEBUG) {
        return String("DEBUG");
    }
    if (level == Level.INFO) {
        return String("INFO");
    }
    if (level == Level.WARN) {
        return String("WARN");
    }
    return String("ERROR");
}
```

### `export fn enabled(level Level) bool {`

Whether a message at `level` is written.

```
export fn enabled(level Level) bool {
    return (level as int) >= _level;
}
```

### `fn write(level Level, values ...Ts) void except`

Writes one line at `level`, if the level is enabled.

The format rule is `std.print`'s: `{}` in the first argument stands for the
following values. The line ends with a newline the logger supplies.
- `level`: the level the line is written at, which is compared with the
  current one before any text is built
- `values`: the format and its arguments, under the rule `std.print` uses:
  the first value holds the `{}` and the rest fill them

```
export
template <Ts ...type>
fn write(level Level, values ...Ts) void except
```

### `fn trace(values ...Ts) void except`

Writes a trace line.
- `values`: the format and its arguments, as `write` takes them

```
export
template <Ts ...type>
fn trace(values ...Ts) void except
```

### `fn debug(values ...Ts) void except`

Writes a debug line.
- `values`: the format and its arguments, as `write` takes them

```
export
template <Ts ...type>
fn debug(values ...Ts) void except
```

### `fn info(values ...Ts) void except`

Writes an info line.
- `values`: the format and its arguments, as `write` takes them

```
export
template <Ts ...type>
fn info(values ...Ts) void except
```

### `fn warn(values ...Ts) void except`

Writes a warning line.
- `values`: the format and its arguments, as `write` takes them

```
export
template <Ts ...type>
fn warn(values ...Ts) void except
```

### `fn error(values ...Ts) void except`

Writes an error line.
- `values`: the format and its arguments, as `write` takes them

```
export
template <Ts ...type>
fn error(values ...Ts) void except
```
