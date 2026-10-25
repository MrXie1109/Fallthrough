//! `std.test` macros, `std.debug` and `std.prelude`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-testing-{label}-{}", std::process::id()));
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
fn check_macros_pass_and_report_failures() {
    let source = r#"import std;

fn main() int except {
    $CHECK(1 + 1 == 2);
    $CHECK_EQ(6, 3 * 2);
    $CHECK_EQ_REAL(2.5, 5.0 / 2.0);
    expected String = String("same");
    actual String = String("same");
    $CHECK_EQ_TEXT(actual, expected);
    std.print("passed\n")??;
    try {
        $CHECK_EQ(1, 2);
    } except {
        std.print("{} ", std.exception().name())??;
    }
    try {
        $FAIL("deliberate");
    } except {
        std.print("{}", std.exception().name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("check");
    assert_eq!(scratch.run("check", source), "passed\nTestError TestError");
}

#[test]
fn debug_assertions_and_traces_work() {
    let source = r#"import std;

fn main() int except {
    $ASSERT_DEBUG(2 > 1);
    message String = String("a trace");
    std.print_trace(&message);
    try {
        $ASSERT_DEBUG(1 > 2);
    } except {
        std.print("{}", std.exception().name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("debug");
    assert_eq!(scratch.run("debug", source), "DebugError");
}

#[test]
fn the_prelude_brings_the_common_names() {
    let source = r#"import std.prelude;

fn main() int except {
    text String = String("prelude");
    array Array<int> = Array<int>();
    array.push(3);
    array.push(4);
    sample f64 = prelude.sqrt(16.0);
    prelude.print("{} {} {}", text, array[0], sample)??;
    return 0;
}
"#;
    let scratch = Scratch::new("prelude");
    assert_eq!(scratch.run("prelude", source), "prelude 3 4");
}
