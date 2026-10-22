//! `BasicString<Char>`: the string shape over a character type.
//!
//! `String` is the `char` specialisation the rest of the library uses and keeps
//! small strings inside the object; `BasicString<Char>` is the plain form, with
//! one buffer, and `String16`/`String32` are the same operations over wider code
//! units.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-bstr-{label}-{}", std::process::id()));
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
fn a_character_string_grows_and_compares() {
    let source = r#"import std;

fn main() int except {
    text BasicString<char> = BasicString<char>("hello");
    text.push('!');
    std.print("{} {} {}\n", text.length(), text[0], text)?!;
    other BasicString<char> = BasicString<char>(" world");
    joined BasicString<char> = text + other;
    std.print("{} {}\n", joined.length(), joined)?!;
    std.print("cmp {} {} {}\n", (text == text) as int, (text == other) as int, (other < text) as int)?!;
    std.print("find {} {}\n", text.find('l'), text.contains('z') as int)?!;
    text.clear();
    std.print("cleared {} {}\n", text.empty() as int, text.length())?!;
    return 0;
}
"#;
    let scratch = Scratch::new("char");
    assert_eq!(
        scratch.run("char", source),
        "6 h hello!\n12 hello! world\ncmp 1 0 1\nfind 2 0\ncleared 1 0\n"
    );
}

#[test]
fn a_wide_string_is_the_same_shape() {
    let source = r#"import std;

fn main() int except {
    units Array<u16> = Array<u16>();
    units.push(72 as u16);
    units.push(105 as u16);
    wide String16 = String16(units);
    std.print("{} {} {}\n", wide.length(), wide[0], wide[1])?!;
    wide.push(33 as u16);
    std.print("{} {}\n", wide.length(), wide[2])?!;
    empty32 String32 = String32();
    std.print("{} {}\n", empty32.empty() as int, empty32.length())?!;
    return 0;
}
"#;
    let scratch = Scratch::new("wide");
    assert_eq!(
        scratch.run("wide", source),
        "2 72 105\n3 33\n1 0\n"
    );
}

#[test]
fn a_local_string_is_copied_and_moved() {
    let source = r#"import std;

fn main() int except {
    original BasicString<char> = BasicString<char>("abc");
    copy BasicString<char> = original;
    copy.push('d');
    std.print("{} {} {}\n", original.length(), copy.length(), copy)?!;
    moved BasicString<char> = std.move(copy);
    std.print("{} {}\n", moved.length(), moved)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("moves");
    assert_eq!(
        scratch.run("moves", source),
        "3 4 abcd\n4 abcd\n"
    );
}
