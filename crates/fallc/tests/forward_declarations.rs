//! Forward declarations, and the incomplete types they make.
//!
//! `type Foo;` says the name exists and nothing else. Until a definition is
//! seen, the type has no layout and no members, so only a pointer or a
//! reference to it may be written. Declarations are order-independent, so a
//! file that declares a type and defines it later is fine: what decides whether
//! a type is complete is whether the definition is anywhere in the program.

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
            std::env::temp_dir().join(format!("fallc-forward-{label}-{}", std::process::id()));
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

#[test]
fn a_declaration_before_the_definition_costs_nothing() {
    // The definition is in the program, so the type is complete wherever it is
    // used -- order-independent, like every other declaration.
    let source = r#"import std;

type Node;

fn build(v int) Node {
    return .{v, nullptr};
}

type Node {
    value int;
    next *Node;
}

fn main() int {
    n Node = build(7);
    C.printf("%d\n", n.value);
    return 0;
}
"#;
    let scratch = Scratch::new("complete");
    assert_eq!(scratch.run("complete", source), "7\n");
}

#[test]
fn a_pointer_and_a_reference_to_an_incomplete_type_are_fine() {
    let source = r#"import std;

type Node;

fn main() int {
    p *Node = nullptr;
    C.printf("%d\n", (p == nullptr) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("pointer");
    assert_eq!(scratch.run("pointer", source), "1\n");
}

#[test]
fn a_value_of_an_incomplete_type_is_refused() {
    let source = "type Node;\n\nfn main() int {\n    n Node;\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is only declared"), "{diagnostics}");
}

#[test]
fn measuring_an_incomplete_type_is_refused() {
    let source = "type Node;\n\nfn main() int {\n    return sizeof(Node) as int;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("the type is incomplete"),
        "{diagnostics}"
    );
}

#[test]
fn a_member_of_an_incomplete_type_is_not_known() {
    let source = "type Node;\n\nfn read(p *Node) int {\n    return p.value;\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("its members are not known yet"),
        "{diagnostics}"
    );
}

#[test]
fn a_union_may_be_declared_first() {
    let source = r#"import std;

union Value;

type Cell;

fn main() int {
    v *Value = nullptr;
    c *Cell = nullptr;
    C.printf("%d\n", (v == nullptr && c == nullptr) as int);
    return 0;
}

union Value {
    i int;
    f float;
}

type Cell {
    value int;
}
"#;
    let scratch = Scratch::new("union");
    assert_eq!(scratch.run("union", source), "1\n");
}

#[test]
fn an_enum_declaration_names_its_base_and_the_members_come_later() {
    let source = r#"import std;

enum Color u32;

fn main() int {
    c Color = 2u32;
    C.printf("%d\n", c as int);
    return 0;
}

enum Color u32 {
    RED,
    GREEN,
}
"#;
    let scratch = Scratch::new("enum");
    assert_eq!(scratch.run("enum", source), "2\n");
}

#[test]
fn a_trait_has_to_be_defined_before_it_is_asked_about() {
    let source = r#"traits Walkable(it int);

fn main() int {
    ok bool = Walkable(int);
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is incomplete"), "{diagnostics}");
}

#[test]
fn two_definitions_of_one_type_are_still_a_mistake() {
    let source = "type Node {\n    a int;\n}\n\ntype Node {\n    b int;\n}\n\nfn main() int {\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("defined more than once"),
        "{diagnostics}"
    );
}
