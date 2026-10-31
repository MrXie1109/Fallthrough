//! `?T`: a value that may be absent, paired with the exception machinery.
//!
//! The question mark is the sugar; the type is `Optional<T>` from `std`, which
//! holds its value inline and answers `present()`, `value()`, `value_or()` and
//! `reset()`. `value()` on an absent optional raises `NoneError`, so "there is
//! no value" travels the same path as every other failure.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-opt-{label}-{}", std::process::id()));
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
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn an_optional_holds_a_value_or_nothing() {
    let source = r#"import std;

fn main() int except {
    present ?int = Optional<int>(3);
    C.printf("%d %d\n", present.present() as int, present.value()??);
    absent ?int = Optional<int>();
    C.printf("%d %d\n", absent.present() as int, absent.value_or(9));
    if (present) {
        C.printf("if yes\n");
    }
    if (absent) {
        C.printf("if no\n");
    } else {
        C.printf("else no\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("basic");
    assert_eq!(scratch.run("basic", source), "1 3\n0 9\nif yes\nelse no\n");
}

#[test]
fn reading_an_absent_optional_raises_none_error() {
    let source = r#"import std;

fn main() int except {
    absent ?int = Optional<int>();
    try {
        C.printf("never %d\n", absent.value()??);
    } except {
        e &const std.Exception = std.exception();
        C.printf("%s: %s\n", e.name().data(), e.what().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("raises");
    assert_eq!(
        scratch.run("raises", source),
        "NoneError: the optional is absent\n"
    );
}

#[test]
fn a_function_answers_with_an_optional() {
    let source = r#"import std;

fn find(index int) ?int {
    if (index == 0) {
        return std.none<int>();
    }
    return std.some(7);
}

fn describe(value ?int) int except {
    if (value) {
        return value.value()??;
    }
    return -1;
}

fn main() int except {
    C.printf("%d %d\n", describe(find(0))??, describe(find(1))??);
    C.printf("%d\n", find(0).present() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("function");
    assert_eq!(scratch.run("function", source), "-1 7\n0\n");
}

#[test]
fn an_optional_string_owns_its_text() {
    // The value lives inline and is copied by the copy constructor, so the two
    // optionals own two buffers and destroying one leaves the other readable.
    let source = r#"import std;

fn main() int except {
    original ?String = Optional<String>(String("hello"));
    copy ?String = original;
    C.printf("%s %s\n", original.value()??.data(), copy.value()??.data());
    copy.reset();
    C.printf("%d %s\n", copy.present() as int, original.value()??.data());
    original.reset();
    C.printf("%d\n", original.present() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("string");
    assert_eq!(scratch.run("string", source), "hello hello\n0 hello\n0\n");
}
