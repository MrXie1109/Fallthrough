//! I/O: one file type for everything -- the standard streams, files on disk, and
//! a C stream a program brings with it.
//!
//! A `File` is a C stream and the direction it was opened for. Nothing is
//! decided by the type: `readable()`/`writable()` are asked at run time, and
//! operating on the wrong side of a handle is a run-time error, exactly as it is
//! for a terminal. `print`/`println` and `input` are the formatted members, and
//! their names say which direction they go.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-io-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run_with_input(&self, label: &str, source: &str, input: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let mut child = std::process::Command::new(&executable)
            .current_dir(&self.directory)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the program can be run");
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(input.as_bytes())
            .expect("the input can be written");
        let output = child.wait_with_output().expect("the program finishes");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .current_dir(&self.directory)
            .output()
            .expect("the program can be run");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn the_standard_output_writes_text_and_characters() {
    // A line is a write and a `put`, which is all a line ever was; `flush` asks
    // the file to hand over what it holds.
    let source = r#"import std;

fn main() int except {
    std.out.write("first")??;
    std.out.put('\n')??;
    std.out.write("second")??;
    std.out.put('\n')??;
    std.out.flush()??;
    return 0;
}
"#;
    let scratch = Scratch::new("out");
    assert_eq!(scratch.run("out", source), "first\nsecond\n");
}

