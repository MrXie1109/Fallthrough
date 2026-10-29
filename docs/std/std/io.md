# `std.io`

I/O: one file type, the standard streams, and formatted reading and writing.

A `File` is a C stream and the direction it was opened for. Everything is one
of these: `std.out`, `std.err`, `std.in`, a file on disk, and a stream a C
library hands back. There is no separate reader or writer type -- a handle is
a handle, and what it can do is a question asked at run time, exactly as it is
of a terminal:

    if (f.readable()) { ... }      // opened for reading (or for both)
    if (f.writable()) { ... }      // opened for writing (or for both)

Operating on a handle the wrong way is therefore a run-time error rather than
a compile error: reading a write-only file raises, seeking a terminal raises,
and a closed file has nothing to say. That is the price of one type, and it is
what makes a terminal, a pipe, a file on disk and a memory stream the same
thing to every routine that takes a `File`.

# Opening

A file is opened from a path with one of C's three modes, or wrapped around a
C stream that is already open:

    f std.File = std.File("notes.txt", std.OpenMode.WRITE)??;
    log std.File = std.File(C.stderr, std.OpenMode.WRITE);
    src std.File = std.File(C.stdin, std.OpenMode.READ);

`OpenMode.READ` is `"rb"`, `OpenMode.WRITE` is `"wb"` (which truncates), and
`OpenMode.RW` is `"r+b"` (which needs the file to exist).

# Formatted I/O: `print`, `println`, `input`

Three members do the formatting, and their names say which way they go:

    std.out.print("x = ", 42, '\n');    // each argument rendered by `str`
    std.out.println("done");             // the same, then a newline
    std.in.input(n, word);               // one word per reference argument

`print` and `println` take values; `input` takes references, so what it reads
is written into the caller's variables. A type is printable when `str` can
render it (a built-in, or one declaring `__str__`), and readable when it
declares `__input__` -- and because the method name says the direction, those
are compile-time requirements, each on its own side.

Nothing is buffered here beyond what C's own stream already does. `flush` is
how a program asks for the bytes to be out.

## Declarations

### `export enum OpenMode int {`

What a file was opened for, in C's terms.

```
export enum OpenMode int {
    // `"rb"`: for reading.
    READ,
    // `"wb"`: for writing, which truncates the file.
    WRITE,
    // `"r+b"`: for reading and writing; the file must exist.
    RW,
}
```

### `export enum SeekFrom int {`

Which way a seek moves from.

The members are C's `SEEK_SET`, `SEEK_CUR` and `SEEK_END`, in that order.

```
export enum SeekFrom int {
    // From the beginning: the offset is an absolute position.
    START,
    // From where the stream is now.
    CURRENT,
    // From the end: an offset of zero is the end, a negative one moves back.
    END,
}
```

### `export type File {`

A stream and the direction it was opened for.

The fields are the C stream and the one character `peek` read ahead; the mode
is what `readable` and `writable` answer from. A default-constructed file has
no stream, which is what a program that opens later, or that may fail to open,
keeps in a variable: nothing about it is an error until it is used.

```
export type File {
    // The C stream behind the file, or null when there is none.
    _stream *byte;
    // A character read ahead by `peek`, or -1 when there is none.
    _pushed int;
    // What the file was opened for.
    _mode OpenMode;
}
```

### `export fn __init__(this &File) void {`

Creates a file with no stream: it has nothing to read or write until `open`.

```
export fn __init__(this &File) void {
    this._stream = nullptr;
    this._pushed = -1;
    this._mode = OpenMode.READ;
}
```

### `export fn __init__(this &File, stream *byte, mode OpenMode) void {`

Creates a file over a C stream that is already open.

This is how the standard streams are made, and how a stream from another C
library -- `popen`, `tmpfile`, a handle opened elsewhere -- becomes a file this
library can use. The mode is the caller's promise about the stream; nothing
can ask a C stream what it was opened for.

