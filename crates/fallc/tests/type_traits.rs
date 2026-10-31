//! The compiler's type traits.
//!
//! A trait query folds to a constant about a type, so it can choose an
//! `if constexpr` branch or constrain an overload with `@enable_if` -- the two
//! ways a C++ template asks what kind of type it was given.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-traits-{label}-{}", std::process::id()));
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
fn the_traits_answer_about_types() {
    // Every trait in one program, answered at compile time.
    let source = r#"import std;
import C;

fn main() int {
    p *int = nullptr;
    C.printf("%d %d %d\n",
        IsInteger(int) as int,
        IsInteger(f64) as int,
        IsFloating(f64) as int);
    C.printf("%d %d %d\n",
        IsNumeric(bool) as int,
        IsPointer(type(p)) as int,
        IsReference(type(p)) as int);
    C.printf("%d %d %d\n",
        IsArray(type(p)) as int,
        IsVoid(void) as int,
        IsBool(bool) as int);
    C.printf("%d %d\n", IsCharacter(char) as int, IsSame(int, i32) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("answers");
    assert_eq!(scratch.run("answers", source), "1 0 1\n0 1 0\n0 1 1\n1 1\n");
}

#[test]
fn a_trait_chooses_a_branch_and_an_overload() {
    // `if constexpr` folds the query, and `@enable_if` picks between two
    // templates that differ only in the type they accept.
    let source = r#"import std;
import C;

@enable_if(IsInteger(T))
template <T type>
fn kind_of(value T) String {
    return String("integer");
}

@enable_if(IsFloating(T))
template <T type>
fn kind_of(value T) String {
    return String("float");
}

template <T type>
fn branch(value T) String {
    if (IsInteger(T)) @constexpr {
        return String("int-branch");
    } else {
        return String("other-branch");
    }
}

fn main() int except {
    C.printf("%s %s\n", kind_of(1).data(), kind_of(1.5).data());
    C.printf("%s %s\n", branch(1).data(), branch(2.5).data());
    return 0;
}
"#;
    let scratch = Scratch::new("dispatch");
    assert_eq!(
        scratch.run("dispatch", source),
        "integer float\nint-branch other-branch\n"
    );
}
