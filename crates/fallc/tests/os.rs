//! Operating-system calls: the process, its environment, the clock.
//!
//! The tests ask the running process about itself, so they check the answer is
//! plausible rather than a particular number: the PID is positive, a variable
//! this process sets is readable back, the working directory is a path, and the
//! monotonic clock moves forward.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-os-{label}-{}", std::process::id()));
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
fn the_process_knows_itself() {
    let source = r#"import std;
import std.os;
import C;

fn main() int except {
    C.printf("pid positive %d\n", (os.pid() > 0) as int);
    C.printf("platform %s arch %s\n", os.platform(), os.arch());
    here Path = os.cwd()?!;
    C.printf("cwd nonempty %d\n", (!here.empty()) as int);
    C.printf("temp nonempty %d\n", (!os.temp_dir().empty()) as int);
    C.printf("home nonempty %d\n", (!os.home().empty()) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("process");
    let output = scratch.run("process", source);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "pid positive 1", "{output}");
    assert!(lines[1].starts_with("platform "), "{output}");
    assert_eq!(lines[2], "cwd nonempty 1", "{output}");
    assert_eq!(lines[3], "temp nonempty 1", "{output}");
    assert_eq!(lines[4], "home nonempty 1", "{output}");
}

#[test]
fn the_environment_is_readable_and_writable() {
    // A variable this process sets is one this process reads: the environment of
    // the running program, not a copy of the parent's.
    let source = r#"import std;
import std.os;
import C;

fn main() int except {
    os.set_env("FALLC_OS_TEST", "hello");
    C.printf("has %d\n", os.has_env("FALLC_OS_TEST") as int);
    value String = os.getenv("FALLC_OS_TEST")?!;
    C.printf("value %s\n", value.data());
    C.printf("missing %d\n", os.has_env("FALLC_OS_TEST_NOT_SET") as int);
    try {
        absent String = os.getenv("FALLC_OS_TEST_NOT_SET")?!;
        C.printf("unexpected %s\n", absent.data());
    } except {
        C.printf("raised %s\n", std.exception().name().data());
    }
    return 0;
}
"#;
    let scratch = Scratch::new("environment");
    assert_eq!(
        scratch.run("environment", source),
        "has 1\nvalue hello\nmissing 0\nraised KeyError\n"
    );
}

#[test]
fn the_monotonic_clock_moves_forward() {
    let source = r#"import std;
import std.os;
import C;

fn main() int {
    first u64 = os.monotonic_ns();
    os.sleep_ms(3);
    second u64 = os.monotonic_ns();
    C.printf("advanced %d\n", (second > first) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("clock");
    assert_eq!(scratch.run("clock", source), "advanced 1\n");
}

#[test]
fn the_shell_is_one_call_away() {
    // `system` hands the command to the shell and returns its status: zero for
    // a command that succeeded, non-zero for one that failed.
    let source = r#"import std.os;
import C;

fn main() int {
    C.printf("%d %d\n", os.system("exit 0") == 0 as int, os.system("exit 3") != 0 as int);
    return 0;
}
"#;
    let scratch = Scratch::new("system");
    assert_eq!(scratch.run("system", source), "1 1\n");
}
