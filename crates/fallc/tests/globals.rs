//! Module-level variables: storage for the whole run.
//!
//! A variable at module scope is `static` in C's sense -- one object, alive
//! from the start of the program -- and unlike a `const` it may be assigned.
//! Its initialiser is part of the program image, so it has to be computable
//! before anything runs; a value that depends on run-time state is a local.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::{compile_text, run};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-global-{label}-{}", std::process::id()));
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
fn a_module_level_variable_keeps_its_value_between_calls() {
    // The whole point of the storage: two calls see one object.
    let source = r#"import std;

counter int = 0;

fn bump() int {
    counter += 1;
    return counter;
}

fn main() int {
    bump();
    bump();
    C.printf("%d\n", counter);
    return 0;
}
"#;
    let scratch = Scratch::new("counter");
    assert_eq!(scratch.run("counter", source), "2\n");
}

#[test]
fn a_global_starts_at_its_written_value() {
    let source = r#"import std;

seed u64 = 88172645463325252;

fn main() int {
    C.printf("%lu\n", seed);
    return 0;
}
"#;
    let scratch = Scratch::new("seed");
    assert_eq!(scratch.run("seed", source), "88172645463325252\n");
}

#[test]
fn a_variable_may_be_written_before_its_declaration_in_the_file() {
    // Declarations are order-independent, as they are for functions and
    // constants.
    let source = r#"import std;

fn read() int {
    return value;
}

value int = 7;

fn main() int {
    C.printf("%d\n", read());
    return 0;
}
"#;
    let scratch = Scratch::new("forward");
    assert_eq!(scratch.run("forward", source), "7\n");
}

#[test]
fn an_unused_global_is_not_an_unused_local() {
    let (outcome, printed) = compile_text(
        "t.ft",
        "hidden int = 1;\n\nfn main() int {\n    return 0;\n}\n",
        &[fallc::cli::EmitKind::Llvm],
    );
    assert_eq!(outcome, fallc::Outcome::Success, "{printed}");
    assert!(!printed.contains("S0140"), "{printed}");
}
