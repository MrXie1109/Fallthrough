//! Forwarding references and `std.forward`.
//!
//! `&&T` with `T` a template parameter is a forwarding reference: an lvalue
//! argument deduces `T` as `&U`, an rvalue deduces `U`, and `std.forward<T>`
//! gives the value back with the category it arrived with.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir()
            .join(format!("fallc-forward-{label}-{}", std::process::id()));
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
fn a_forwarding_reference_deduces_the_value_category() {
    let source = r#"import std;
import C;

template <T type>
fn is_lvalue(value &&T) int {
    if (IsReference(T)) @constexpr {
        return 1;
    } else {
        return 0;
    }
}

fn main() int {
    a int = 1;
    C.printf("%d %d\n", is_lvalue(a), is_lvalue(2));
    return 0;
}
"#;
    let scratch = Scratch::new("category");
    assert_eq!(scratch.run("category", source), "1 0\n");
}

#[test]
fn forward_keeps_an_lvalue_an_lvalue_and_an_rvalue_an_rvalue() {
    // The sink reports which overload took the value: `&int` for an lvalue,
    // `&&int` for an rvalue.
    let source = r#"import std;
import C;

fn sink(value &int) int {
    return 1;
}

fn sink(value &&int) int {
    return 2;
}

template <T type>
fn relay(value &&T) int {
    return sink(std.forward<T>(value));
}

fn main() int {
    a int = 7;
    C.printf("%d %d\n", relay(a), relay(9));
    return 0;
}
"#;
    let scratch = Scratch::new("forward");
    assert_eq!(scratch.run("forward", source), "1 2\n");
}
