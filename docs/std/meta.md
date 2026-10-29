# `meta`

Compile-time facts about the program being built.

These are the compiler's, not a library's: what the file is called, which
line a macro is written on, and which platform the binary is being built for
are answers only the compiler has. Each macro here wraps one of its builtins,
so a program writes `$meta.FILE` and the compiler's spelling (`$__builtin_FILE`)
stays an implementation detail.

    import meta;
    std.out.println("built from ", $meta.FILE, " on ", $meta.PLATFORM);

A macro is expanded before parsing, so these may stand anywhere a token may:
in an expression, as a constant, inside another macro. `WINDOWS` and `POSIX`
expand to `1`/`0`, which is what makes them usable as
`if ($meta.WINDOWS) @constexpr`.

This module is not part of `std`: a program that does not use it does not
import it.
The path of the file this macro is expanded in.

## Declarations

### `export define LINE $__builtin_LINE`

The number of the line the macro is on.

```
export define LINE $__builtin_LINE
```

### `export define COLUMN $__builtin_COLUMN`

The number of the column the macro starts at.

```
export define COLUMN $__builtin_COLUMN
```

### `export define PLATFORM $__builtin_PLATFORM`

The platform being built for, as text: `linux`, `windows`, `macos`.

```
export define PLATFORM $__builtin_PLATFORM
```

### `export define ARCH $__builtin_ARCH`

The architecture being built for, as text: `x86_64`, `aarch64`.

```
export define ARCH $__builtin_ARCH
```

### `export define WINDOWS $__builtin_OS_WINDOWS`

`1` when the program is being built for Windows, `0` otherwise.

```
export define WINDOWS $__builtin_OS_WINDOWS
```

### `export define POSIX $__builtin_OS_POSIX`

`1` when the program is being built for a POSIX system, `0` otherwise.

```
export define POSIX $__builtin_OS_POSIX
```

### `export define DATE $__builtin_DATE`

The date of the build, `YYYY-MM-DD`.

```
export define DATE $__builtin_DATE
```

### `export define TIME $__builtin_TIME`

The time of the build, `HH:MM:SS`.

```
export define TIME $__builtin_TIME
```
