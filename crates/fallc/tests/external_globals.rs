//! Variables that live in a library.
//!
//! `export stdin *c8 @extern_C;` declares a name whose storage is not ours:
//! the declaration says what the name means, and the library owns the object.
//! The C bindings use it for the three streams, which is what makes
//! `C.fgets(.., C.stdin)` and `C.fflush(C.stdout)` ordinary uses of an imported
//! name rather than compiler magic.

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
            std::env::temp_dir().join(format!("fallc-extern-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn module(&self, name: &str, source: &str) -> &Self {
        std::fs::write(self.directory.join(format!("{name}.ftm")), source)
            .expect("the module can be written");
        self
    }

    /// Compiles and runs a program in this directory, returning its output.
    fn run(&self, label: &str, source: &str) -> (bool, String) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the program can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        if run(&options) != std::process::ExitCode::SUCCESS {
            return (false, String::new());
        }
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        (true, String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn an_external_variable_declaration_is_in_the_tree() {
    let (outcome, tree) = compile_text("t.ft", "export counter int @extern_C;\n", &[EmitKind::Ast]);
    assert_eq!(outcome, Outcome::Success, "{tree}");
    assert!(tree.contains("ExternVariable"), "{tree}");
    assert!(tree.contains("counter"), "{tree}");
}

#[test]
fn a_declared_external_variable_is_a_name_a_program_can_use() {
    // Declared and read: the symbol is the library's, and code generation
    // declares it once so the linker can find it.
    let (outcome, ir) = compile_text(
        "t.ft",
        "import C;\n\
         \n\
         fn main() int {\n\
         \x20   C.fflush(C.stdout);\n\
         \x20   return 0;\n\
         }\n",
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Success, "{ir}");
    assert!(ir.contains("@stdout"), "{ir}");
}

#[test]
fn the_streams_are_objects_at_run_time() {
    // A declared-but-unresolved global would link to a null address; these are
    // the three `FILE *`s the C library actually owns.
    let source = r#"import std;

fn main() int {
    if (C.stdin == nullptr) {
        return 1;
    }
    if (C.stdout == nullptr) {
        return 2;
    }
    if (C.stderr == nullptr) {
        return 3;
    }
    C.printf("streams\n");
    return 0;
}
"#;
    let scratch = Scratch::new("streams");
    let (built, printed) = scratch.run("streams", source);
    assert!(built, "the program did not compile");
    assert_eq!(printed, "streams\n");
}

#[test]
fn an_imported_variable_is_bound_like_an_imported_function() {
    // The declaring module owns no storage either way; what travels is the
    // name and the type.
    let scratch = Scratch::new("imported");
    scratch.module(
        "shared",
        "export stdout *c8 @extern_C;\n\n\
         export fn stream() *c8 {\n\
         \x20   return stdout;\n\
         }\n",
    );
    let source = r#"import std;
import shared;

fn main() int {
    if (shared.stream() == nullptr) {
        return 1;
    }
    if (shared.stdout != shared.stream()) {
        return 2;
    }
    C.printf("same\n");
    return 0;
}
"#;
    let (built, printed) = scratch.run("imported", source);
    assert!(built, "the program did not compile");
    assert_eq!(printed, "same\n");
}
