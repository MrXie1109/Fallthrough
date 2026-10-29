# `std.os`

The operating system: the process it runs in, the environment around it, and
the clock it measures.

The calls are POSIX on Unix and Win32 on Windows, chosen by the loader from
`os_sys.ftm` / `os_sys.windows.ftm`; the interface below is the same either
way.

    import std.os;
    os.pid();                       // this process
    os.getenv("HOME");              // the environment, raising KeyError
    os.exit(0);                     // leave
    os.monotonic_ns();              // a clock that never jumps

`std.os` is about the process and the system; files and paths are `std.io`
and `std.fs`, and so is the working directory's *path* -- `cwd` returns one.

## Declarations

### `export fn pid() int {`

The process's identifier, which is what a platform's tools call the PID.

```
export fn pid() int {
    return os_sys.pid();
}
```

### `export fn exit(code int) void {`

Ends the process with `code` as its exit status.

Nothing after this runs: no destructor, no `finally`. A program that has to
clean up exits from `main` instead, which unwinds the way any return does.

```
export fn exit(code int) void {
    os_sys.exit_process(code);
}
```

### `export fn getenv(name *const char) String except {`

The value of the environment variable `name`.

Raises `KeyError` when the variable is not set; `hasenv` is the question form
for a program for which that is ordinary.

```
export fn getenv(name *const char) String except {
    raw *const char = os_sys.env_raw(name);
    if (raw == nullptr) {
        raise "KeyError": "the environment has no such variable";
    }
    return String(raw);
}
```

### `export fn hasenv(name *const char) bool {`

Whether the environment has a variable named `name`.

```
export fn hasenv(name *const char) bool {
    return os_sys.env_raw(name) != nullptr;
}
```

### `export fn setenv(name *const char, value *const char) bool {`

Sets `name` in this process's environment.

It is this process's environment: a child started afterwards sees it, and the
parent's environment does not change.

```
export fn setenv(name *const char, value *const char) bool {
    return os_sys.setenv(name, value);
}
```

### `export fn cwd() Path except {`

The current working directory.

```
export fn cwd() Path except {
    buffer [4096]char;
    if (!os_sys.cwd_into(&buffer[0], 4096)) {
        raise "IOError": "the working directory could not be read";
    }
    return Path(&buffer[0]);
}
```

### `export fn chdir(path &const Path) bool {`

Changes the current working directory.

```
export fn chdir(path &const Path) bool {
    return os_sys.chdir_to(path.text());
}
```

### `export fn tempdir() Path {`

A temporary directory: `TMPDIR`, or the platform's own when it is unset.

```
export fn tempdir() Path {
    name *const char = os_sys.env_raw("TMPDIR");
    if (name == nullptr) {
        name = os_sys.env_raw("TEMP");
    }
    if (name == nullptr) {
        name = os_sys.env_raw("TMP");
    }
    if (name == nullptr) {
        name = os_sys.fallback_temp_dir();
    }
    return Path(name);
}
```

### `export fn home() Path {`

The user's home directory, from the platform's environment variable.

A program that runs without one -- a service, a container -- gets the current
directory rather than a failure, because "no home" is a state a program can
work in and a path is what it asked for.

```
export fn home() Path {
    name *const char = os_sys.env_raw(os_sys.home_variable());
    if (name == nullptr) {
        return Path(".");
    }
    return Path(name);
}
```

### `export fn system(command *const char) int {`

Runs `command` through the system's shell, returning its exit status.

A non-zero status is the command's answer, not a compile error: the caller
decides whether it failed. The shell is the platform's own, so the command is
written the way that shell writes it. A `String` command is passed as
`text.data()`, and a literal as itself.

```
export fn system(command *const char) int {
    return C.system(command);
}
```

### `export fn monotonic_ns() u64 {`

Nanoseconds on the monotonic clock: it never jumps backwards when the wall
clock is set, so a difference of two readings is a duration.

```
export fn monotonic_ns() u64 {
    return os_sys.monotonic_ns();
}
```

### `export fn sleep_ms(milliseconds int) void {`

Suspends the calling thread for `milliseconds`.

```
export fn sleep_ms(milliseconds int) void {
    if (milliseconds <= 0) {
        return;
    }
    // `std.sync` owns the sleep; importing it here would tie the two modules in
    // a circle for one call, so the platform module is what is called.
    os_sys.sleep_ms(milliseconds as usize);
}
```

### `export fn platform() *const char {`

The platform the program was built for: `linux`, `windows`, `macos`.

It is the compiler's `$__builtin_PLATFORM`, so it is a constant of the build rather
than something the running system answers.

```
export fn platform() *const char {
    return $__builtin_PLATFORM;
}
```

### `export fn arch() *const char {`

The architecture the program was built for: `x86_64`, `aarch64`.

```
export fn arch() *const char {
    return $__builtin_ARCH;
}
```