```
export fn __init__(this &File, stream *byte, mode OpenMode) void {
    this._stream = stream;
    this._pushed = -1;
    this._mode = mode;
}
```

### `export fn __init__(this &File, path *const char, mode OpenMode) void except {`

Creates a file over a path, opening it now.

A file that cannot be opened is an error, not an invalid file: the caller that
wants to decide for itself keeps an invalid file and calls `open`, whose
failure it can catch.

```
export fn __init__(this &File, path *const char, mode OpenMode) void except {
    this._stream = nullptr;
    this._pushed = -1;
    this._mode = mode;
    this.open(path, mode)??;
}
```

### `export fn open(this &File, path *const char, mode OpenMode) void except {`

Opens a path as this file, replacing whatever stream it held.

Reopening a file that already has a stream closes that stream first, so `open`
on a file in use is how a program moves it to another path.

```
export fn open(this &File, path *const char, mode OpenMode) void except {
    this.close()??;
    this._mode = mode;
    if (mode == OpenMode.READ) {
        this._stream = C.fopen(path, "rb");
    } else {
        if (mode == OpenMode.WRITE) {
            this._stream = C.fopen(path, "wb");
        } else {
            this._stream = C.fopen(path, "r+b");
        }
    }
    if (this._stream == nullptr) {
        raise "IOError": describe(String(path));
    }
}
```

### `export fn valid(this &const File) bool {`

Whether the file has a stream.

```
export fn valid(this &const File) bool {
    return this._stream != nullptr;
}
```

### `export fn readable(this &const File) bool {`

Whether the file may be read from.

A file opened for writing answers false, and so does one with no stream. The
question is asked at run time because the direction is a property of the
handle rather than of its type; a program that knows which it opened needs no
test, and one that was handed a `File` can ask.

```
export fn readable(this &const File) bool {
    return this._stream != nullptr
        && (this._mode == OpenMode.READ || this._mode == OpenMode.RW);
}
```

### `export fn writable(this &const File) bool {`

Whether the file may be written to.

```
export fn writable(this &const File) bool {
    return this._stream != nullptr
        && (this._mode == OpenMode.WRITE || this._mode == OpenMode.RW);
}
```

### `export fn read(this &File, buffer *char, count usize) usize except {`

Reads at most `count` characters, and returns how many were read.

```
export fn read(this &File, buffer *char, count usize) usize except {
    return this._read_bytes(buffer as *byte, count)??;
}
```

### `export fn read(this &File, buffer *byte, count usize) usize except {`

Reads at most `count` bytes, and returns how many were read.

```
export fn read(this &File, buffer *byte, count usize) usize except {
    return this._read_bytes(buffer, count)??;
}
```

### `export fn get(this &File) char except {`

Reads one character.

The end of the input is an error, not a value: a `char` has no code left over
to mean "nothing". A program that may reach the end asks `eof()` first, or
reads with `getline`, which answers how much it found.

```
export fn get(this &File) char except {
    code int = this._next_code()??;
    if (code < 0) {
        raise "EOFError": "the input ended where a character was expected";
    }
    return code as char;
}
```

### `export fn get(this &File, value &char) bool except {`

Reads one character into `value`, and answers whether it found one.

The form for a reader that may reach the end: `false` *is* the end, and there
is no exception to carry.

```
export fn get(this &File, value &char) bool except {
    code int = this._next_code()??;
    if (code < 0) {
        return false;
    }
    value = code as char;
    return true;
}
```

### `export fn peek(this &File) char except {`

Looks at the next character without consuming it.

```
export fn peek(this &File) char except {
    if (this._pushed < 0) {
        this._pushed = this._next_code()??;
    }
    if (this._pushed < 0) {
        raise "EOFError": "the input ended where a character was expected";
    }
    return this._pushed as char;
}
```

### `export fn eof(this &File) bool except {`

Whether the next character is the end of the input.

