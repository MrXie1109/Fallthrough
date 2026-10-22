//! `std.encoding`, `std.unicode`, `std.strings` and `std.grid`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-util-{label}-{}", std::process::id()));
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
fn encodings_round_trip_and_match_the_known_text() {
    let source = r#"import std;

fn main() int except {
    data Bytes = Bytes("AB");
    hex String = std.hex_encode(&data);
    std.print("{} {}\n", hex, (std.hex_decode(&hex)?! as String))?!;
    b64 String = std.base64_encode(&data);
    std.print("{} {}\n", b64, (std.base64_decode(&b64)?! as String))?!;
    greeting Bytes = Bytes("hello");
    std.print("{}\n", std.base64_encode(&greeting))?!;
    url String = String("a b/c");
    encoded String = std.percent_encode(&url);
    std.print("{} {}\n", encoded, (std.percent_decode(&encoded)?! as String))?!;
    try {
        bad String = String("x");
        std.hex_decode(&bad)?!;
    } except {
        e &const std.Exception = std.exception();
        std.print("{}\n", e.name())?!;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("encoding");
    assert_eq!(
        scratch.run("encoding", source),
        "4142 AB\nQUI= AB\naGVsbG8=\na%20b%2Fc a b/c\nValueError\n"
    );
}

#[test]
fn utf8_encodes_decodes_and_is_validated() {
    let source = r#"import std;

fn main() int except {
    e String = std.encode(0x00e9u32)?!;
    std.print("{} {} {} {}\n", e.length(), std.valid(&e) as int, std.count(&e), std.decode(&e)?![0])?!;
    hello String = String("héllo");
    std.print("{} {}\n", hello.length(), std.count(&hello))?!;
    bad String = String("x");
    bad.push(0xff as char);
    std.print("{}\n", std.valid(&bad) as int)?!;
    try {
        std.decode(&bad)?!;
    } except {
        e2 &const std.Exception = std.exception();
        std.print("{}\n", e2.name())?!;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("unicode");
    assert_eq!(
        scratch.run("unicode", source),
        "2 1 1 233\n6 5\n0\nValueError\n"
    );
}

#[test]
fn text_helpers_join_repeat_replace_and_pad() {
    let source = r#"import std;

fn main() int except {
    parts Array<String> = Array<String>();
    parts.push(String("x"));
    parts.push(String("y"));
    sep String = String(", ");
    std.print("{}\n", std.join(&parts, &sep))?!;
    ab String = String("ab");
    std.print("{}\n", std.repeat(&ab, 3usize))?!;
    dash String = String("-");
    plus String = String("+");
    base String = String("a-b-c");
    std.print("{}\n", std.replace_all(&base, &dash, &plus))?!;
    seven String = String("7");
    std.print("{} {}\n", std.pad_start(&seven, 3usize, '0' as char), std.pad_end(&seven, 3usize, '.' as char))?!;
    mixed String = String("aBc");
    shout String = String("HeLLo");
    std.print("{} {}\n", std.upper(&mixed), std.lower(&shout))?!;
    text String = String("a\nb\n");
    parts2 Array<String> = std.lines(&text);
    std.print("{}\n", parts2.length())?!;
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(
        scratch.run("strings", source),
        "x, y\nababab\na+b+c\n007 7..\nABC hello\n3\n"
    );
}

#[test]
fn a_grid_is_rows_of_equal_length() {
    let source = r#"import std;

fn main() int except {
    grid Grid<int> = Grid<int>(2usize, 3usize, 0);
    grid.set(1, 2, 7);
    std.print("{} {} {} {}\n", grid.rows(), grid.columns(), grid.length(), grid.at(1, 2))?!;
    std.print("{}\n", grid.at(0, 0))?!;
    filled Grid<int> = Grid<int>(2usize, 2usize, 5);
    filled.fill(9);
    std.print("{}\n", filled.at(1, 1))?!;
    copy Grid<int> = filled;
    std.print("{} {}\n", (copy == filled) as int, copy.at(0, 1))?!;
    return 0;
}
"#;
    let scratch = Scratch::new("grid");
    assert_eq!(
        scratch.run("grid", source),
        "2 3 6 7\n0\n9\n1 9\n"
    );
}
