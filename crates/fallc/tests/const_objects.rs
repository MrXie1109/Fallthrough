//! What `const` reaches.
//!
//! `const` is shallow and it belongs to the object, not to the types inside it:
//! a `const` record may not be changed, but a pointer *field* of it may not be
//! reassigned while the pointee is still writable. A method that writes through
//! its receiver has to say `this &T`, and such a method may not be called on a
//! const object; a method that says `this &const T` may. A field marked
//! `mutable` is the one thing a const object may still change.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-const-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
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

/// The `Point` used by the tests below, plus the methods they call.
const POINT: &str = r#"import std;

type Point {
    x int;
    y int;
    tag int mutable;
}

fn shift(this &Point, d int) void {
    this.x += d;
}

fn sum(this &const Point) int {
    return this.x + this.y;
}
"#;

#[test]
fn a_writing_method_may_not_be_called_on_a_const_object() {
    let source = format!(
        "{POINT}
fn main() int {{
    p const Point = .{{1, 2, 0}};
    p.shift(2);
    return 0;
}}
"
    );
    let (outcome, diagnostics) = compile_text("t.ft", &source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("cannot be called on a `const` object"),
        "{diagnostics}"
    );
    // The suggestion is a note, and notes are shown only with `-Wverbose`.
    assert!(diagnostics.contains("`this`"), "{diagnostics}");
}

#[test]
fn a_reading_method_may_be_called_on_a_const_object() {
    let source = format!(
        "{POINT}
fn main() int {{
    p const Point = .{{1, 2, 0}};
    return p.sum();
}}
"
    );
    let scratch = Scratch::new("read");
    // The program returns 3, which the harness does not read; what matters is
    // that a `this &const T` method accepts a const receiver.
    let _ = scratch.run("read", &source);
}

#[test]
fn a_mutable_field_may_be_changed_on_a_const_object() {
    let source = format!(
        "{POINT}
fn main() int {{
    p const Point = .{{1, 2, 0}};
    p.tag += 1;
    C.printf(\"%d\\n\", p.tag);
    return 0;
}}
"
    );
    let scratch = Scratch::new("mutable");
    assert_eq!(scratch.run("mutable", source.as_str()), "1\n");
}

#[test]
fn an_ordinary_field_may_not_be_assigned_on_a_const_object() {
    let source = format!(
        "{POINT}
fn main() int {{
    p const Point = .{{1, 2, 0}};
    p.x = 3;
    return 0;
}}
"
    );
    let (outcome, diagnostics) = compile_text("t.ft", source.as_str(), &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("cannot assign to `x` of a `const` object"),
        "{diagnostics}"
    );
}

#[test]
fn const_is_shallow() {
    // The pointer field may not be reassigned -- it is part of the object -- but
    // what it points at is not part of the object, so writing through it is
    // allowed.
    let source = r#"import std;

type Holder {
    p *int;
    n int;
}

fn main() int {
    v int = 1;
    w int = 2;
    h const Holder = .{ &v, 7 };
    *h.p = 5;
    C.printf("%d\n", v);
    return 0;
}
"#;
    let scratch = Scratch::new("shallow");
    assert_eq!(scratch.run("shallow", source), "5\n");
}

#[test]
fn a_const_pointer_field_may_not_be_reassigned() {
    let source = r#"import std;

type Holder {
    p *int;
    n int;
}

fn main() int {
    v int = 1;
    w int = 2;
    h const Holder = .{ &v, 7 };
    h.p = &w;
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("cannot assign to `p` of a `const` object"),
        "{diagnostics}"
    );
}

#[test]
fn a_const_element_of_a_const_array_may_not_be_written() {
    let source =
        "fn main() int {\n    a const [3]int = .{1, 2, 3};\n    a[1] = 4;\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("const") || diagnostics.contains("assign"),
        "{diagnostics}"
    );
}