The question is about the *next* character, so it is answered by looking for
one: C's `feof` says only that a read has already failed, which would answer
`false` for a stream whose end the reader had not reached yet. The character
found here is kept, so asking costs nothing and `get` still sees it.

```
export fn eof(this &File) bool except {
    if (this._pushed >= 0) {
        return false;
    }
    if (this._stream == nullptr) {
        return true;
    }
    this._pushed = this._next_code()??;
    return this._pushed < 0;
}
```

### `export fn getline(this &File, into &String, until char = '\n') bool except {`

Reads up to `until` into `into`, replacing what it held.

Returns `false` when there was nothing to read. What `into` held before is
cleared first: reading a line answers "what is the next line", not "what else
is on this one". The terminator is consumed when it is read, and is not part
of the text. It has a default, because a line is what a caller almost always
wants.

```
export fn getline(this &File, into &String, until char = '\n') bool except {
    into.clear();
    read_any bool = false;
    while (true) {
        code int = this._next_code()??;
        if (code < 0) {
            break;
        }
        read_any = true;
        if (code == until as int) {
            break;
        }
        into.push(code as char);
    }
    return read_any;
}
```

### `export fn write(this &File, text &const String) usize except {`

Writes a `String`, and returns how many bytes were written.

```
export fn write(this &File, text &const String) usize except {
    return this._write_bytes(text.data() as *const byte, text.length())??;
}
```

### `export fn write(this &File, text *const char) usize except {`

Writes a C string, and returns how many bytes were written.

The length is measured with `C.strlen`, because a `*const char` has no length
of its own: it is a C string, and the terminator is where it ends.

```
export fn write(this &File, text *const char) usize except {
    if (text == nullptr) {
        return 0;
    }
    return this._write_bytes(text as *const byte, C.strlen(text))??;
}
```

### `export fn write(this &File, buffer *const byte, count usize) usize except {`

Writes `count` bytes, and returns how many were written.

```
export fn write(this &File, buffer *const byte, count usize) usize except {
    return this._write_bytes(buffer, count)??;
}
```

### `export fn put(this &File, character char) usize except {`

Writes one character.

```
export fn put(this &File, character char) usize except {
    if (!this.valid()) {
        return 0;
    }
    written usize = C.fwrite(&character, 1, 1, this._stream);
    if (written == 0 && C.ferror(this._stream) != 0) {
        raise "IOError": describe("the stream stopped before the character was out");
    }
    return written;
}
```

### `export fn flush(this &File) void except {`

Asks the file to hand over what it holds.

```
export fn flush(this &File) void except {
    if (this._stream == nullptr) {
        return;
    }
    if (C.fflush(this._stream) != 0) {
        raise "IOError": describe("the stream could not be flushed");
    }
}
```

### `export fn seek(this &File, offset isize, origin SeekFrom) usize except {`

Moves the position, and returns where it now is.

A stream that cannot seek -- a terminal, a pipe -- raises, which is the
run-time form of "this stream is not a file on disk". A file opened for
reading and writing can use this to read back what it wrote.

```
export fn seek(this &File, offset isize, origin SeekFrom) usize except {
    if (this._stream == nullptr) {
        raise "IOError": describe("the file has no stream");
    }
    // A seek on a stream that still holds bytes would drop them, and C says a
    // seek between writing and reading must be preceded by a flush; this is the
    // one line that makes the next operation see what the last wrote.
    if (C.fflush(this._stream) != 0) {
        raise "IOError": describe("the stream could not be flushed");
    }
    whence int = 0;
    if (origin == SeekFrom.CURRENT) {
        whence = 1;
    } else {
        if (origin == SeekFrom.END) {
            whence = 2;
        }
    }
    if (C.fseek(this._stream, offset, whence) != 0) {
        raise "IOError": describe("the stream cannot seek there");
    }
    place isize = C.ftell(this._stream);
    if (place < 0) {
        raise "IOError": describe("the stream cannot say where it is");
    }
    return place as usize;
}
```

