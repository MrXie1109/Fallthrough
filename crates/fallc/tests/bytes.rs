//! `std.Bytes`: a sequence of bytes, the untyped sibling of `String`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-bytes-{label}-{}", std::process::id()));
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
fn bytes_are_stored_and_grown_one_at_a_time() {
    let source = r#"import std;

fn main() int except {
    data Bytes = Bytes("hi");
    data.push(33 as byte);
    data.push(10 as byte);
    empty_now bool = data.empty();
    std.print("{} {} {}\n", data.length(), empty_now as int, data[0])?!;
    data.resize(5, 0 as byte);
    std.print("{} {}\n", data.length(), data[4])?!;
    data.clear();
    cleared bool = data.empty();
    std.print("{}\n", cleared as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("grow");
    assert_eq!(
        scratch.run("grow", source),
        "4 0 104\n5 0\n1\n"
    );
}

#[test]
fn bytes_and_text_convert_both_ways() {
    let source = r#"import std;

fn main() int except {
    text String = String("hello");
    as_bytes Bytes = text as Bytes;
    as_bytes.push(33 as byte);
    back String = as_bytes as String;
    std.print("{}\n", back)?!;
    original Bytes = Bytes("hello");
    same Bytes = Bytes("hello");
    std.print("{} {}\n", (original == same) as int, (original == as_bytes) as int)?!;
    copy Bytes = original;
    copy.push(64 as byte);
    std.print("{} {}\n", original.length(), copy.length())?!;
    return 0;
}
"#;
    let scratch = Scratch::new("convert");
    assert_eq!(
        scratch.run("convert", source),
        "hello!\n1 0\n5 6\n"
    );
}

#[test]
fn a_byte_sequence_can_be_walked() {
    let source = r#"import std;

fn main() int except {
    data Bytes = Bytes("AB");
    for (value &const u8 : data) {
        std.print("{:x} ", value)?!;
    }
    std.print("\n")?!;
    return 0;
}
"#;
    let scratch = Scratch::new("walk");
    assert_eq!(scratch.run("walk", source), "41 42 \n");
}

#[test]
fn a_call_before_a_template_call_does_not_borrow_its_index() {
    // The constant index of the tree being checked is not the index of the
    // template's body. Reading the caller's arguments through the body's index
    // folded an unrelated expression: `data.length()` became `-1`. This mixes
    // a method call, a bool cast and a reference argument in one `print`, which
    // is the shape that found it.
    let source = r#"import std;

fn main() int except {
    data Bytes = Bytes("hi");
    data.push(33 as byte);
    data.push(10 as byte);
    empty_now bool = data.empty();
    std.print("bytes {} {} {}\n", data.length(), empty_now as int, data[0])?!;
    values Array<int> = Array<int>();
    values.push(7);
    values.push(8);
    std.print("array {} {} {}\n", values.length(), empty_now as int, values[1])?!;
    text String = String("hi");
    std.print("text {} {} {}\n", text.length(), empty_now as int, text[0])?!;
    return 0;
}
"#;
    let scratch = Scratch::new("index");
    assert_eq!(
        scratch.run("index", source),
        "bytes 4 0 104\narray 2 0 8\ntext 2 0 h\n"
    );
}
