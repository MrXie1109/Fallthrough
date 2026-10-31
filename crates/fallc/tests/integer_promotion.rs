//! Lossless integer promotion (02 Types §12.5).
//!
//! A small integer type may stand where a wider integer type is wanted when
//! every value of the source fits in the target; signed to unsigned is never
//! implicit, and `bool`, the character types and `byte` do not take part.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-promote-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn options(&self, label: &str, source: &str) -> (PathBuf, Options) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        (executable, options)
    }

    fn accepted(&self, label: &str, source: &str) -> String {
        let (executable, options) = self.options(label, source);
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

    fn rejected(&self, label: &str, source: &str) -> String {
        let (_executable, options) = self.options(label, source);
        assert_eq!(
            run(&options),
            std::process::ExitCode::FAILURE,
            "`{label}` was accepted but must be rejected"
        );
        String::new()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn widening_in_every_target_position_works() {
    let source = r#"import std;

fn take(value i64) i64 {
    return value;
}

fn produce() i64 {
    small int = 7;
    return small;
}

fn main() int except {
    small int = 7;
    wide i64 = small;
    assignment i64 = 0i64;
    assignment = small;
    unsigned u8 = 200u8;
    wide_unsigned u64 = unsigned;
    wide_signed i16 = unsigned;
    medium u16 = 1000u16;
    signed i32 = medium;
    std.print("{} {} {} {} {} {}\n",
        wide, take(small), produce(), wide_unsigned, wide_signed, signed)??;
    return 0;
}
"#;
    let scratch = Scratch::new("allowed");
    assert_eq!(scratch.accepted("allowed", source), "7 7 7 200 200 1000\n");
}

#[test]
fn illegal_directions_are_refused() {
    let scratch = Scratch::new("rejected");

    // Signed to unsigned is never implicit.
    scratch.rejected(
        "signed-to-unsigned",
        r#"fn main() int {
    signed int = -1;
    target u64 = signed;
    return 0;
}
"#,
    );
    // Unsigned to signed of the same width is not a widening.
    scratch.rejected(
        "same-width",
        r#"fn main() int {
    source u32 = 1u32;
    target i32 = source;
    return 0;
}
"#,
    );
    // Narrowing.
    scratch.rejected(
        "narrowing",
        r#"fn main() int {
    source i64 = 1i64;
    target i32 = source;
    return 0;
}
"#,
    );
    // `bool`, characters and `byte` do not participate.
    scratch.rejected(
        "bool",
        r#"fn main() int {
    flag bool = true;
    target int = flag;
    return 0;
}
"#,
    );
    scratch.rejected(
        "character",
        r#"fn main() int {
    letter c8 = 'a';
    target int = letter;
    return 0;
}
"#,
    );
    scratch.rejected(
        "byte",
        r#"fn main() int {
    raw byte = 1 as byte;
    target int = raw;
    return 0;
}
"#,
    );
}

#[test]
fn an_explicit_cast_still_allows_every_direction() {
    let source = r#"import std;

fn main() int except {
    signed int = -1;
    unsigned u64 = signed as u64;
    wide i64 = 300i64;
    narrow u8 = wide as u8;
    flag bool = true;
    number int = flag as int;
    std.print("{} {} {} {}\n", unsigned, narrow, number, (300 as u8))??;
    return 0;
}
"#;
    let scratch = Scratch::new("casts");
    assert_eq!(
        scratch.accepted("casts", source),
        "18446744073709551615 44 1 44\n"
    );
}
