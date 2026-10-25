//! `std.csv`, `std.toml` and `std.xml`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-ser-{label}-{}", std::process::id()));
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
fn csv_rows_parse_and_write_back() {
    let source = r#"import std;

fn main() int except {
    body String = String("a,b,c\n1,\"hello, world\",3\n\"x\"\"y\",,\n");
    rows Array<Array<String>> = std.parse_rows(&body)??;
    std.print("{} {} {} {}\n", rows.length(), rows[1][1], rows[2][0], rows[2][1].length())??;
    written String = std.write_rows(&rows);
    std.print("{}", written)??;
    try {
        bad String = String("a,\"b");
        std.parse_rows(&bad)??;
    } except {
        e &const std.Exception = std.exception();
        std.print("\n{}\n", e.name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("csv");
    assert_eq!(
        scratch.run("csv", source),
        "3 hello, world x\"y 0\na,b,c\n1,\"hello, world\",3\n\"x\"\"y\",,\nValueError\n"
    );
}

#[test]
fn toml_entries_are_read_by_table_and_key() {
    let source = r##"import std;

fn main() int except {
    body String = String("# config\nname = \"demo\"\ncount = 3\nratio = 1.5\nenabled = true\n\n[server]\nhost = \"localhost\"\nport = 8080\n");
    document TomlDocument = std.parse_toml(&body)??;
    std.print("{}\n", document.length())??;
    server String = String("server");
    port_key String = String("port");
    host_key String = String("host");
    root String = String("");
    ratio_key String = String("ratio");
    std.print("{} {} {}\n",
        document.get(&server, &port_key).value()??.as_int()??,
        document.get(&server, &host_key).value()??.as_string()??,
        document.get(&root, &ratio_key).value()??.as_real()??)??;
    return 0;
}
"##;
    let scratch = Scratch::new("toml");
    assert_eq!(scratch.run("toml", source), "6\n8080 localhost 1.5\n");
}

#[test]
fn xml_elements_attributes_and_text_round_trip() {
    let source = r#"import std;

fn main() int except {
    body String = String("<?xml version=\"1.0\"?>\n<config><name lang=\"en\">demo &amp; test</name><item id=\"1\"/><item id=\"2\"/></config>");
    document XmlDocument = std.parse_xml(&body)??;
    root XmlValue = document.root();
    std.print("{} {} {}\n", root.name(), root.child_count(), root.child(0)??.text())??;
    lang String = String("lang");
    std.print("{}\n", root.child(0)??.attribute(&lang).value()??)??;
    written String = std.serialize(&root)??;
    std.print("{}", written)??;
    return 0;
}
"#;
    let scratch = Scratch::new("xml");
    assert_eq!(
        scratch.run("xml", source),
        "config 3 demo & test\nen\n<config><name lang=\"en\">demo &amp; test</name><item id=\"1\"/><item id=\"2\"/></config>"
    );
}
