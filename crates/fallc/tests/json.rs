//! `std.json`: a parsed document, values into it, and a writer.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-json-{label}-{}", std::process::id()));
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
fn a_document_is_parsed_and_read() {
    let source = r#"import std;

fn main() int except {
    body String = String("{\"name\": \"ada\", \"age\": 36, \"tags\": [1, 2, 3], \"ok\": true, \"none\": null}");
    document JsonDocument = std.parse(&body)?!;
    root JsonValue = document.root();
    ok_key String = String("ok");
    std.print("{} {} {}\n", root.kind() as int, root.length(), root.member(&ok_key).value()?!.as_bool() as int)?!;
    name_key String = String("name");
    std.print("{}\n", root.member(&name_key).value()?!.as_text()?!)?!;
    tags_key String = String("tags");
    tags JsonValue = root.member(&tags_key).value()?!;
    sum int = 0;
    index usize = 0;
    while (index < tags.length()) {
        element JsonValue = tags.at(index)?!;
        sum += element.as_number()?! as int;
        index += 1;
    }
    none_key String = String("none");
    std.print("{} {}\n", sum, root.member(&none_key).value()?!.is_null() as int)?!;
    missing_key String = String("missing");
    std.print("{}\n", root.member(&missing_key).present() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("read");
    assert_eq!(
        scratch.run("read", source),
        "5 5 1\nada\n6 1\n0\n"
    );
}

#[test]
fn a_document_writes_back_the_same_json() {
    let source = r#"import std;

fn main() int except {
    body String = String("{\"name\":\"ada\",\"age\":36,\"tags\":[1,2,3],\"ok\":true,\"none\":null}");
    document JsonDocument = std.parse(&body)?!;
    root JsonValue = document.root();
    written String = std.serialize(&root)?!;
    std.print("{}\n", written)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("write");
    assert_eq!(
        scratch.run("write", source),
        "{\"name\":\"ada\",\"age\":36,\"tags\":[1,2,3],\"ok\":true,\"none\":null}\n"
    );
}

#[test]
fn malformed_json_and_wrong_accessors_raise() {
    let source = r#"import std;

fn main() int except {
    good String = String("[1, 2]");
    document JsonDocument = std.parse(&good)?!;
    root JsonValue = document.root();
    try {
        std.print("never {}\n", root.as_text()?!)?!;
    } except {
        e &const std.Exception = std.exception();
        std.print("{}\n", e.name())?!;
    }
    bad String = String("[1,");
    try {
        std.parse(&bad)?!;
    } except {
        e2 &const std.Exception = std.exception();
        std.print("{}\n", e2.name())?!;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("errors");
    assert_eq!(scratch.run("errors", source), "ValueError\nValueError\n");
}
