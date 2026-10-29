# `std.error.macos`

The system's error number, as Darwin spells it.

The calls are the POSIX ones; Darwin names the thread's error pointer
`__error` rather than `__errno_location`.

## Declarations

### `export fn last_error() int {`

The error number of the last C call that failed on this thread.

```
export fn last_error() int {
    return *__error();
}
```

### `export fn clear_error() void {`

Clears the error number, so a stale one is not read as this call's answer.

```
export fn clear_error() void {
    *__error() = 0;
}
```

### `export fn error_text(code int) *const char {`

The system's text for `code`.

```
export fn error_text(code int) *const char {
    return strerror(code);
}
```
