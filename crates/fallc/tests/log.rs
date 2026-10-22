//! `std.log`: named levels, one line each, a filter, and a stream.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-log-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, label: &str, source: &str) -> (String, String) {
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
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn the_level_filters_the_lines_and_the_format_fills_them() {
    let source = r#"import std;
import std.log;

fn main() int except {
    log.info("hello {}", 1)?!;
    log.debug("hidden")?!;
    log.set_level(log.Level.DEBUG);
    log.debug("visible {}", 2)?!;
    log.set_level(log.Level.WARN);
    log.info("hidden again")?!;
    log.warn("careful {}", 3)?!;
    log.error("bad")?!;
    return 0;
}
"#;
    let scratch = Scratch::new("filter");
    let (out, err) = scratch.run("filter", source);
    assert_eq!(out, "");
    assert_eq!(
        err,
        "[INFO] hello 1\n[DEBUG] visible 2\n[WARN] careful 3\n[ERROR] bad\n"
    );
}

#[test]
fn a_level_can_silence_everything() {
    let source = r#"import std;
import std.log;

fn main() int except {
    log.set_level(log.Level.OFF);
    log.error("nothing")?!;
    std.print("{} {}\n", log.enabled(log.Level.ERROR) as int, log.level() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("off");
    let (out, err) = scratch.run("off", source);
    assert_eq!(out, "0 5\n");
    assert_eq!(err, "");
}
