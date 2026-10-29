//! `std.iter`'s range tools and `std.range`'s integer ranges.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-itr-{label}-{}", std::process::id()));
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
fn range_tools_advance_find_and_test() {
    let source = r#"import std;

fn main() int except {
    values Array<int> = Array<int>();
    values.push(4);
    values.push(5);
    values.push(6);
    values.push(7);
    even Even = Even();
    odd Odd = Odd();
    std.print("{} {} {} {}\n",
        std.count_if(values.begin(), values.end(), even),
        std.all_of(values.begin(), values.end(), even) as int,
        std.any_of(values.begin(), values.end(), odd) as int,
        std.none_of(values.begin(), values.end(), odd) as int)??;
    first *int = std.find_if(values.begin(), values.end(), even);
    std.print("{}\n", *first)??;
    third *int = std.advance(values.begin(), values.end(), 2usize);
    last *int = std.nth(values.begin(), values.end(), 99usize);
    std.print("{} {}\n", *third, (last == values.end()) as int)??;
    return 0;
}
"#;
    let scratch = Scratch::new("iter");
    assert_eq!(
        scratch.run("iter", source),
        "2 0 1 0\n4\n6 1\n"
    );
}

#[test]
fn integer_ranges_count_up_and_down() {
    let source = r#"import std;

fn main() int except {
    total i64 = 0i64;
    for (value i64 : std.to(5i64)) {
        total += value;
    }
    std.print("{} {}\n", total, std.to(5i64).length())??;
    down i64 = 0i64;
    for (value i64 : std.down_to(3i64, 0i64)) {
        down += value;
    }
    std.print("{}\n", down)??;
    stepped i64 = 0i64;
    range Range = Range(0i64, 10i64, 3i64)??;
    for (value i64 : range) {
        stepped += 1i64;
    }
    std.print("{} {}\n", stepped, range.length())??;
    return 0;
}
"#;
    let scratch = Scratch::new("range");
    assert_eq!(scratch.run("range", source), "10 5\n6\n4 4\n");
}
