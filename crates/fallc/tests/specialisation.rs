//! Template specialisation of a type family.
//!
//! `template <> type Crate<int> { .. }` replaces the member `Crate<int>`;
//! `template <T type> type Crate<*T> { .. }` replaces every member whose
//! argument is a pointer. When several match, the most specific wins, and two
//! that neither contains the other are ambiguous rather than resolved by
//! declaration order.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-spec-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, source).expect("the source can be written");
        path
    }

    /// Compiles the named files as one program and runs it.
    fn run_files(&self, sources: &[PathBuf], output: &str) -> (bool, String) {
        let executable = self.directory.join(output);
        let mut args: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
        args.push("-o".to_owned());
        args.push(executable.display().to_string());
        let options = Options::parse(&args).expect("the arguments are well formed");
        if run(&options) != std::process::ExitCode::SUCCESS {
            return (false, String::new());
        }
        let result = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        (true, String::from_utf8_lossy(&result.stdout).into_owned())
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
fn a_full_specialisation_replaces_the_member() {
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

template <>
type Crate<int> {
    v int;
    extra int;
}

fn main() int {
    a Crate<int> = .{1, 2};
    b Crate<double> = .{1.5};
    C.printf("%d %d %.1f\n", a.v, a.extra, b.v);
    return 0;
}
"#;
    let scratch = Scratch::new("full");
    assert_eq!(scratch.run("full", source), "1 2 1.5\n");
}

#[test]
fn a_partial_specialisation_matches_a_pointer() {
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

template <T type>
type Crate<*T> {
    ptr *T;
    len int;
}

fn main() int {
    a Crate<int> = .{7};
    b Crate<*double> = .{nullptr, 3};
    C.printf("%d %d\n", a.v, b.len);
    return 0;
}
"#;
    let scratch = Scratch::new("partial");
    assert_eq!(scratch.run("partial", source), "7 3\n");
}

#[test]
fn the_most_specific_specialisation_wins() {
    // `Crate<*int>` matches both the partial pointer pattern and the full
    // `int` one, and the full one is more specific.
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

template <T type>
type Crate<*T> {
    ptr *T;
    len int;
}

template <>
type Crate<*int> {
    ptr *int;
    count int;
}

fn main() int {
    a Crate<int> = .{7};
    c *int = &a.v;
    d Crate<*int> = .{c, 9};
    C.printf("%d %d\n", a.v, d.count);
    return 0;
}
"#;
    let scratch = Scratch::new("specific");
    assert_eq!(scratch.run("specific", source), "7 9\n");
}

#[test]
fn two_specialisations_that_neither_contains_the_other_are_ambiguous() {
    let source = r#"import std;

template <T type, U type>
type Pair {
    a T;
    b U;
}

template <T type>
type Pair<T, int> {
    first T;
    second int;
}

template <T type>
type Pair<int, T> {
    first int;
    second T;
}

fn main() int {
    p Pair<int, int> = .{1, 2};
    C.printf("%d\n", p.first + p.second);
    return 0;
}
"#;
    let (outcome, diagnostics) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(diagnostics.contains("is ambiguous"), "{diagnostics}");
}

#[test]
fn a_family_method_sees_the_specialised_member() {
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

fn doubled(this &const Crate<T>) T {
    return this.v + this.v;
}

template <>
type Crate<int> {
    v int;
    extra int;
}

fn main() int {
    a Crate<int> = .{20, 1};
    b Crate<double> = .{1.5};
    C.printf("%d %.1f\n", a.doubled() + a.extra, b.doubled());
    return 0;
}
"#;
    let scratch = Scratch::new("methods");
    assert_eq!(scratch.run("methods", source), "41 3.0\n");
}

#[test]
fn a_specialisation_declared_in_another_module_is_used() {
    // The family, its specialisations and a method written against the
    // family's parameter all live in `crates.ftm`; the program only imports
    // them.
    let scratch = Scratch::new("module");
    scratch.write(
        "crates.ftm",
        r#"export
template <T type>
type Crate {
    v T;
}

template <>
type Crate<int> {
    v int;
    extra int;
}

template <T type>
type Crate<*T> {
    ptr *T;
    len int;
}

export fn tag(this &const Crate<T>) int {
    return this.extra;
}
"#,
    );
    let main = scratch.write(
        "main.ft",
        r#"import std;
import crates;

fn main() int {
    a crates.Crate<int> = .{1, 2};
    b crates.Crate<*double> = .{nullptr, 3};
    c crates.Crate<double> = .{1.5};
    C.printf("%d %d %.1f %d %d\n", a.v, a.extra, c.v, b.len, a.tag());
    return 0;
}
"#,
    );
    let (built, output) = scratch.run_files(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "1 2 1.5 3 2\n");
}

#[test]
fn a_method_written_for_one_concrete_member_crosses_a_module() {
    // `twice` is written against `Crate<int>`, so it belongs to that member
    // rather than being a template. Its symbol is the member's, and an
    // importing module has to build the same one: the written `Crate<int>`
    // has to resolve to the member `Crate<i32>` before the symbol is derived,
    // or the caller asks for `Crate` and the link fails.
    let scratch = Scratch::new("concrete-member");
    scratch.write(
        "crates.ftm",
        r#"export
template <T type>
type Crate {
    v T;
}

template <>
type Crate<int> {
    v int;
    extra int;
}

export fn twice(this &const Crate<int>) int {
    return this.v + this.extra;
}
"#,
    );
    let main = scratch.write(
        "main.ft",
        r#"import std;
import crates;

fn main() int {
    a crates.Crate<int> = .{1, 2};
    C.printf("%d\n", a.twice());
    return 0;
}
"#,
    );
    let (built, output) = scratch.run_files(&[main], "app");
    assert!(built, "the program did not compile");
    assert_eq!(output, "3\n");
}

#[test]
fn a_member_that_matches_no_specialisation_uses_the_generic_one() {
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

template <>
type Crate<int> {
    v int;
    extra int;
}

fn main() int {
    b Crate<u8> = .{9u8};
    C.printf("%d\n", b.v as int);
    return 0;
}
"#;
    let scratch = Scratch::new("fallback");
    assert_eq!(scratch.run("fallback", source), "9\n");
}

#[test]
fn a_family_parameter_is_deduced_from_its_member() {
    // `Crate<T>` in a parameter says what `T` is when the argument is a member
    // of that family, and the argument may be a value, a reference or a
    // pointer. Only a bare parameter name was deduced, so every call had to
    // write `first<int>(c)` -- and the doc example, which does not, did not
    // compile.
    let source = r#"import C;

template <T type>
type Crate {
    value T;
}

template <T type>
fn by_value(c Crate<T>) T {
    return c.value;
}

template <T type>
fn by_reference(c &const Crate<T>) T {
    return c.value;
}

template <T type>
fn by_pointer(c *const Crate<T>) T {
    return c->value;
}

fn main() int {
    c Crate<int> = .{5};
    C.printf("%d %d %d\n", by_value(c), by_reference(c), by_pointer(&c));
    return 0;
}
"#;
    let scratch = Scratch::new("deduce-family");
    let output = scratch.run("deduce_family", source);
    assert_eq!(output.trim(), "5 5 5");
}
