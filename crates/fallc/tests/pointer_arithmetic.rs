//! Pointer arithmetic: the offset is in elements, scaled by the pointee's size.
//!
//! A pointer is an address, and the only arithmetic an address has is C's: add
//! or subtract a count of *elements*, and subtract two pointers to get the
//! distance between them. The scaling is the compiler's business, and it is
//! what makes `p + 1` skip one whole `Pair` rather than one byte.
//!
//! The offset is a `usize`, like an index, and the difference is an `isize`,
//! because it may be negative. Nothing here is checked: a pointer one past the
//! end is meaningful, and anything beyond it is undefined, exactly as in C.

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
            std::env::temp_dir().join(format!("fallc-ptr-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable and runs it.
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

    /// Compiles the source in one file and returns what the compiler printed.
    fn check(&self, _label: &str, source: &str) -> (Outcome, String) {
        compile_text("test.ft", source, &[EmitKind::Llvm])
    }

    /// Compiles with `-Wverbose`, so that the explanations are printed too.
    ///
    /// What the compiler *says* is the message; what to write instead is an
    /// explanation, and behind `-Wverbose` because a diagnostic is read far
    /// more often than a rule is looked up.
    fn check_verbose(&self, _label: &str, source: &str) -> (Outcome, String) {
        let mut warnings = fallc_diag::WarningPolicy::new();
        warnings.set_verbose(true);
        fallc::pipeline::compile_text_with_warnings("test.ft", source, &[EmitKind::Llvm], &warnings)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_pointer_is_advanced_by_elements() {
    let source = r#"import std;

fn main() int {
    a [4]int;
    a[0] = 10;
    a[1] = 20;
    a[2] = 30;
    a[3] = 40;
    p *int = &a[0];
    for (i int = 0; i < 4; i += 1) {
        C.printf("%d\n", p[0]);
        p = p + 1;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("elements");
    assert_eq!(scratch.run("elements", source), "10\n20\n30\n40\n");
}

#[test]
fn increment_and_decrement_move_one_element() {
    let source = r#"import std;

fn main() int {
    a [3]int;
    a[0] = 1;
    a[1] = 2;
    a[2] = 3;
    p *int = &a[0];
    ++p;
    C.printf("%d\n", p[0]);
    --p;
    C.printf("%d\n", p[0]);
    q *int = p++;
    C.printf("%d %d\n", q[0], p[0]);
    r *int = p--;
    C.printf("%d %d\n", r[0], p[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("incdec");
    assert_eq!(scratch.run("incdec", source), "2\n1\n1 2\n2 1\n");
}

#[test]
fn the_scaling_is_the_pointee_size() {
    // `Pair` is two `int`s, so one element is eight bytes: `p + 1` reaches the
    // second pair, and the distance between the two pointers is one element.
    let source = r#"import std;

type Pair {
    a int;
    b int;
}

fn main() int {
    pairs [3]Pair;
    pairs[0] = .{1, 2};
    pairs[1] = .{3, 4};
    pairs[2] = .{5, 6};
    p *Pair = &pairs[0];
    q *Pair = p + 2;
    C.printf("%d %d\n", p[1].a, p[1].b);
    C.printf("%d %d\n", q[0].a, q[0].b);
    C.printf("%lld\n", q - p);
    C.printf("%lld\n", p - q);
    return 0;
}
"#;
    let scratch = Scratch::new("scaling");
    assert_eq!(scratch.run("scaling", source), "3 4\n5 6\n2\n-2\n");
}

#[test]
fn a_byte_pointer_advances_one_byte_and_a_wide_one_by_its_width() {
    let source = r#"import std;

fn main() int {
    bytes [3]byte;
    bytes[0] = 1 as byte;
    bytes[1] = 2 as byte;
    bytes[2] = 3 as byte;
    b *byte = &bytes[0];
    C.printf("%d %d\n", b[1] as int, (b + 2)[0] as int);

    wide [3]i64;
    wide[0] = 100;
    wide[1] = 200;
    wide[2] = 300;
    w *i64 = &wide[0];
    C.printf("%lld %lld\n", w[1], (w + 2)[0]);
    C.printf("%lld\n", &wide[1] - w);
    return 0;
}
"#;
    let scratch = Scratch::new("bytes");
    assert_eq!(scratch.run("bytes", source), "2 3\n200 300\n1\n");
}

#[test]
fn a_pointer_is_offset_by_an_integer_on_either_side() {
    let source = r#"import std;

fn main() int {
    a [4]int;
    a[0] = 1;
    a[1] = 2;
    a[2] = 3;
    a[3] = 4;
    p *int = &a[0];
    q *int = 2 + p;
    C.printf("%d\n", q[0]);
    r *int = q - 1;
    C.printf("%d\n", r[0]);
    p += 3;
    C.printf("%d\n", p[0]);
    p -= 3;
    C.printf("%d\n", p[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("offset");
    assert_eq!(scratch.run("offset", source), "3\n2\n4\n1\n");
}

#[test]
fn a_pointer_may_be_ordered() {
    let source = r#"import std;

fn main() int {
    a [3]int;
    p *int = &a[0];
    q *int = &a[2];
    if (p < q) {
        C.printf("p before q\n");
    }
    if (q > p) {
        C.printf("q after p\n");
    }
    if (p <= p) {
        C.printf("p equals p\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("order");
    assert_eq!(
        scratch.run("order", source),
        "p before q\nq after p\np equals p\n"
    );
}

#[test]
fn a_qualified_pointer_may_be_advanced() {
    // The qualifiers survive: advancing a `*const T` yields a `*const T`.
    let source = r#"import std;

fn main() int {
    a [2]int;
    a[0] = 7;
    a[1] = 8;
    p *const int = &a[0];
    q *const int = p + 1;
    C.printf("%d\n", q[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("qualified");
    assert_eq!(scratch.run("qualified", source), "8\n");
}

#[test]
fn an_offset_of_another_integer_type_is_refused() {
    let source = "fn f(a [2]int) int {\n    p *int = &a[0];\n    n int = 1;\n    q *int = p + n;\n    return q[0];\n}\n";
    let scratch = Scratch::new("offset-type");
    let (outcome, output) = scratch.check_verbose("offset-type", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("offset by `usize`"), "{output}");
    assert!(output.contains("as usize"), "{output}");
}

#[test]
fn adding_two_pointers_is_refused() {
    let source =
        "fn f(a [2]int) int {\n    p *int = &a[0];\n    q *int = p + p;\n    return q[0];\n}\n";
    let scratch = Scratch::new("add-pointers");
    let (outcome, output) = scratch.check("add-pointers", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("cannot add two pointers"), "{output}");
}

#[test]
fn the_difference_of_different_pointee_types_is_refused() {
    let source = "fn f(a [2]int) int {\n    p *int = &a[0];\n    q *u8 = nullptr;\n    return (p - q) as int;\n}\n";
    let scratch = Scratch::new("difference");
    let (outcome, output) = scratch.check_verbose("difference", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("difference"), "{output}");
    assert!(output.contains("same type"), "{output}");
}
