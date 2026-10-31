//! `std.Any`: a value of any type, kept as its text and type name.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-any-{label}-{}", std::process::id()));
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
fn a_holder_carries_values_of_different_types() {
    let source = r#"import std;

fn main() int except {
    a Any = Any(42);
    std.print("{} {} {}\n", a.present() as int, a.type_name(), a)??;
    std.print("int {}\n", a as int??)??;
    b Any = Any(String("hi"));
    std.print("{} {}\n", b.type_name(), b as String??)??;
    c Any = Any(2.5);
    std.print("real {}\n", c as f64??)??;
    d Any = Any(true);
    std.print("bool {}\n", d as bool??)??;
    return 0;
}
"#;
    let scratch = Scratch::new("types");
    assert_eq!(
        scratch.run("types", source),
        "1 int 42\nint 42\nString hi\nreal 2.5\nbool true\n"
    );
}

#[test]
fn a_wrong_question_raises_type_error() {
    let source = r#"import std;

fn main() int except {
    a Any = Any(42);
    try {
        std.print("never {}\n", a as String??)??;
    } except {
        e &const std.Exception = std.exception();
        std.print("caught {}\n", e.name())??;
    }
    empty Any = Any();
    std.print("empty {} {}\n", empty.present() as int, empty.type_name())??;
    return 0;
}
"#;
    let scratch = Scratch::new("raises");
    assert_eq!(
        scratch.run("raises", source),
        "caught TypeError\nempty 0 \n"
    );
}

#[test]
fn a_holder_is_copied_and_compared() {
    let source = r#"import std;

fn main() int except {
    a Any = Any(String("text"));
    b Any = a;
    std.print("eq {} {}\n", (a == b) as int, (a == Any(42)) as int)??;
    b.reset();
    std.print("{} {}\n", b.present() as int, a as String??)??;
    return 0;
}
"#;
    let scratch = Scratch::new("copies");
    assert_eq!(scratch.run("copies", source), "eq 1 0\n0 text\n");
}
