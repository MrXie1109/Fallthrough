//! Numbers: the generic operations over any numeric type, and the ones on reals.
//!
//! `min`, `max`, `clamp`, `abs`, `square` are templates: they ask only what a
//! number already answers. The rest is the C library at `f64`, which is the
//! precision it computes at.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-math-{label}-{}", std::process::id()));
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
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn the_generic_operations_work_for_integers_and_for_reals() {
    let source = r#"import std;

fn main() int {
    C.printf("%d %d %d %d\n", std.min(3, 5), std.max(3, 5), std.clamp(12, 0, 10), std.abs(-7));
    C.printf("%d %d %d\n", std.clamp(5, 0, 10), std.clamp(-3, 0, 10), std.clamp(10, 0, 10));
    a int = 5;
    b int = 0;
    c int = 10;
    C.printf("%d %d\n", std.clamp(a, b, c), std.clamp(-3, b, c));
    C.printf("%d %d\n", std.square(4), std.squared_length(3, 4));
    C.printf("%.2f %.2f\n", std.min(2.5, 1.5), std.abs(-2.5));
    return 0;
}
"#;
    let scratch = Scratch::new("generic");
    assert_eq!(
        scratch.run("generic", source),
        "3 5 10 7\n5 0 10\n5 0\n16 25\n1.50 2.50\n"
    );
}

#[test]
fn the_real_functions_are_the_libraries() {
    let source = r#"import std;

fn main() int {
    C.printf("%.4f %.1f %.1f %.1f\n", std.sqrt(2.0), std.pow(2.0, 10.0), std.floor(3.7), std.ceil(3.2));
    C.printf("%.4f %.1f\n", std.log(std.exp(1.0)), std.fabs(-3.5));
    return 0;
}
"#;
    let scratch = Scratch::new("real");
    assert_eq!(
        scratch.run("real", source),
        "1.4142 1024.0 3.0 4.0\n1.0000 3.5\n"
    );
}
