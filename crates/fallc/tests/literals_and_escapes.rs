//! String literals: escapes, raw strings, and the mistakes around them.
//!
//! A string literal is the bytes it stands for, so `"a\tb"` is three bytes and
//! not four. The decoding happens in one place -- `fallc_ast::literal` -- so a
//! literal used as a value and the same literal used as a template argument
//! cannot disagree.

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
            std::env::temp_dir().join(format!("fallc-literal-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, label: &str, source: &str) -> Vec<u8> {
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
        output.stdout
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn escapes_are_decoded_into_the_bytes_they_name() {
    let source = r#"import std;

fn main() int {
    C.printf("%s", "a\tb\nc\\d\"e\x41");
    return 0;
}
"#;
    let scratch = Scratch::new("escapes");
    let printed = scratch.run("escapes", source);
    assert_eq!(printed, b"a\tb\nc\\d\"eA");
}

#[test]
fn a_unicode_escape_is_encoded_as_utf8() {
    let source = r#"import std;

fn main() int {
    C.printf("%s\n", "\u{263A}");
    return 0;
}
"#;
    let scratch = Scratch::new("unicode");
    assert_eq!(scratch.run("unicode", source), "☺\n".as_bytes());
}

#[test]
fn a_raw_string_has_no_escapes_and_may_span_lines() {
    let source = "import std;\n\nfn main() int {\n    C.printf(\"[%s]\", r\"a\\nb\nc\");\n    return 0;\n}\n";
    let scratch = Scratch::new("raw");
    assert_eq!(scratch.run("raw", source), b"[a\\nb\nc]");
}

#[test]
fn a_raw_string_may_be_delimited_by_a_tag() {
    let source = "import std;\n\nfn main() int {\n    C.printf(\"[%s]\", r#tag\"has \"quotes\" inside\"tag#);\n    return 0;\n}\n";
    let scratch = Scratch::new("tag");
    assert_eq!(scratch.run("tag", source), b"[has \"quotes\" inside]");
}

#[test]
fn a_literal_that_is_never_closed_is_reported() {
    let source = "fn main() int {\n    s *const c8 = \"unterminated\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("L0014"), "{diagnostics}");
    assert!(diagnostics.contains("never closed"), "{diagnostics}");
}

#[test]
fn only_the_widths_that_name_a_character_type_prefix_a_literal() {
    // `u64"x"` is not a `c8` literal with its prefix dropped: there is no `c64`
    // for it to be, so it is a name followed by a string and the parser says so.
    let source = "fn main() int {\n    s *const c8 = u64\"ab\";\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("expected"), "{diagnostics}");
}

#[test]
fn a_triple_quoted_string_may_span_lines_and_still_decodes_escapes() {
    // `""".."""` is the multi-line spelling, not the raw one: the newlines are
    // part of the text and the escapes are still escapes.
    let source = "import std;\n\nfn main() int {\n    text *const c8 = \"\"\"one\ntwo\\tthree\nfour \\\"quoted\\\"\"\"\";\n    C.printf(\"[%s]\", text);\n    return 0;\n}\n";
    let scratch = Scratch::new("multiline");
    let printed = scratch.run("multiline", source);
    assert_eq!(printed, b"[one\ntwo\tthree\nfour \"quoted\"]".to_vec());
}

#[test]
fn a_triple_quoted_string_counts_its_decoded_bytes() {
    // Two characters and the NUL is `[3]c8`; `a\n` is two bytes, not three.
    let source = "fn main() int {\n    a [3]const c8 = \"\"\"ab\"\"\";\n    b [3]const c8 = \"\"\"a\\n\"\"\";\n    return (a[0] as int) + (b[1] as int) - (b[2] as int);\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}

#[test]
fn an_escaped_quote_does_not_close_a_triple_quoted_string() {
    let source =
        "fn main() int {\n    s [6]const c8 = \"\"\"a\\\"\\\"\\\"b\"\"\";\n    return 0;\n}\n";
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    // Three escaped quotes are three characters, so the body is `a"""b`.
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
}
