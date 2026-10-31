//! Statement forms that the grammar had to grow into: a `for` clause that is an
//! expression rather than a binding, and a `switch` label that must be a value
//! the compiler knows.

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
            std::env::temp_dir().join(format!("fallc-stmt-{label}-{}", std::process::id()));
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
fn a_for_initialiser_may_be_an_expression() {
    // Any expression may stand where a statement may, and the initialiser of a
    // `for` is one such place.
    let source = r#"import std;

fn main() int {
    i int = 0;
    total int = 0;
    for (i = 1; i < 4; i += 1) {
        total += i;
    }
    C.printf("%d %d\n", i, total);
    return 0;
}
"#;
    let scratch = Scratch::new("for");
    assert_eq!(scratch.run("for", source), "4 6\n");
}

#[test]
fn a_case_label_has_to_be_a_value_the_compiler_knows() {
    let source = r#"import std;

total int = 0;

fn bump() int {
    total += 1;
    return total;
}

fn main() int {
    switch (total) {
        case bump(): {
            return 1;
        }
        default: {
            return 0;
        }
    }
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("`case` label"), "{diagnostics}");
}

#[test]
fn two_arms_may_not_claim_the_same_label() {
    let source = r#"fn main() int {
    n int = 1;
    switch (n) {
        case 1: {
            return 1;
        }
        case 2: {
            return 2;
        }
        case 1: {
            return 3;
        }
        default: {
            return 0;
        }
    }
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("already a label"), "{diagnostics}");
}

#[test]
fn a_folded_case_label_is_a_constant() {
    // A call the compiler can evaluate is a value it knows, so it may stand in
    // a `case` label just as a literal may.
    let source = r#"import std;

@constexpr fn triple(n int) int {
    return n * 3;
}

fn main() int {
    n int = 6;
    switch (n) {
        case triple(2): {
            C.printf("six\n");
        }
        default: {
            C.printf("other\n");
        }
    }
    return 0;
}
"#;
    let scratch = Scratch::new("label");
    assert_eq!(scratch.run("label", source), "six\n");
}
