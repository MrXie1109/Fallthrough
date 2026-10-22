//! Short strings live inside the `String` object.
//!
//! `String` is three words, and a string of up to `sizeof(String) - 2` bytes
//! keeps its text in those words: no allocation on creation, none on copy, none
//! on destruction. What a caller can observe is the capacity -- the room the
//! object itself provides -- and the address of `data()`, which for an inline
//! string is an address inside the string rather than on the heap.
//!
//! The tests run programs rather than inspect the compiler's records: the
//! address `data()` prints is the address the program really has, and the text
//! is read back through C, which is what the storage exists for.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-sso-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable and runs it.
    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let args = [
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ];
        let options = Options::parse(&args).expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// The value on a line beginning with `label`, without the label.
fn field(output: &str, label: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{output}"))
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("`{label}` names nothing in:\n{output}"))
        .to_owned()
}

#[test]
fn a_short_string_reports_the_room_inside_the_object() {
    // An empty string allocates nothing, so its capacity is the object's own
    // storage: `sizeof(String)` bytes, one of which is the marker and one the
    // terminator.
    let source = r#"import std;

fn main() int {
    s String;
    C.printf("cap %lu\n", s.capacity());
    C.printf("size %lu\n", sizeof(String));
    C.printf("len %lu\n", s.length());
    return 0;
}
"#;
    let scratch = Scratch::new("capacity");
    let output = scratch.run("capacity", source);
    assert_eq!(field(&output, "cap"), "23", "{output}");
    assert_eq!(field(&output, "size"), "24", "{output}");
    assert_eq!(field(&output, "len"), "0", "{output}");
}

#[test]
fn a_short_string_never_moves_its_bytes() {
    // Appending within the object's room does not reallocate, so the address of
    // the text cannot change. A heap string would print a different address as
    // soon as it grew.
    let source = r#"import std;

fn main() int {
    s String = "a";
    C.printf("start %p\n", s.data());
    s.push('b');
    s.push('c');
    s.push('d');
    C.printf("after %p\n", s.data());
    C.printf("text [%s] len %lu cap %lu\n", s.data(), s.length(), s.capacity());
    return 0;
}
"#;
    let scratch = Scratch::new("stable");
    let output = scratch.run("stable", source);
    assert_eq!(
        field(&output, "start"),
        field(&output, "after"),
        "an inline string's bytes moved:\n{output}"
    );
    assert!(output.contains("text [abcd] len 4 cap 23"), "{output}");
}

#[test]
fn growing_past_the_object_moves_the_text_to_the_heap() {
    // The boundary is the object's room: one more byte than it holds is what
    // makes the string allocate, and the address of the text changes with it.
    let source = r#"import std;

fn main() int {
    s String = "0123456789012345678901";
    C.printf("inline %p cap %lu len %lu\n", s.data(), s.capacity(), s.length());
    s.push('x');
    C.printf("heap %p cap %lu len %lu\n", s.data(), s.capacity(), s.length());
    C.printf("text [%s]\n", s.data());
    return 0;
}
"#;
    let scratch = Scratch::new("grow");
    let output = scratch.run("grow", source);
    assert_eq!(field(&output, "inline"), "0x0".replace("0x0", &field(&output, "inline")), "{output}");
    assert!(output.contains("inline") && output.contains("cap 23 len 22"), "{output}");
    // A heap string's capacity is larger than the text, and its text is intact.
    assert!(output.contains("text [0123456789012345678901x]"), "{output}");
    assert!(
        !window(&output, "heap").contains("cap 23 "),
        "the string did not allocate when it outgrew the object:\n{output}"
    );
}

/// Returns the rest of the line beginning with `label`.
fn window<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{output}"))
}

#[test]
fn a_shrunk_string_returns_to_the_object() {
    // `shrink_to_fit` gives the bytes back as well as the room: a text short
    // enough to live inside the object moves back into it.
    let source = r#"import std;

fn main() int {
    s String = "long enough that the text needs a heap buffer";
    C.printf("before cap %lu\n", s.capacity());
    s.clear();
    s.push('a');
    s.shrink_to_fit();
    C.printf("after cap %lu text [%s]\n", s.capacity(), s.data());
    s.push('b');
    C.printf("grown [%s]\n", s.data());
    return 0;
}
"#;
    let scratch = Scratch::new("shrink");
    let output = scratch.run("shrink", source);
    assert!(output.contains("after cap 23 text [a]"), "{output}");
    assert!(output.contains("grown [ab]"), "{output}");
}

