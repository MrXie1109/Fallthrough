//! Truth values: what a condition reads.
//!
//! The rule is stated per kind rather than "convert to `bool`": a reference is
//! tested through itself, an array is always true because it is viewed as a
//! pointer, and a pointer is true exactly when it is not null.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-truth-{label}-{}", std::process::id()));
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
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_pointer_is_true_when_it_is_not_null() {
    let source = r#"import std;

fn main() int {
    nums [2]int;
    p *int = nullptr;
    if (p) {
        C.printf("null is true\n");
    } else {
        C.printf("null is false\n");
    }
    p = &nums[0];
    if (p) {
        C.printf("an address is true\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("pointer");
    assert_eq!(
        scratch.run("pointer", source),
        "null is false\nan address is true\n"
    );
}

#[test]
fn an_array_is_always_true() {
    let source = r#"import std;

fn main() int {
    empty [1]int;
    if (empty) {
        C.printf("an array is true\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("array");
    assert_eq!(scratch.run("array", source), "an array is true\n");
}

#[test]
fn a_reference_is_tested_through_itself() {
    // The address of `r` is never null, so testing the address would make every
    // reference true; the referent decides instead.
    let source = r#"import std;

fn main() int {
    a [2]int;
    a[0] = 0;
    a[1] = 7;
    zero &int = *&a[0];
    seven &int = *&a[1];
    if (zero) {
        C.printf("zero is true\n");
    } else {
        C.printf("zero is false\n");
    }
    if (seven) {
        C.printf("seven is true\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("reference");
    assert_eq!(
        scratch.run("reference", source),
        "zero is false\nseven is true\n"
    );
}
