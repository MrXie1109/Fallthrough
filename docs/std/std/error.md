# `std.error`

The system's error number, and the text it gives that number.

A C call that fails says why in a small integer and a message. The message is
for people; the number is what a program branches on, so both are here: the I/O
errors carry the number, and a caller can ask the system the same question.

## Declarations

### `export fn last_error() int {`

The error number of the last C call that failed on this thread.

```
export fn last_error() int {
    return *__errno_location();
}
```

### `export fn clear_error() void {`

Clears the error number, so a stale one is not read as this call's answer.

```
export fn clear_error() void {
    *__errno_location() = 0;
}
```

### `export fn error_text(code int) *const char {`

The system's text for `code`.

```
export fn error_text(code int) *const char {
    return strerror(code);
}
```
