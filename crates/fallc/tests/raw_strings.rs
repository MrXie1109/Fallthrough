//! Raw strings: `r"..."` and `r#tag"..."tag#`.
//!
//! A raw string is not "a string with fewer escapes" -- it is a string whose
//! characters are exactly the source between its delimiters. `r"C:\dir"` is
//! four characters, and the delimiter form exists for the text that has to
//! contain a `"`: `r#q"a "quoted" word"q#`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::{compile_text, run};

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-raw-{label}-{}", std::process::id()));
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
        assert!(
            output.status.success(),
            "`{label}` failed: {}",
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

#[test]
fn a_raw_string_keeps_its_characters() {
    // No escape is decoded, so `\n` is the two characters `\` and `n`, and the
    // length says so: `r"a\nb"` is four bytes plus the terminator.
    let source = r#"import std;

fn main() int {
    C.printf(r"a\nb\n");
    C.printf("%lu\n", sizeof(type(r"a\nb")));
    return 0;
}
"#;
    let scratch = Scratch::new("keep");
    assert_eq!(scratch.run("keep", source), "a\\nb\\n5\n");
}

#[test]
fn a_delimited_raw_string_may_hold_quotes() {
    let source = r#"import std;

fn main() int {
    C.printf(r#q"a "quoted" word"q#);
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("quoted");
    assert_eq!(scratch.run("quoted", source), "a \"quoted\" word\n");
}

#[test]
fn a_raw_string_may_span_lines() {
    let source = "import std;\n\nfn main() int {\n    C.printf(r\"one\ntwo\");\n    return 0;\n}\n";
    let scratch = Scratch::new("lines");
    assert_eq!(scratch.run("lines", source), "one\ntwo");
}

#[test]
fn a_raw_string_is_still_a_string() {
    // The type is `[N]c8` with a terminator, so it decays to a pointer and is
    // accepted where a string is wanted.
    let source = r#"import std;

fn main() int {
    s *const c8 = r"text";
    C.printf("%s\n", s);
    return 0;
}
"#;
    let (outcome, printed) =
        compile_text("t.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Success, "{printed}");
}
