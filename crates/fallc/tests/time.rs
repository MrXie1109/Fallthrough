//! `std.time`: durations, monotonic instants, and the wall clock.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-time-{label}-{}", std::process::id()));
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
fn durations_add_compare_and_render() {
    let source = r#"import std;
import std.time;

fn main() int except {
    a Duration = time.from_millis(1500);
    b Duration = time.from_seconds(2);
    std.print("{} {} {} {}\n", a.nanos(), a.millis(), a.seconds(), a)??;
    std.print("sum {} {}\n", (a + b).nanos(), (b - a).millis())??;
    std.print("cmp {} {}\n", (a < b) as int, (a == time.from_millis(1500)) as int)??;
    std.print("neg {}\n", time.from_millis(0) - time.from_millis(250))??;
    return 0;
}
"#;
    let scratch = Scratch::new("duration");
    assert_eq!(
        scratch.run("duration", source),
        "1500000000 1500 1 1.500s\nsum 3500000000 500\ncmp 1 1\nneg -0.250s\n"
    );
}

#[test]
fn an_instant_measures_how_long_something_took() {
    let source = r#"import std;
import std.time;
import std.os;

fn main() int except {
    start Instant = time.now();
    os.sleep_ms(3);
    took Duration = time.since(&start);
    std.print("{} {}\n", (took.nanos() >= 0i64) as int, (took.millis() >= 3i64) as int)??;
    return 0;
}
"#;
    let scratch = Scratch::new("instant");
    assert_eq!(scratch.run("instant", source), "1 1\n");
}

#[test]
fn the_wall_clock_converts_to_a_civil_date() {
    let source = r#"import std;
import std.time;

fn main() int except {
    std.print("{}\n", SystemTime(0i64).to_utc())??;
    std.print("{}\n", SystemTime(1792454400i64 * 1000000000i64).to_utc())??;
    std.print("{}\n", SystemTime(-86400i64 * 1000000000i64).to_utc())??;
    now SystemTime = time.unix_now();
    std.print("{}\n", (now.unix_seconds() > 1600000000i64) as int)??;
    return 0;
}
"#;
    let scratch = Scratch::new("utc");
    assert_eq!(
        scratch.run("utc", source),
        "1970-01-01 00:00:00\n2026-10-20 00:00:00\n1969-12-31 00:00:00\n1\n"
    );
}
