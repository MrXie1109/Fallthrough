//! `std.fmt`: the format rule behind `print`, and the specs it understands.
//!
//! The rule lives in `std/fmt.ftm`; `std.io` writes what that module renders.
//! A spec is `[[fill]align][width][.precision][type]`, applied to the value's
//! rendered text: alignment and width work on any text, `x`/`X`/`b`/`o` convert
//! a decimal integer, and `.precision` with `f`/`e`/`g` formats a real.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-fmt-{label}-{}", std::process::id()));
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
fn a_spec_says_how_wide_and_which_way() {
    let source = r#"import std;

fn main() int except {
    std.print("[{:>8}][{:<8}][{:^8}]\n", "ab", "cd", "ef")?!;
    std.print("[{:08}][{:08.2f}]\n", 42, 3.14159)?!;
    std.print("[{:x}][{:X}][{:b}][{:o}]\n", 255, 255, 5, 8)?!;
    std.print("[{:.2f}][{}]\n", 3.14159, 3.14159)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("specs");
    assert_eq!(
        scratch.run("specs", source),
        "[      ab][cd      ][   ef   ]\n[00000042][00003.14]\n[ff][FF][101][10]\n[3.14][3.14159]\n"
    );
}

#[test]
fn a_type_renders_itself_with_format() {
    let source = r#"import std;
import std.fmt;

type Point {
    x int;
    y int;
}

fn __format__(this &const Point) String {
    return String("(") + str(this.x) + "," + str(this.y) + ")";
}

fn main() int except {
    p Point = .{1, 2};
    std.print("point {}\n", p)?!;
    std.print("padded [{:>9}]\n", p)?!;
    text String = fmt.format_value(p);
    std.print("direct {}\n", text)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("formattable");
    assert_eq!(
        scratch.run("formattable", source),
        "point (1,2)\npadded [    (1,2)]\ndirect (1,2)\n"
    );
}

#[test]
fn the_format_module_renders_without_a_file() {
    let source = r#"import std;
import std.fmt;

fn main() int except {
    pieces auto = fmt.empty_pieces();
    pieces.push(String("[{}]"));
    pieces.push(fmt.format_value(7));
    line String = fmt.rendered(&pieces)?!;
    std.print("{}\n", line)?!;
    spec String = String("04");
    std.print("[{}]\n", fmt.apply_spec(String("7"), &spec)?!)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("module");
    assert_eq!(scratch.run("module", source), "[7]\n[0007]\n");
}