#[test]
fn copying_a_short_string_gives_each_its_own_bytes() {
    // Copying an inline string copies the object's bytes, so the two strings
    // have different addresses and neither shares the other's storage.
    let source = r#"import std;

fn main() int {
    a String = "tiny";
    b String = a;
    C.printf("distinct %d\n", a.data() != b.data());
    C.printf("a [%s] b [%s]\n", a.data(), b.data());
    b.push('!');
    C.printf("after a [%s] b [%s]\n", a.data(), b.data());
    return 0;
}
"#;
    let scratch = Scratch::new("copy");
    let output = scratch.run("copy", source);
    assert!(output.contains("distinct 1"), "{output}");
    assert!(output.contains("after a [tiny] b [tiny!]"), "{output}");
}

#[test]
fn a_binding_with_no_initialiser_can_be_grown() {
    // `s String;` is zero-initialised, and an all-zero object is a valid empty
    // string. Growing one used to write the first character over `_data`, which
    // destroyed the all-zero state *before* the inline marker was written -- so
    // the string decided it owned a buffer at the address of its own first
    // byte, and the terminator store crashed.
    let source = r#"import std;

fn main() int {
    s String;
    s.push('a');
    s.push('b');
    C.printf("[%s] len %lu\n", s.data(), s.length());
    return 0;
}
"#;
    let scratch = Scratch::new("uninit-grow");
    let output = scratch.run("uninit_grow", source);
    assert!(output.contains("[ab] len 2"), "{output}");
}

#[test]
fn a_number_can_be_rendered_into_a_string() {
    // `str` on a number goes through the runtime renderer and then copies the
    // text into a `String` with the C-string constructor. That constructor was
    // looked up as taking `*String` while the library declares `&String`, so
    // the symbol named a function that does not exist.
    let source = r#"import std;

fn main() int {
    text String = str(42);
    text.push('!');
    C.printf("[%s]\n", text.data());
    C.printf("[%s]\n", str(-7).data());
    return 0;
}
"#;
    let scratch = Scratch::new("render");
    let output = scratch.run("render", source);
    assert!(output.contains("[42!]"), "{output}");
    assert!(output.contains("[-7]"), "{output}");
}

#[test]
fn every_kind_of_value_is_rendered_as_what_it_means() {
    // The renderer is chosen by the value's meaning, not by its machine type.
    // It used to be one entry point taking an `int`: a `bool` was passed as an
    // `i1`, a `i64` as an `i64`, a `float` as an `f32`, and each of those was IR
    // that does not verify -- or, for an unsigned value, a negative number.
    let source = r#"import std;

fn main() int {
    C.printf("%s %s %s\n", str(true).data(), str(false).data(), str(7).data());
    big i64 = 9000000000;
    ubig u64 = 9000000000;
    small i8 = -5i8;
    C.printf("%s %s %s\n", str(big).data(), str(ubig).data(), str(small).data());
    f f32 = 1.5f;
    C.printf("%s %s\n", str(f).data(), str('A').data());
    C.printf("%s %s\n", repr("hi").data(), repr('y').data());
    return 0;
}
"#;
    let scratch = Scratch::new("render-kinds");
    let output = scratch.run("render_kinds", source);
    assert!(output.contains("true false 7"), "{output}");
    assert!(output.contains("9000000000 9000000000 -5"), "{output}");
    assert!(output.contains("1.5 A"), "{output}");
    assert!(output.contains("\"hi\" 'y'"), "{output}");
}