### `export fn close(this &File) void except {`

Closes the file.

The standard streams are the program's to use, not to close: closing them is
done at exit, so `close` on one forgets it and leaves it to the C library.

```
export fn close(this &File) void except {
    if (this._stream != nullptr && this._stream != C.stdout as *byte
        && this._stream != C.stderr as *byte && this._stream != C.stdin as *byte)
    {
        if (C.fclose(this._stream) != 0) {
            this._stream = nullptr;
            this._pushed = -1;
            raise "IOError": describe("the stream could not be closed");
        }
    }
    this._stream = nullptr;
    this._pushed = -1;
}
```

### `export fn __init__(this &File, other &&File) void {`

Takes another handle's stream.

A `File` is move-only: two handles for one stream would close it twice. A
move leaves the source invalid, which is what makes `std.out = File(..)` and a
returned `File` one handle rather than two.

```
export fn __init__(this &File, other &&File) void {
    this._stream = other._stream;
    this._pushed = other._pushed;
    this._mode = other._mode;
    other._stream = nullptr;
    other._pushed = -1;
}
```

### `export fn __drop__(this &File) void {`

Closes the stream this handle owns.

The standard streams are not closed: they belong to the process, and a
`File` variable going out of scope is not a request to end output. `close`
follows the same rule.

```
export fn __drop__(this &File) void {
    if (this._stream != nullptr && this._stream != C.stdout as *byte
        && this._stream != C.stderr as *byte && this._stream != C.stdin as *byte)
    {
        C.fclose(this._stream);
    }
    this._stream = nullptr;
    this._pushed = -1;
}
```

### `fn print(this &File, values ...Ts) void except`

Writes its arguments, with `{}` in the first one standing for the rest.

`std.out.print("{} + {} = {}", 1, 2, 3)` names the shape of the line, and each
value is rendered by its `__format__` when it has one, by `str` otherwise. A
first argument with no `{}` is written as it stands and the rest after it,
which is the older form: `print("x = ", x, '\n')` still means what it did.

One variadic template, not one function per argument count: the arguments are
rendered into a small array by a pack expansion, and the format rule decides
how they are written.
- `values`: what to write; a first argument holding `{}` is the format

```
export
template <Ts ...type>
fn print(this &File, values ...Ts) void except
```

### `fn println(this &File, values ...Ts) void except`

Writes its arguments, then a newline.

The line is the unit a terminal and a log file are read in, so it has a name
of its own rather than being `print(..)` and a `'\n'` at every call.
- `values`: what to write, as `print` takes them

```
export
template <Ts ...type>
fn println(this &File, values ...Ts) void except
```

### `fn input(this &File, values ...&Ts) void except`

Reads one value per reference argument, skipping the whitespace before each.

`std.in.input(x, y)` reads the next two words and stores them in `x` and `y`.
The arguments are a pack of references: an element is the caller's own
variable, so what is read is written there. Anything that does not parse
raises, because there is no value to store and a zero would be a lie.
- `values`: the caller's variables, one per word to read

```
export
template <Ts ...type>
fn input(this &File, values ...&Ts) void except
```

### `fn read_value(this &File, value &T) void except`

Reads one value, skipping the whitespace before it.
- `value`: the variable the word is stored in

```
template <T type>
fn read_value(this &File, value &T) void except
```

### `fn read_token(this &File, into &String) bool except`

Reads the next word into `into`, skipping the whitespace around it.

A word is what a formatted read has to work with: numbers, characters and
strings are all written as runs of non-space characters, so one reader serves
them all and the type's own `__input__` decides what the word means.
- `into`: the string the word is written to; cleared first

```
fn read_token(this &File, into &String) bool except
```

### `fn _next_code(this &File) int except`

Reads one character as the integer a C stream gives, which is -1 at the end.

