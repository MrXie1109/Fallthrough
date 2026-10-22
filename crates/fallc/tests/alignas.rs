//! `@align(N)`: the alignment a field is *required* to have.
//!
//! A type's layout is its fields' sizes and alignments, and `alignas` is the
//! one way to change the alignment a field would get on its own. It can only
//! raise it: the field's own alignment is a property of its type, and a request
//! below it would mean packing a type more tightly than it lays itself out.
//!
//! The tests check both halves of "the layout honours it": the numbers
//! `sizeof`/`alignof` report, and the addresses the generated code reaches. A
//! size that disagreed with the code's offsets would still print a plausible
//! number, which is why the programs that run are here too.

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
            std::env::temp_dir().join(format!("fallc-align-{label}-{}", std::process::id()));
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

    /// Compiles the source, which must be refused.
    fn refused(&self, label: &str, source: &str) -> String {
        let (outcome, printed) = fallc::pipeline::compile_text(
            &format!("{label}.ft"),
            source,
            &[fallc::cli::EmitKind::Llvm],
        );
        assert_eq!(outcome, fallc::Outcome::Failure, "`{label}` compiled");
        printed
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_requested_alignment_raises_the_field_and_the_type() {
    // `b` would sit at offset 4 in `Plain`; asked for sixteen, it starts at 16
    // and the type is as aligned as it is. Everything after it moves, and the
    // whole is padded to a multiple of the type's alignment.
    let source = r#"import std;

type Plain {
    a u8;
    b int;
}

type Padded {
    a u8;
    b int @align(16);
    c u8;
}

fn main() int {
    C.printf("%lu %lu\n", sizeof(Plain), alignof(Plain));
    C.printf("%lu %lu\n", sizeof(Padded), alignof(Padded));
    return 0;
}
"#;
    let scratch = Scratch::new("raise");
    assert_eq!(scratch.run("raise", source), "8 4\n32 16\n");
}

#[test]
fn an_alignment_equal_to_the_natural_one_changes_nothing() {
    // `int` is already four-byte aligned, so asking for four is a request the
    // type already satisfies: the layout is what it would have been.
    let source = r#"import std;

type Plain {
    a u8;
    b int;
}

type Same {
    a u8;
    b int @align(4);
}

fn main() int {
    C.printf("%lu %lu\n", sizeof(Same), alignof(Same));
    C.printf("%lu %lu\n", sizeof(Plain), alignof(Plain));
    return 0;
}
"#;
    let scratch = Scratch::new("same");
    assert_eq!(scratch.run("same", source), "8 4\n8 4\n");
}

#[test]
fn an_aligned_type_carries_its_alignment_into_everything_that_holds_it() {
    // A type is aligned to its most-aligned field, so a field of an
    // over-aligned type raises *this* type too -- and an array of it is sized
    // by the padded size, not the sum of the members.
    let source = r#"import std;

type Padded {
    a u8;
    b int @align(16);
}

type Held {
    first u8;
    inner Padded;
    last u8;
}

fn main() int {
    C.printf("held %lu %lu\n", sizeof(Held), alignof(Held));
    C.printf("array %lu %lu\n", sizeof([3]Padded), alignof([3]Padded));
    C.printf("wide %lu %lu\n", sizeof(alignas_holder), alignof(alignas_holder));
    return 0;
}

type alignas_holder {
    first int @align(32);
    second int;
}
"#;
    let scratch = Scratch::new("carry");
    assert_eq!(
        scratch.run("carry", source),
        // `first` at 0, `inner` at 16 (32 bytes), `last` at 48, padded to 64.
        "held 64 16\narray 96 16\nwide 32 32\n"
    );
}

#[test]
fn a_field_is_reached_at_the_offset_the_layout_gave_it() {
    // The numbers above could be right while the code reached the wrong
    // members. Writing one element of an array and reading every field back
    // checks the addresses the code generator uses: an offset that disagreed
    // with `sizeof` would make the second element overlap the first.
    let source = r#"import std;

type Padded {
    a u8;
    b int @align(16);
    c u8;
}

fn main() int {
    arr [3]Padded;
    for (i usize = 0; i < 3; i += 1) {
        arr[i].a = 1 as u8;
        arr[i].b = (i * 10) as int;
        arr[i].c = 2 as u8;
    }
    for (i usize = 0; i < 3; i += 1) {
        C.printf("%d %d %d\n", arr[i].a as int, arr[i].b, arr[i].c as int);
    }
    return 0;
}
"#;
    let scratch = Scratch::new("offset");
    assert_eq!(scratch.run("offset", source), "1 0 2\n1 10 2\n1 20 2\n");
}

#[test]
fn an_alignment_on_a_union_member_raises_the_union() {
    // The members share one address, so an alignment asked for by any of them
    // is asked for by the object they share.
    let source = r#"import std;

union Value {
    i int;
    b byte @align(16);
}

fn main() int {
    v Value;
    v.i = 65;
    C.printf("%lu %lu %d %d\n", sizeof(Value), alignof(Value), v.b as int, v.i);
    return 0;
}
"#;
    let scratch = Scratch::new("union");
    assert_eq!(scratch.run("union", source), "16 16 65 65\n");
}

#[test]
fn an_alignment_that_is_not_a_power_of_two_is_refused() {
    let source = "type A {\n    a int @align(3);\n}\nfn main() int {\n    return 0;\n}\n";
    let scratch = Scratch::new("power");
    let printed = scratch.refused("power", source);
    assert!(printed.contains("S0138"), "{printed}");
    assert!(printed.contains("not a power of two"), "{printed}");
}

#[test]
fn an_alignment_weaker_than_the_type_is_refused() {
    // `alignas` raises; it never packs. `int` is four-byte aligned, so two
    // would ask for a field the type cannot lay itself out as.
    let source = "type A {\n    a int @align(2);\n}\nfn main() int {\n    return 0;\n}\n";
    let scratch = Scratch::new("weak");
    let printed = scratch.refused("weak", source);
    assert!(printed.contains("S0139"), "{printed}");
    assert!(printed.contains("weaker than"), "{printed}");
}

#[test]
fn an_alignment_must_be_a_compile_time_constant() {
    let source = "fn f(n int) int {\n    return n;\n}\n\ntype A {\n    a int @align(f(2));\n}\n\nfn main() int {\n    return 0;\n}\n";
    let scratch = Scratch::new("constant");
    let printed = scratch.refused("constant", source);
    assert!(printed.contains("S0137"), "{printed}");
    assert!(printed.contains("needs a value the compiler knows"), "{printed}");
}

#[test]
fn a_layout_without_an_alignment_is_what_it_always_was() {
    // The natural rule is untouched: a field starts at the next multiple of its
    // own alignment and the type is padded to its own.
    let source = r#"import std;

type Pair {
    a u8;
    b int;
}

type Other {
    a u8;
    b u8;
    c int;
}

fn main() int {
    C.printf("%lu %lu %lu %lu\n", sizeof(Pair), alignof(Pair), sizeof(Other), alignof(Other));
    return 0;
}
"#;
    let scratch = Scratch::new("natural");
    assert_eq!(scratch.run("natural", source), "8 4 8 4\n");
}
