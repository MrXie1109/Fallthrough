//! `auto`, and what a modifier around it means.
//!
//! A bare `auto` takes the initialiser's *value* type, so `x auto = *p` copies.
//! A modifier is matched against the initialiser one indirection at a time,
//! which is what makes `q *auto = &v` a `*int` and not a `**int`: the `*` in the
//! type pairs with the pointer the initialiser already has, and `auto` takes
//! what is left.
//!
//! The programs are compiled and run, because the difference between a copy and
//! an alias is only visible in what the program does to the original.

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
            std::env::temp_dir().join(format!("fallc-auto-{label}-{}", std::process::id()));
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_bare_auto_copies() {
    let source = r#"import std;

fn main() int {
    v int = 5;
    p *int = &v;
    x auto = *p;
    x = 3;
    C.printf("%d %d\n", x, v);
    return 0;
}
"#;
    let scratch = Scratch::new("copy");
    assert_eq!(scratch.run("copy", source), "3 5\n");
}

#[test]
fn a_reference_auto_binds_the_place() {
    let source = r#"import std;

fn main() int {
    v int = 5;
    p *int = &v;
    r &auto = *p;
    r = 9;
    C.printf("%d %d\n", r, v);
    return 0;
}
"#;
    let scratch = Scratch::new("reference");
    assert_eq!(scratch.run("reference", source), "9 9\n");
}

#[test]
fn a_pointer_auto_pairs_with_the_pointers_initialiser() {
    // `q *auto = &v` is `*int`, not `**int`: the written `*` and the
    // initialiser's pointer are the same indirection.
    let source = r#"import std;

fn main() int {
    v int = 5;
    q *auto = &v;
    C.printf("%d\n", q[0]);
    *q = 12;
    C.printf("%d\n", v);
    return 0;
}
"#;
    let scratch = Scratch::new("pointer");
    assert_eq!(scratch.run("pointer", source), "5\n12\n");
}

#[test]
fn a_const_auto_is_a_value_that_may_not_be_written() {
    let source = r#"import std;

fn main() int {
    n const auto = 7;
    C.printf("%d\n", n as int);
    return 0;
}
"#;
    let scratch = Scratch::new("const");
    assert_eq!(scratch.run("const", source), "7\n");
}

#[test]
fn an_auto_under_a_modifier_still_needs_an_initialiser() {
    let source = "fn f() int {\n    r &auto;\n    return 0;\n}\n";
    let scratch = Scratch::new("no-init");
    let (outcome, output) = scratch.check("no-init", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("needs an initialiser"), "{output}");
}

#[test]
fn a_modifier_around_auto_does_not_accept_a_designated_initialiser() {
    let source = "type P {\n    x int;\n}\n\nfn f() int {\n    p *auto = .{1};\n    return 0;\n}\n";
    let scratch = Scratch::new("designated");
    let (outcome, output) = scratch.check("designated", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("designated initialiser"), "{output}");
}

#[test]
fn a_reference_auto_does_not_turn_a_number_into_a_reference() {
    // The deduction fills the type, not the storage: `&auto` still needs a
    // place, and a number is not one.
    let source = "fn f() int {\n    r &auto = 5;\n    return 0;\n}\n";
    let scratch = Scratch::new("number");
    let (outcome, output) = scratch.check("number", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

#[test]
fn an_auto_binding_is_constructed_from_its_initialiser() {
    // `auto` deduces the *type*; it does not turn construction into a plain
    // store. `b auto = a;` stored `a`'s three words into `b`, so the two shared
    // one buffer and both destructors freed it -- a double free at the end of
    // the scope, with no copy constructor ever called.
    let source = r#"import std;
import C;

fn main() int {
    a String = "a long string that owns a buffer of its own for sure here";
    b auto = a;
    C.printf("%d\n", (a.data() == b.data()) as int);
    return 0;
}
"#;
    let output = Scratch::new("auto-copy").run("auto_copy", source);
    assert_eq!(output.trim(), "0", "the copy shares its source's buffer");
}

#[test]
fn an_auto_binding_from_a_move_is_a_move() {
    // The other half: `std.move(a)` says the value is dying, so the move
    // constructor is what `auto` reaches, and `a` is left empty.
    let source = r#"import std;
import C;

fn main() int {
    a String = "a long string that owns a buffer of its own for sure here";
    b auto = std.move(a);
    C.printf("[%s] %lu\n", a.data(), a.length());
    return 0;
}
"#;
    let output = Scratch::new("auto-move").run("auto_move", source);
    assert_eq!(output.trim(), "[] 0", "the source was not emptied");
}

#[test]
fn an_auto_binding_of_a_value_still_copies_it() {
    // `x auto = *p;` reads through the pointer. With no constructor to call --
    // the type is an `int` -- the read is a load, which is what it always was.
    let source = r#"import C;

fn main() int {
    n int = 7;
    p *int = &n;
    q auto = *p;
    C.printf("%d\n", q);
    return 0;
}
"#;
    let output = Scratch::new("auto-deref").run("auto_deref", source);
    assert_eq!(output.trim(), "7");
}

#[test]
fn an_rvalue_reference_binds_the_initialiser_it_names() {
    // `b &&auto = std.move(a);` is the spelling the reference gives for "this
    // is the dying value itself". It did not parse: a statement beginning with
    // a name and `&&` was read as a logical `&&` expression, so the `auto` had
    // nowhere to go. `a &auto = ..` already read as a binding, and the two forms
    // follow the same rule.
    let source = r#"import std;
import C;

fn main() int {
    a String = "a long string that owns a buffer of its own for sure here";
    b &&auto = std.move(a);
    c String = b;
    C.printf("%s\n", c.data());
    return 0;
}
"#;
    let output = Scratch::new("auto-rvalue").run("auto_rvalue", source);
    assert_eq!(
        output.trim(),
        "a long string that owns a buffer of its own for sure here"
    );
}
