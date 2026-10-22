//! Macros, end to end: definition, invocation, and what the parser sees.
//!
//! A macro is expanded between lexing and parsing, so everything here goes
//! through the whole compiler: a definition that survives to the parser would
//! be a syntax error, and an expansion the parser never saw could not run.

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
            std::env::temp_dir().join(format!("fallc-macro-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn module(&self, name: &str, source: &str) -> &Self {
        std::fs::write(self.directory.join(format!("{name}.ftm")), source)
            .expect("the module can be written");
        self
    }

    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the program can be written");
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
fn an_object_like_macro_stands_for_its_body() {
    let source = r#"import std;

define N 5

fn main() int {
    a [$N]int;
    C.printf("%d\n", $N);
    return 0;
}
"#;
    let scratch = Scratch::new("object");
    assert_eq!(scratch.run("object", source), "5\n");
}

#[test]
fn a_function_like_macro_takes_arguments() {
    let source = r#"import std;

define SQUARE(x) ((x) * (x))
define ADD(a, b) ((a) + (b))

fn main() int {
    C.printf("%d %d\n", $SQUARE(3), $ADD(4, 5));
    return 0;
}
"#;
    let scratch = Scratch::new("function");
    assert_eq!(scratch.run("function", source), "9 9\n");
}

#[test]
fn a_macro_expands_recursively() {
    let source = r#"import std;

define ONE 1
define TWO $ONE + $ONE
define FOUR $TWO + $TWO

fn main() int {
    C.printf("%d\n", $FOUR);
    return 0;
}
"#;
    let scratch = Scratch::new("recursive");
    assert_eq!(scratch.run("recursive", source), "4\n");
}

#[test]
fn a_paramete_ris_stringified_by_hash() {
    let source = r#"import std;

define NAME(x) #x

fn main() int {
    C.printf("[%s]\n", $NAME(hello world));
    return 0;
}
"#;
    let scratch = Scratch::new("stringify");
    assert_eq!(scratch.run("stringify", source), "[hello world]\n");
}

#[test]
fn two_tokens_are_pasted_by_double_hash() {
    let source = r#"import std;

define JOIN(a, b) a ## b

fn main() int {
    value int = 42;
    C.printf("%d\n", $JOIN(4, 2));
    return 0;
}
"#;
    let scratch = Scratch::new("paste");
    assert_eq!(scratch.run("paste", source), "42\n");
}

#[test]
fn a_variadic_macro_reaches_its_arguments_through_va_list() {
    let source = r#"import std;

define LOG(...) C.printf(__VA_LIST__)

fn main() int {
    $LOG("%d %d\n", 1, 2);
    return 0;
}
"#;
    let scratch = Scratch::new("variadic");
    assert_eq!(scratch.run("variadic", source), "1 2\n");
}

#[test]
fn a_backslash_continues_a_definition() {
    let source = r#"import std;

define TRIPLE(x) ((x) + \
                        (x) + (x))

fn main() int {
    C.printf("%d\n", $TRIPLE(2));
    return 0;
}
"#;
    let scratch = Scratch::new("continue");
    assert_eq!(scratch.run("continue", source), "6\n");
}

#[test]
fn a_macro_may_stand_where_a_type_is_written() {
    let source = r#"import std;

define ELEM int
define LEN 3

fn main() int {
    a [$LEN]$ELEM;
    a[2] = 7;
    C.printf("%d\n", a[2]);
    return 0;
}
"#;
    let scratch = Scratch::new("type");
    assert_eq!(scratch.run("type", source), "7\n");
}

#[test]
fn an_exported_definition_is_usable_from_an_importing_module() {
    let scratch = Scratch::new("export");
    scratch.module(
        "twice",
        "import std;\n\n\
         export define LIMIT 3\n\
         export define DOUBLE(x) ((x) * 2)\n\n\
         export fn scale(n int) int {\n\
         \x20   if (n > $LIMIT) {\n\
         \x20       return $LIMIT;\n\
         \x20   }\n\
         \x20   return $DOUBLE(n);\n\
         }\n",
    );
    let source = r#"import std;
import twice;

fn main() int {
    C.printf("%d %d\n", twice.scale(2), twice.scale(9));
    return 0;
}
"#;
    assert_eq!(scratch.run("export", source), "4 3\n");
}

#[test]
fn an_argument_may_be_any_expression() {
    // A conditional in an argument is checked like any other conditional. It
    // was not, once: every token of an expansion carried the invocation's span,
    // and two expressions that share a span cannot be told apart by the
    // checker's span-keyed records.
    let source = r#"import std;

define LOG(fmt, ...) C.printf(fmt, __VA_LIST__)
define MAX(a, b) ((a) > (b) ? (a) : (b))

fn main() int {
    a int = 5;
    b int = 2;
    $LOG("%d %d\n", a > b ? a : b, $MAX(3, 9));
    return 0;
}
"#;
    let scratch = Scratch::new("argument");
    assert_eq!(scratch.run("argument", source), "5 9\n");
}

#[test]
fn an_unknown_macro_is_reported_at_its_invocation() {
    let (outcome, diagnostics) = compile_text(
        "t.ft",
        "fn main() int {\n    return $MISSING;\n}\n",
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("MISSING"), "{diagnostics}");
}
