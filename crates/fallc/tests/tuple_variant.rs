//! `Pair`/`Triple` and `Variant2`: a fixed group of values, and one of a list.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-tup-{label}-{}", std::process::id()));
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
fn a_pair_copies_its_elements() {
    let source = r#"import std;

fn main() int except {
    point Pair<int, int> = Pair<int, int>(3, 4);
    std.print("{} {}\n", point.first, point.second)?!;
    text Pair<String, int> = Pair<String, int>(String("hi"), 7);
    copied Pair<String, int> = text;
    copied.first.push('!');
    std.print("{} {} {}\n", text.first, copied.first, copied.second)?!;
    std.print("eq {}\n", (point == Pair<int, int>(3, 4)) as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("pair");
    assert_eq!(scratch.run("pair", source), "3 4\nhi hi! 7\neq 1\n");
}

#[test]
fn a_triple_has_three_positions() {
    let source = r#"import std;

fn main() int except {
    three Triple<int, char, String> = Triple<int, char, String>(1, 'x', String("end"));
    std.print("{} {} {}\n", three.first, three.third, three.second)?!;
    moved Triple<int, char, String> = std.move(three);
    std.print("{} {}\n", moved.first, moved.third)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("triple");
    assert_eq!(scratch.run("triple", source), "1 end x\n1 end\n");
}

#[test]
fn a_variant_says_which_alternative_it_holds() {
    let source = r#"import std;

fn main() int except {
    value Variant2<int, String> = Variant2<int, String>(5);
    std.print("{} {}\n", value.is_first() as int, value.first()?!)?!;
    value.set_second(String("now text"));
    std.print("{} {}\n", value.is_second() as int, value.second()?!)?!;
    try {
        std.print("never {}\n", value.first()?!)?!;
    } except {
        e &const std.Exception = std.exception();
        std.print("caught {}\n", e.name())?!;
    }
    copied Variant2<int, String> = value;
    std.print("{} {}\n", copied.is_second() as int, copied.second()?!)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("variant");
    assert_eq!(
        scratch.run("variant", source),
        "1 5\n1 now text\ncaught TypeError\n1 now text\n"
    );
}