`get`, `peek`, `eof`, `getline` and the token reader differ in what they do
with the character; this is the reading they share, kept private so that the
pushed-back character is handled in one place.

```
fn _next_code(this &File) int except
```

### `fn _read_bytes(this &File, buffer *byte, count usize) usize except`

Reads `count` bytes, after giving back the character `peek` read ahead.
- `buffer`: where the bytes are written
- `count`: how many bytes are wanted at most

```
fn _read_bytes(this &File, buffer *byte, count usize) usize except
```

### `fn _write_bytes(this &File, buffer *const byte, count usize) usize except`

Writes `count` bytes, and reports a stream that stopped early.
- `buffer`: the bytes to write
- `count`: how many of them

```
fn _write_bytes(this &File, buffer *const byte, count usize) usize except
```

### `fn describe(message &const String) String`

The message a raised I/O error carries.

When the system has an error number for what went wrong, the number and its
text are part of the message: "the stream could not be closed" is a guess,
"the stream could not be closed (9: Bad file descriptor)" is the answer.
- `message`: what the operation was doing

```
fn describe(message &const String) String
```

### `export fn __input__(this &int, token &const String) void except {`

----------------------------------------------------------- formatted reading
Reads a word as an `int`.

```
export fn __input__(this &int, token &const String) void except {
    this = token as int??;
}
```

### `export fn __input__(this &usize, token &const String) void except {`

Reads a word as a `usize`.

```
export fn __input__(this &usize, token &const String) void except {
    this = token as usize??;
}
```

### `export fn __input__(this &char, token &const String) void except {`

Reads a word as a `char`, taking its first character.

```
export fn __input__(this &char, token &const String) void except {
    if (token.length() == 0) {
        raise "ValueError": "no character to read";
    }
    this = token.data()[0];
}
```

### `export fn __input__(this &String, token &const String) void except {`

Reads a word as a `String`.

```
export fn __input__(this &String, token &const String) void except {
    this = token;
}
```

### `export fn __input__(this &f64, token &const String) void except {`

Reads a word as a real number.

```
export fn __input__(this &f64, token &const String) void except {
    this = token as f64??;
}
```

### `export out File;`

---------------------------------------------------------------- the standard streams
The standard output file.

```
export out File;
```

### `export err File;`

The standard error file.

```
export err File;
```

### `export in File;`

The standard input file.

```
export in File;
```

### `fn __setup__() void`

Connects the three standard files.

The C streams are not compile-time constants and a module-level variable's
initial value has to be, so this is what `__setup__` is for: it runs before
anything else, dependencies first.

```
fn __setup__() void
```

### `export from std.fmt import Formattable;`

------------------------------------------------------------- the short forms
The format rule itself lives in `std.fmt`: `print` writes what that module
renders, so the two spellings of the rule cannot drift apart.

```
export from std.fmt import Formattable;
```

### `fn print(values ...Ts) void except`

Writes its arguments to the standard output, with the format rule of
`std.out.print(..)`.

It is the member call under a shorter name: a program that writes to the
terminal does not have to name the file it writes to.
- `values`: what to write, as `print` takes them

```
export
template <Ts ...type>
fn print(values ...Ts) void except
```

### `fn println(values ...Ts) void except`

Writes its arguments to the standard output, then a newline.
- `values`: what to write, as `print` takes them

```
export
template <Ts ...type>
fn println(values ...Ts) void except
```

### `export fn input() String except {`

Reads one line from the standard input, without its terminator.

It is the value form of `std.in.getline(..)`: the whole line is returned, and
the end of the input raises `EOFError` because there is no line left to
return. A blank line returns an empty `String` rather than nothing, which is
the difference between "the line was empty" and "there was no line".

```
export fn input() String except {
    line String;
    if (!in.getline(&line)??) {
        raise "EOFError": "the input ended where a line was expected";
    }
    return line;
}
```
