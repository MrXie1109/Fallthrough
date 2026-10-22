//! Indexing arrays, and indexing them *through a reference*.
//!
//! `c &[N][N]double` names an array, so `c[i]` is a row of that array and
//! `c[i][j]` is one element of it. Both sides of an assignment work: the
//! checker gives the subscript the element's type, and code generation counts
//! from the address the reference holds.
//!
//! `&array` is the address of the array, not the array. Reading the array
//! first produced a value of half a megabyte for a 256x256 matrix, and LLVM 18
//! dies selecting an aggregate that large, so the difference between a place
//! and a value is what these tests pin down.

use std::path::PathBuf;

use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-index-{label}-{}", std::process::id()));
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
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn a_reference_is_indexed_through_itself() {
    // The row of a 2-D array is reached through a reference to the whole array.
    // Both the write and the read go through `c`, which holds an address.
    let source = r#"import std;

N const usize = 3;

fn fill(c &[N][N]int) void {
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            c[i][j] = (i * 10 + j) as int;
        }
    }
}

fn total(c &const [N][N]int) int {
    s int = 0;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            s += c[i][j];
        }
    }
    return s;
}

fn main() int {
    m [N][N]int;
    fill(&m);
    C.printf("%d\n", total(&m));
    return 0;
}
"#;
    let scratch = Scratch::new("two");
    // 0+1+2 + 10+11+12 + 20+21+22
    assert_eq!(scratch.run("two", source), "99\n");
}

#[test]
fn a_reference_to_one_array_is_indexed_like_the_array() {
    let source = r#"import std;

N const usize = 4;

fn read(row &const [N]int, index usize) int {
    return row[index];
}

fn write(row &[N]int, index usize, value int) void {
    row[index] = value;
}

fn main() int {
    a [N]int;
    for (i usize = 0; i < N; i += 1) {
        write(&a, i, (i * i) as int);
    }
    C.printf("%d %d %d\n", read(&a, 0), read(&a, 2), read(&a, 3));
    return 0;
}
"#;
    let scratch = Scratch::new("row");
    assert_eq!(scratch.run("row", source), "0 4 9\n");
}

#[test]
fn a_compound_assignment_reaches_the_element_a_reference_names() {
    let source = r#"import std;

N const usize = 2;

fn bump(c &[N][N]int) void {
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            c[i][j] += 1;
        }
    }
}

fn main() int {
    m [N][N]int;
    m[1][1] = 41;
    bump(&m);
    C.printf("%d\n", m[1][1]);
    return 0;
}
"#;
    let scratch = Scratch::new("compound");
    assert_eq!(scratch.run("compound", source), "42\n");
}

#[test]
fn indexing_an_array_does_not_read_the_whole_array() {
    // `m[i]` starts from `m`'s address. Reading `m` as a value first was both
    // wasteful and fatal for a large array: LLVM cannot select an aggregate
    // value of that size.
    let (outcome, ir) = compile_text(
        "t.ft",
        "N const usize = 4;\n\
         \n\
         fn main() int {\n\
         \x20   m [N][N]int;\n\
         \x20   m[1][2] = 7;\n\
         \x20   return m[1][2];\n\
         }\n",
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, fallc::Outcome::Success, "{ir}");
    assert!(
        !ir.contains("load [4 x [4 x i32]]"),
        "the array was read as a value:\n{ir}"
    );
}

#[test]
fn taking_the_address_of_an_array_does_not_read_it() {
    let (outcome, ir) = compile_text(
        "t.ft",
        "N const usize = 4;\n\
         \n\
         fn clear(c &[N][N]int) void {\n\
         \x20   c[0][0] = 0;\n\
         }\n\
         \n\
         fn main() int {\n\
         \x20   m [N][N]int;\n\
         \x20   clear(&m);\n\
         \x20   return 0;\n\
         }\n",
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, fallc::Outcome::Success, "{ir}");
    assert!(
        !ir.contains("load [4 x [4 x i32]]"),
        "`&m` read the array it took the address of:\n{ir}"
    );
}
