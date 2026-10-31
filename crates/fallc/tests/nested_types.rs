//! Types declared inside a type, and what they are for.
//!
//! A nested type belongs to the type that declares it and is named through it:
//! `Array<int>.Iterator`. It takes no storage in the value, and it is written
//! inside its owner because that is where the name belongs. A nested *alias* is
//! the whole feature today -- `type Iterator = *T;` -- which is enough to give
//! the standard containers the names the language uses for them.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-nested-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Compiles the source into an executable and runs it.
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
            "`{label}` failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Compiles the source in one file and returns what the compiler printed.
    fn check(&self, _label: &str, source: &str) -> (Outcome, String) {
        compile_text("test.ft", source, &[EmitKind::Llvm])
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.clone().as_path());
    }
}

#[test]
fn a_nested_alias_is_named_through_its_owner() {
    let source = r#"import std;

type Outer {
    x int;
    type Id = int;
    type Handle = *int;
}

fn main() int {
    a Outer.Id = 5;
    p Outer.Handle = &a;
    C.printf("%d %d\n", a, p[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("owner");
    assert_eq!(scratch.run("owner", source), "5 5\n");
}

#[test]
fn a_nested_alias_of_a_family_takes_its_arguments() {
    // `Box<int>.Element` is `int` and `Box<u8>.Element` is `u8`: the nested
    // type belongs to the *member*, not to the family.
    let source = r#"import std;

template <T type>
type Cell {
    v T;
    type Element = T;
    type Ptr = *T;
}

fn main() int {
    a Cell<int>.Element = 7;
    b Cell<u8>.Element = 9;
    p Cell<int>.Ptr = &a;
    C.printf("%d %d %d\n", a, b as int, p[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("family");
    assert_eq!(scratch.run("family", source), "7 9 7\n");
}

#[test]
fn an_arrays_iterator_is_a_pointer_named_in_the_container() {
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(10);
    a.push(20);
    it Array<int>.Iterator = a.begin();
    C.printf("%d %d\n", it[0], it[1]);
    last Array<int>.Iterator = a.end();
    C.printf("%lld\n", last - it);
    return 0;
}
"#;
    let scratch = Scratch::new("array-iterator");
    assert_eq!(scratch.run("array-iterator", source), "10 20\n2\n");
}

#[test]
fn a_reverse_iterator_walks_backwards() {
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    a.push(2);
    a.push(3);
    it Array<int>.ReverseIterator = a.rbegin();
    last Array<int>.ReverseIterator = a.rend();
    for (; it != last; ++it) {
        C.printf("%d ", *it);
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("reverse");
    assert_eq!(scratch.run("reverse", source), "3 2 1 \n");
}

#[test]
fn an_array_is_edited_through_iterators() {
    let source = r#"import std;

fn main() int {
    a Array<int> = Array<int>();
    a.push(1);
    a.push(2);
    a.push(3);
    a.insert(a.begin() + 1, 9);
    for (x int : a) {
        C.printf("%d ", x);
    }
    C.printf("\n");
    gone int = a.erase(a.begin());
    C.printf("%d ", gone);
    for (x int : a) {
        C.printf("%d ", x);
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("edit");
    // The first line is the array after the insert; the second begins with
    // what the erase returned and then shows what is left.
    assert_eq!(scratch.run("edit", source), "1 9 2 3 \n1 9 2 3 \n");
}

#[test]
fn a_string_is_edited_by_index_or_by_position() {
    // The index form is kept and the iterator form is added, because a byte
    // index is what a program usually has and a position is what a loop hands
    // over.
    let source = r#"import std;

fn main() int {
    s String = "hello";
    s.insert(0, '>');
    C.printf("%s\n", s.data());
    s.insert(s.begin() + 6, '!');
    C.printf("%s\n", s.data());
    dropped char = s.erase(s.begin());
    C.printf("%c %s\n", dropped, s.data());
    return 0;
}
"#;
    let scratch = Scratch::new("string-edit");
    assert_eq!(
        scratch.run("string-edit", source),
        ">hello\n>hello!\n> hello!\n"
    );
}

#[test]
fn a_nested_name_that_is_not_declared_is_reported() {
    let source =
        "type Outer {\n    x int;\n}\n\nfn f() int {\n    a Outer.Nope = 1;\n    return a;\n}\n";
    let scratch = Scratch::new("missing");
    let (outcome, output) = scratch.check("missing", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    // A plain outer type's member is a dotted name, and an unknown one is
    // reported as a type that does not exist.
    assert!(output.contains("Outer.Nope"), "{output}");
}

#[test]
fn only_an_alias_may_be_declared_inside_a_body() {
    let source = "type Outer {\n    x int;\n    type Inner {\n        y int;\n    }\n}\n\nfn f() int {\n    return 0;\n}\n";
    let scratch = Scratch::new("struct");
    let (outcome, output) = scratch.check("struct", source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("nested alias"), "{output}");
}