#[test]
fn a_file_is_written_and_read_back() {
    // The same two lines are written through one file and read back through
    // another, so what crosses is the file and not a special-purpose pair of
    // functions. Both are `File`; the mode decided their direction.
    let source = r#"import std;

fn main() int except {
    sink std.File = std.File("notes.txt", std.OpenMode.WRITE)??;
    if (!sink.valid() || !sink.writable() || sink.readable()) {
        return 1;
    }
    sink.write("alpha")??;
    sink.put('\n')??;
    sink.write("beta\n")??;
    sink.close()??;

    src std.File = std.File("notes.txt", std.OpenMode.READ)??;
    if (!src.valid() || !src.readable() || src.writable()) {
        return 2;
    }
    line std.String = std.String();
    while (src.getline(line)??) {
        std.out.write(line)??;
        std.out.put('\n')??;
    }
    src.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("file");
    assert_eq!(scratch.run("file", source), "alpha\nbeta\n");
}

#[test]
fn a_file_is_created_without_a_stream_and_opened_later() {
    // The default constructor makes an invalid file: nothing about it is an
    // error until it is used, and `valid` is how a program asks. `open` is the
    // member that gives it a stream, and the mode says which way it goes.
    let source = r#"import std;

fn main() int except {
    sink std.File = std.File();
    if (sink.valid()) {
        std.out.write("valid too early")??;
    } else {
        std.out.write("invalid")??;
    }
    std.out.put('\n')??;
    sink.open("made.txt", std.OpenMode.WRITE)??;
    if (sink.valid() && sink.writable()) {
        std.out.write("opened")??;
    }
    std.out.put('\n')??;
    sink.write("body")??;
    sink.close()??;
    if (sink.valid()) {
        std.out.write("still valid")??;
    } else {
        std.out.write("closed")??;
    }
    std.out.put('\n')??;
    return 0;
}
"#;
    let scratch = Scratch::new("invalid");
    assert_eq!(scratch.run("invalid", source), "invalid\nopened\nclosed\n");
}

#[test]
fn a_file_that_cannot_be_opened_raises() {
    // A file constructed from a path either opens the file or raises: a program
    // that wants to decide for itself keeps an invalid file and calls `open`,
    // whose failure it can catch.
    let source = r#"import std;

fn missing() int except {
    src std.File = std.File("nothing-here.txt", std.OpenMode.READ)??;
    return 1;
}

fn main() int except {
    try {
        marker int = missing()??;
        std.out.write(str(marker))??;
    } except {
        e &const std.Exception = std.exception();
        std.out.write(e.name())??;
        std.out.put('\n')??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("missing");
    assert_eq!(scratch.run("missing", source), "IOError\n");
}

#[test]
fn an_input_file_reads_characters_and_the_end() {
    // `read` fills a buffer, `get` and `peek` read one character, and `eof`
    // answers whether the input is finished. At the end `get` raises: a `char`
    // has no value left over to mean "nothing", so the end is an error rather
    // than a sentinel, and `eof` is how a program asks beforehand.
    let source = r#"import std;

fn main() int except {
    sink std.File = std.File("data.txt", std.OpenMode.WRITE)??;
    sink.write("xyz")??;
    sink.close()??;

    src std.File = std.File("data.txt", std.OpenMode.READ)??;
    buffer [4]char;
    got usize = src.read(&buffer[0], 2)??;
    std.out.write(str(got))??;
    std.out.put('\n')??;
    first char = src.peek()??;
    second char = src.get()??;
    std.out.put(first)??;
    std.out.put(second)??;
    std.out.put('\n')??;
    std.out.write(str(src.eof()))??;
    std.out.put('\n')??;
    try {
        past char = src.get()??;
        std.out.write(str(past))??;
    } except {
        std.out.write(std.exception().name())??;
        std.out.put('\n')??;
    }
    src.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("bytes");
    assert_eq!(scratch.run("bytes", source), "2\nzz\ntrue\nEOFError\n");
}

#[test]
fn a_duplex_file_seeks_back_to_what_it_wrote() {
    // One handle, both directions: `OpenMode.RW` opens a file for reading and
    // writing, and `seek` is what makes the two halves one file moved forwards
    // and backwards.
    let source = r#"import std;

fn main() int except {
    make std.File = std.File("both.txt", std.OpenMode.WRITE)??;
    make.write("first line\nsecond line\n")??;
    make.close()??;

    both std.File = std.File("both.txt", std.OpenMode.RW)??;
    if (!both.readable() || !both.writable()) {
        return 1;
    }
    both.write("FIRST")??;
    both.seek(0isize, std.SeekFrom.START)??;
    line String;
    both.getline(&line)??;
    std.out.print("read: ", line, "\n")??;
    both.seek(11isize, std.SeekFrom.START)??;
    rest String;
    both.getline(&rest)??;
    std.out.print("again: ", rest, "\n")??;
    both.seek(16isize, std.SeekFrom.START)??;
    tail String;
    both.getline(&tail)??;
    std.out.print("tail: ", tail, "\n")??;
    std.out.print("end: ", both.seek(0isize, std.SeekFrom.END)??, "\n")??;
    both.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("duplex");
    assert_eq!(
        scratch.run("duplex", source),
        "read: FIRST line\nagain: second line\ntail: d line\nend: 23\n"
    );
}

#[test]
fn the_standard_streams_are_files_like_any_other() {
    // A C stream is a `File` too: `std.out` is nothing more than
    // `File(C.stdout, OpenMode.WRITE)`, and a program can wrap a stream of its
    // own the same way.
    let source = r#"import std;

fn main() int except {
    log std.File = std.File(C.stdout, std.OpenMode.WRITE);
    log.print("wrapped\n")??;
    std.out.print("direct ", 42, "\n")??;
    std.out.write(str(std.out.valid()))??;
    std.out.put('\n')??;
    std.out.write(str(std.out.readable()))??;
    std.out.put('\n')??;
    std.out.write(str(std.out.writable()))??;
    std.out.put('\n')??;
    return 0;
}
"#;
    let scratch = Scratch::new("stdio");
    assert_eq!(
        scratch.run("stdio", source),
        "wrapped\ndirect 42\ntrue\nfalse\ntrue\n"
    );
}

#[test]
fn the_direction_of_a_handle_is_asked_at_run_time() {
    // Operating on the wrong side of a handle is a run-time error, not a
    // compile error: a read-only file answers `writable() == false`, and a
    // write through it raises rather than silently writing nowhere.
    let source = r#"import std;

fn main() int except {
    sink std.File = std.File("ro.txt", std.OpenMode.WRITE)??;
    sink.write("text")??;
    sink.close()??;

    src std.File = std.File("ro.txt", std.OpenMode.READ)??;
    if (!src.writable()) {
        std.out.write("not writable")??;
        std.out.put('\n')??;
    }
    try {
        written usize = src.write("more")??;
        std.out.write(str(written))??;
    } except {
        std.out.write(std.exception().name())??;
        std.out.put('\n')??;
    }
    src.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("direction");
    assert_eq!(scratch.run("direction", source), "not writable\nIOError\n");
}

#[test]
fn the_output_file_formats_its_arguments() {
    // `std.out.print(..)` renders each argument with `str` and writes it: text is
    // text, a number is its digits, a character is itself.
    let source = r#"import std;

fn main() int except {
    x int = 42;
    std.out.print("x = ", x, "\n")??;
    std.out.print('a', 'b', 'c', "\n")??;
    std.out.print(1, " and ", 2, " and ", 3, "\n")??;

    f std.File = std.File("format.txt", std.OpenMode.WRITE)??;
    f.print("file: ", x, '\n')??;
    f.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("format");
    assert_eq!(
        scratch.run("format", source),
        "x = 42\nabc\n1 and 2 and 3\n"
    );
}

#[test]
fn the_input_file_reads_values_into_their_variables() {
    // `std.in.input(..)` reads one word per value and stores it as the variable's
    // type: a number takes its digits, a character its first character, and a
    // `String` the whole word. The same call writes on a writable file.
    let source = r#"import std;

fn main() int except {
    count int = 0;
    word String = String();
    letter char = '?';
    ratio f64 = 0.0;
    std.in.input(count, word, letter, ratio)??;
    std.out.print(count, " ", word, " ", letter, " ", ratio, "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("input");
    assert_eq!(
        scratch.run_with_input("input", source, "42 hello z 2.5\n"),
        "42 hello z 2.5\n"
    );
}

#[test]
fn a_formatted_read_can_fill_a_type_of_the_program() {
    // A type with `__input__` is readable, and one with `__str__` is writable.
    // The formatted call is the same on both sides of the same handle.
    let source = r#"import std;

type Version {
    major int;
    minor int;
}

fn __init__(this &Version) void {
    this.major = 0;
    this.minor = 0;
}

fn __str__(this &const Version) String {
    text String = str(this.major);
    text.push('.');
    text.push(str(this.minor));
    return text;
}

fn __input__(this &Version, token &const String) void except {
    separator String = ".";
    dot isize = token.find(separator)??;
    if (dot < 0) {
        raise "ValueError": "a version is `major.minor`";
    }
    head String = token.substring(0, dot as usize);
    tail String = token.substring(dot as usize + 1, token.length() - dot as usize - 1);
    this.major = head as int??;
    this.minor = tail as int??;
}

fn main() int except {
    source std.File = std.File("version.txt", std.OpenMode.WRITE)??;
    source.write("2.5\n")??;
    source.close()??;

    f std.File = std.File("version.txt", std.OpenMode.READ)??;
    v Version;
    f.input(v)??;
    std.out.print("major ", v.major, ", minor ", v.minor, "\n")??;
    f.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("version");
    assert_eq!(scratch.run("version", source), "major 2, minor 5\n");
}

#[test]
fn a_type_that_cannot_be_read_is_refused_when_it_is_read() {
    // `print` needs `str`, `input` needs `__input__`, and the method name says
    // which: a type with only `__str__` can be printed and cannot be read, and
    // that is a compile error at the `input` call rather than a run-time one.
    let source = r#"import std;

type Tag {
    n int;
}

fn __init__(this &Tag) void {
    this.n = 7;
}

fn __str__(this &const Tag) String {
    return String("tag");
}

fn main() int except {
    tag Tag;
    std.out.print("value: ", tag, "\n")??;
    source std.File = std.File("tag.txt", std.OpenMode.WRITE)??;
    source.write("99\n")??;
    source.close()??;
    f std.File = std.File("tag.txt", std.OpenMode.READ)??;
    f.input(tag)??;
    f.close()??;
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("__input__"), "{diagnostics}");
}

#[test]
fn println_writes_a_newline_after_its_arguments() {
    // `println` is `print` with the newline the unit a terminal and a log file
    // are read in; with no arguments it writes just the newline.
    let source = r#"import std;

fn main() int except {
    std.out.println("line")??;
    std.out.println(1, " ", 2)??;
    std.out.println()??;
    std.out.print("end")??;
    return 0;
}
"#;
    let scratch = Scratch::new("println");
    assert_eq!(scratch.run("println", source), "line\n1 2\n\nend");
}

#[test]
fn the_formatted_members_respect_the_direction() {
    // `print` writes and `input` reads, and using one on the wrong side of a
    // handle is a run-time error with the file's own name on it.
    let source = r#"import std;

fn main() int except {
    sink std.File = std.File("d.txt", std.OpenMode.WRITE)??;
    sink.write("text")??;
    sink.close()??;

    read_only std.File = std.File("d.txt", std.OpenMode.READ)??;
    try {
        read_only.print("more")??;
        std.out.print("printed\n")??;
    } except {
        std.out.print(std.exception().name())??;
        std.out.print("\n")??;
    }
    read_only.close()??;

    write_only std.File = std.File("d.txt", std.OpenMode.WRITE)??;
    n int = 0;
    try {
        write_only.input(n)??;
        std.out.print("read ", n, "\n")??;
    } except {
        std.out.print(std.exception().name())??;
        std.out.print("\n")??;
    }
    write_only.close()??;
    return 0;
}
"#;
    let scratch = Scratch::new("formatted-direction");
    assert_eq!(
        scratch.run("formatted-direction", source),
        "IOError\nIOError\n"
    );
}

#[test]
fn the_short_forms_print_and_input_use_the_standard_streams() {
    // `std.print(..)` is `std.out.print(..)` under a shorter name, and
    // `std.input()` returns the next line of `std.in` -- a value, or an
    // `EOFError` when there is no line left. A blank line is an empty `String`,
    // which is a line that was read.
    let source = r#"import std;

fn main() int except {
    std.print("x = ", 42, "\n")??;
    std.print("plain line\n")??;
    first String = std.input()??;
    std.print("got: ", first, "\n")??;
    blank String = std.input()??;
    std.print("blank length: ", blank.length(), "\n")??;
    try {
        last String = std.input()??;
        std.print("unexpected: ", last, "\n")??;
    } except {
        std.print(std.exception().name(), "\n")??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("short");
    assert_eq!(
        scratch.run_with_input("short", source, "hello\n\n"),
        "x = 42\nplain line\ngot: hello\nblank length: 0\nEOFError\n"
    );
}

#[test]
fn print_fills_a_format_string_and_asks_format_for_a_value() {
    // `{}` is filled from the arguments in order; a value with `__format__` is
    // asked for its own rendering, and a value without one falls back to `str`.
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn __str__(this &const Point) String {
    text String = String("(");
    text.push(str(this.x));
    text.push(", ");
    text.push(str(this.y));
    text.push(")");
    return text;
}

fn __format__(this &const Point) String {
    return String("point");
}

fn main() int except {
    std.print("{} + {} = {}\n", 1, 2, 3)??;
    p Point = .{ 1, 2 };
    std.print("at {}\n", p)??;
    std.print("fallback {}\n", 7)??;
    std.print("x = ", 5, "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("print-format");
    assert_eq!(
        scratch.run("print-format", source),
        "1 + 2 = 3\nat point\nfallback 7\nx = 5\n"
    );
}

#[test]
fn an_io_error_carries_the_system_error_number() {
    // "the file could not be opened" is a guess; the number and the system's
    // text are the answer, and they are part of the message.
    let source = r#"import std;
import C;

fn main() int {
    try {
        f std.File = std.File("/definitely/missing-file", std.OpenMode.READ)??;
    } except {
        e &const std.Exception = std.exception();
        C.printf("%s\n", e.name().data());
        C.printf("%s\n", e.what().data());
        C.printf("%d\n", std.last_error() as int);
        C.printf("%s\n", std.error_text(2));
    }
    return 0;
}
"#;
    let scratch = Scratch::new("io-errno");
    let output = scratch.run("io-errno", source);
    assert!(output.contains("IOError"), "{output}");
    assert!(
        output.contains("(2: No such file or directory)"),
        "{output}"
    );
    assert_eq!(output.lines().nth(2), Some("2"), "{output}");
}

#[test]
fn print_takes_a_pack_not_a_fixed_number_of_arguments() {
    // One variadic template serves every arity: no argument-count ceiling.
    let source = r#"import std;

fn main() int except {
    std.print(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, "\n")??;
    std.print("{} {} {} {} {} {}\n", 1, 2, 3, 4, 5, 6)??;
    return 0;
}
"#;
    let scratch = Scratch::new("variadic");
    assert_eq!(
        scratch.run("variadic", source),
        "12345678910\n1 2 3 4 5 6\n"
    );
}
