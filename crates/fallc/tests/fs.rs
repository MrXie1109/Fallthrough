//! Paths: the pieces of a path, and joining them.
//!
//! A `Path` is text with a few questions a program can ask of it: what directory
//! it is in, what it is called, what extension it has. The questions are answered
//! without touching the filesystem, and `exists` is the one that asks it.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-fs-{label}-{}", std::process::id()));
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
            .current_dir(&self.directory)
            .output()
            .expect("the program can be run");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn a_path_knows_its_directory_name_and_extension() {
    let source = r#"import std;

fn main() int {
    p Path = Path("/tmp/notes/readme.txt");
    C.printf("%s\n%s\n%s\n%s\n", p.data().data(), p.directory().data().data(), p.file_name().data(), p.extension().data());
    C.printf("%s\n", Path("/tmp/notes").file_name().data());
    C.printf("%s|%s\n", Path("/").directory().data().data(), Path("readme").directory().data().data());
    C.printf("none %d\n", Path("readme").extension().empty() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("pieces");
    assert_eq!(
        scratch.run("pieces", source),
        "/tmp/notes/readme.txt\n/tmp/notes\nreadme.txt\ntxt\nnotes\n/|.\nnone 1\n"
    );
}

#[test]
fn joining_a_path_puts_one_separator_between_the_parts() {
    // A directory that already ends in a separator does not get a second one.
    let source = r#"import std;

fn main() int {
    base Path = Path("/tmp");
    name Path = Path("file.txt");
    C.printf("%s\n", base.join(name).data().data());
    trailing Path = Path("/tmp/");
    C.printf("%s\n", trailing.join(name).data().data());
    C.printf("same %d\n", (Path("a") == Path("a")) as int);
    C.printf("different %d\n", (Path("a") != Path("b")) as int);
    C.printf("cloned %s\n", Path("x").clone().data().data());
    return 0;
}
"#;
    let scratch = Scratch::new("join");
    assert_eq!(
        scratch.run("join", source),
        "/tmp/file.txt\n/tmp/file.txt\nsame 1\ndifferent 1\ncloned x\n"
    );
}

#[test]
fn exists_asks_the_filesystem_and_a_path_prints_as_its_text() {
    let source = r#"import std;

fn main() int except {
    C.printf("missing %d\n", Path("nothing-here.txt").exists() as int);
    sink std.File = std.File("here.txt", std.OpenMode.WRITE)?!;
    sink.write("x")?!;
    sink.close()?!;
    C.printf("present %d\n", Path("here.txt").exists() as int);
    std.out.print("path: ", Path("here.txt"), "\n")?!;
    return 0;
}
"#;
    let scratch = Scratch::new("exists");
    assert_eq!(
        scratch.run("exists", source),
        "missing 0\npresent 1\npath: here.txt\n"
    );
}

#[test]
fn a_path_opens_the_file_it_names() {
    // The path and the file in one step: `p.file(mode)` is the constructor with
    // the path's own text, so the file is written and read back through the
    // path that named it.
    let source = r#"import std;

fn main() int except {
    made Path = Path("written.txt");
    f std.File = made.file(std.OpenMode.WRITE)?!;
    f.write("from a path\n")?!;
    f.close()?!;

    again std.File = made.file(std.OpenMode.READ)?!;
    line std.String = std.String();
    again.getline(line)?!;
    C.printf("%s", line.data());
    again.close()?!;
    return 0;
}
"#;
    let scratch = Scratch::new("path-file");
    assert_eq!(scratch.run("path-file", source), "from a path");
}

#[test]
fn a_path_handles_the_awkward_edge_cases() {
    // A trailing separator names the directory it follows, a leading dot names
    // a hidden file rather than an extension, and joining never produces two
    // separators.
    let source = r#"import std;

fn main() int {
    C.printf("name %s\n", Path("a/b/").file_name().data());
    C.printf("root %s\n", Path("/").file_name().data());
    C.printf("hidden %s\n", Path(".bashrc").extension().data());
    C.printf("dotted %s\n", Path("a.b/c").extension().data());
    C.printf("join %s\n", Path("a/").join(Path("/b")).text());
    C.printf("join2 %s\n", Path("a").join(Path("b")).text());
    return 0;
}
"#;
    let scratch = Scratch::new("path-edges");
    assert_eq!(
        scratch.run("path-edges", source),
        "name b\nroot \nhidden \ndotted \njoin a/b\njoin2 a/b\n"
    );
}

#[test]
fn a_directory_can_be_made_read_and_measured() {
    // `mkdir` makes one level; `read_dir` lists the names without `.`/`..`;
    // `size`, `is_file` and `is_directory` ask the system about an entry.
    let source = r#"import std;

fn main() int except {
    home Path = Path("/tmp");
    dir Path = home.join(Path("fallc-fs-readdir"));
    dir.remove();
    C.printf("mkdir %d\n", dir.mkdir() as int);
    C.printf("directory %d\n", dir.is_directory() as int);
    inner Path = dir.join(Path("a.txt"));
    f std.File = inner.file(std.OpenMode.WRITE)?!;
    f.write("hello")?!;
    f.close()?!;
    C.printf("file %d size %d\n", inner.is_file() as int, inner.size() as int);
    names Array<String> = dir.read_dir()?!;
    C.printf("names %d\n", names.length() as int);
    for (name &const String : names) {
        C.printf("%s\n", name.data());
    }
    inner.remove();
    dir.remove();
    return 0;
}
"#;
    let scratch = Scratch::new("read-dir");
    assert_eq!(
        scratch.run("read-dir", source),
        "mkdir 1\ndirectory 1\nfile 1 size 5\nnames 1\na.txt\n"
    );
}
