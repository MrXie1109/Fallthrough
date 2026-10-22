//! The compiler's builtin macros, through `meta.ftm`.
//!
//! A builtin is a fact only the compiler has: this file's name, the line a macro
//! is on, the platform the program is being built for. `meta.ftm` wraps each one
//! under a `$meta.*` name; the builtin spelling uses the `builtin_` prefix so a
//! library macro and a compiler fact are told apart.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-meta-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn build(&self, label: &str, source: &str) -> std::process::Command {
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
        let mut command = std::process::Command::new(&executable);
        command.current_dir(&self.directory);
        command
    }

    fn run(&self, label: &str, source: &str) -> String {
        let mut command = self.build(label, source);
        let output = command.output().expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Compiles with a target override in the compiler's own environment.
    fn run_for(&self, label: &str, source: &str, target: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
            .arg(&main)
            .arg("-o")
            .arg(&executable)
            .env("FALLC_TARGET_OS", target)
            .status()
            .expect("the compiler can be run");
        assert!(status.success(), "`{label}` did not compile");
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn the_file_and_position_macros_describe_where_they_are() {
    let source = r#"import meta;
import C;

fn main() int {
    C.printf("%s|%d|%d\n", $meta.FILE, $meta.LINE, $meta.COLUMN);
    return 0;
}
"#;
    let scratch = Scratch::new("position");
    let output = scratch.run("position", source);
    let fields: Vec<&str> = output.trim_end().split('|').collect();
    assert_eq!(fields.len(), 3, "{output}");
    assert!(fields[0].ends_with("main.ft"), "{output}");
    assert_eq!(fields[1], "5", "{output}");
    let column: u32 = fields[2].parse().expect("a column number");
    assert!(column > 0, "{output}");
}

#[test]
fn the_target_macros_follow_the_platform_being_built() {
    // `FALLC_TARGET_OS` retargets the check, so the same program reports the
    // other platform's facts.
    let source = r#"import meta;
import C;

fn main() int {
    C.printf("%s|%s|%d|%d\n", $meta.PLATFORM, $meta.ARCH, $meta.WINDOWS, $meta.POSIX);
    return 0;
}
"#;
    let scratch = Scratch::new("target");
    let native = scratch.run("target", source);
    let line = native.lines().next().unwrap_or_default().to_owned();
    assert!(line.starts_with("linux|"), "{line}");
    assert!(line.ends_with("|0|1"), "{line}");

    let line = scratch.run_for("target-windows", source, "windows");
    let line = line.trim_end().to_owned();
    assert!(line.starts_with("windows|"), "{line}");
    assert!(line.ends_with("|1|0"), "{line}");
}
