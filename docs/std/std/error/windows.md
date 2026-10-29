# `std.error.windows`

The system's error number, on Windows.

The C runtime keeps its own error number -- `_errno` is the accessor Windows
uses where POSIX has `__errno_location` -- and that is the number a failed
`fopen` sets. The Win32 error is a separate number; `last_win32_error` asks
for it, for the calls that set it instead.

## Declarations

### `link kernel32 dynamic from default;`

```
link kernel32 dynamic from default;
```

### `export fn last_error() int {`

The C runtime's error number for this thread.

```
export fn last_error() int {
    return *_errno();
}
```

### `export fn clear_error() void {`

Clears the C runtime's error number.

```
export fn clear_error() void {
    *_errno() = 0;
}
```

### `export fn error_text(code int) *const char {`

The C runtime's text for `code`.

```
export fn error_text(code int) *const char {
    return strerror(code);
}
```

### `export fn last_win32_error() uint {`

The Win32 error number of the last call that set one.

```
export fn last_win32_error() uint {
    return GetLastError();
}
```