#[test]
fn a_type_that_cannot_be_written_down_says_so() {
    // A declared type with no `__str__` is the compiler's limit, not a mistake
    // it can render around: the diagnostic names the declaration to add instead
    // of failing in code generation.
    let source = "type P {\n    v int;\n}\n\nfn main() int {\n    p P = .{3};\n    text String = str(p);\n    return 0;\n}\n";
    let (outcome, printed) = fallc::pipeline::compile_text("render.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(printed.contains("S0182"), "{printed}");
    assert!(printed.contains("__str__"), "{printed}");
}

#[test]
fn the_text_can_be_searched_split_and_trimmed() {
    // The queries a caller needs before writing a loop by hand: where a
    // substring is, whether the text starts or ends with one, the bytes of a
    // range, and the pieces between separators.
    let source = r#"import std;

fn main() int {
    text String = "  hello world  ";
    C.printf("find %ld %ld\n", text.find(String("world")), text.find(String("zzz")));
    C.printf("contains %d %d\n", text.contains(String("hello")) as int, text.contains(String("bye")) as int);
    C.printf("starts %d ends %d\n", text.starts_with(String("  he")) as int, text.ends_with(String("  ")) as int);
    C.printf("trimmed [%s]\n", text.trimmed().data());
    C.printf("sub [%s] [%s]\n", text.substring(2, 5).data(), text.substring(20, 3).data());
    parts Array<String>;
    String("a,b,,c").split(',', parts);
    C.printf("split %d:", parts.length());
    for (piece String : parts) {
        C.printf(" [%s]", piece.data());
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("queries");
    assert_eq!(
        scratch.run("queries", source),
        "find 8 -1\ncontains 1 0\nstarts 1 ends 1\ntrimmed [hello world]\n\
         sub [hello] []\nsplit 4: [a] [b] [] [c]\n"
    );
}

#[test]
fn text_converts_to_a_number_or_it_raises() {
    // The whole text has to be the number, and a value that does not fit is
    // refused instead of wrapping. A conversion that fails raises `ValueError`,
    // so asking "is this a number?" is a `try`/`except` away.
    let source = r#"import std;

fn attempt(label *const char, text &const String) void except {
    try {
        C.printf("%s %d\n", label, text as int?!);
    } except {
        C.printf("%s bad\n", label);
    }
}

fn main() int except {
    attempt("twelve", String("12"))?!;
    attempt("negative", String("-7"))?!;
    attempt("junk", String("12x"))?!;
    attempt("empty", String(""))?!;
    attempt("overflow", String("99999999999"))?!;
    C.printf("size %lu\n", String("42") as usize?!);
    C.printf("real %.1f\n", String("2.5e2") as f64?!);
    try {
        C.printf("real junk %f\n", String("2.5x") as f64?!);
    } except {
        C.printf("real junk bad\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("conversions");
    assert_eq!(
        scratch.run("conversions", source),
        "twelve 12\nnegative -7\njunk bad\nempty bad\noverflow bad\n\
         size 42\nreal 250.0\nreal junk bad\n"
    );
}

#[test]
fn a_reference_renders_as_its_referent() {
    // `str(r)` for `r: &T` is the text of the `T`, not the address of the
    // reference: the same string, whether it is printed from the variable, from
    // a value passed by reference, or from `str(..)` directly.
    let source = r#"import std;

fn show(text &const String) void except {
    std.out.print("ref ", text, "\n")?!;
}

fn main() int except {
    text String = "abc";
    std.out.print("value ", text, "\n")?!;
    std.out.print("str ", str(text), "\n")?!;
    show(text)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("str-ref");
    assert_eq!(
        scratch.run("str-ref", source),
        "value abc\nstr abc\nref abc\n"
    );
}

#[test]
fn text_converts_to_a_number_with_as() {
    // `String.__as__` is the conversion: the whole text must be the number, and
    // a text that is not one raises `ValueError`.
    let source = r#"import std;
import C;

fn main() int except {
    C.printf("%d %lu %f\n",
        String("-42") as int?!,
        String("100") as usize?!,
        String("2.5e2") as f64?!);
    try {
        C.printf("never %d\n", String("12x") as int?!);
    } except {
        e &const std.Exception = std.exception();
        C.printf("%s: %s\n", e.name().data(), e.what().data());
    }
    try {
        C.printf("never %lu\n", String("-1") as usize?!);
    } except {
        e &const std.Exception = std.exception();
        C.printf("%s\n", e.name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("as-conversions");
    assert_eq!(
        scratch.run("as-conversions", source),
        "-42 100 250.000000\nValueError: the text is not an integer\nValueError\n"
    );
}
