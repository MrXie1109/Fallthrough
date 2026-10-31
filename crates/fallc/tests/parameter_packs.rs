//! Function parameter packs: a parameter list of any length.
//!
//! A function may declare `values ...Ts`, a parameter that takes any number of
//! arguments. The call decides how many, and the body is expanded into one
//! parameter per element: `f(values...)` passes them all, and
//! `(call(values), ...)` repeats the call. `sizeof...(values)` is the count.

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
            std::env::temp_dir().join(format!("fallc-packs-{label}-{}", std::process::id()));
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
fn a_parameter_pack_takes_any_number_of_arguments() {
    let source = r#"import std;

template <Ts ...type>
fn count_and_sum(values ...Ts) int {
    total int = 0;
    (total += values, ...);
    return sizeof...(values) as int * 1000 + total;
}

fn main() int except {
    std.out.print(count_and_sum(1, 2, 3), " ", count_and_sum(7), " ", count_and_sum(), "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("any-number");
    assert_eq!(scratch.run("any-number", source), "3006 1007 0\n");
}

#[test]
fn a_comma_fold_repeats_a_call_for_each_element() {
    let source = r#"import std;

type Sink { }

fn __init__(this &Sink) void { }

fn put(this &Sink, value int) void except {
    std.out.print("i", value, " ")??;
}

fn put(this &Sink, value f64) void except {
    std.out.print("f", value, " ")??;
}

template <Ts ...type>
fn emit(sink &Sink, values ...Ts) void except {
    (sink.put(values)??, ...);
}

fn main() int except {
    s Sink;
    emit(s, 1, 2.5, 3)??;
    std.out.print("\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("comma-fold");
    assert_eq!(scratch.run("comma-fold", source), "i1 f2.5 i3 \n");
}

#[test]
fn a_pack_expands_into_another_calls_arguments() {
    let source = r#"import std;

template <Ts ...type>
fn sum_all(values ...Ts) int {
    total int = 0;
    (total += values, ...);
    return total;
}

template <Ts ...type>
fn forward(values ...Ts) int {
    return sum_all(values...);
}

fn main() int except {
    std.out.print(forward(1, 2, 3), " ", forward(9), "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("expansion");
    assert_eq!(scratch.run("expansion", source), "6 9\n");
}

#[test]
fn a_fixed_element_type_is_checked_for_every_element() {
    let source = r#"fn total(values ...int) int {
    sum int = 0;
    (sum += values, ...);
    return sum;
}

fn main() int {
    return total(1, "two");
}
"#;
    // The program does not type check, so it is compiled through the in-memory
    // pipeline, which reports the failure instead of running anything.
    let (outcome, output) =
        fallc::pipeline::compile_text("test.ft", source, &[fallc::cli::EmitKind::Llvm]);
    assert_eq!(outcome, fallc::Outcome::Failure, "{output}");
    assert!(output.contains("type mismatch"), "{output}");
}

#[test]
fn the_output_device_writes_more_than_eight_arguments() {
    let source = r#"import std;

fn main() int except {
    std.out.print(1, " ", 2, " ", 3, " ", 4, " ", 5, " ", 6, " ", 7, " ", 8, " ", 9, " ", 10, "\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("many");
    assert_eq!(scratch.run("many", source), "1 2 3 4 5 6 7 8 9 10\n");
}

#[test]
fn a_method_may_take_a_pack_too() {
    // A device-like value is called with any number of arguments; the family's
    // parameter decides which member exists, and the method's pack takes the
    // rest.
    let source = r#"import std;

enum Mode int { A, B }

template <M Mode>
type Dev { }

template <M Mode>
fn __init__(this &Dev<M>) void { }

@enable_if(M != Mode.B)
export
template <Ts ...type>
fn __call__(this &Dev<M>, values ...Ts) void except {
    (std.out.print(values, " ")??, ...);
}

fn main() int except {
    d Dev<Mode.A>;
    d(1, 2, 3)??;
    std.out.print("\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("method-pack");
    assert_eq!(scratch.run("method-pack", source), "1 2 3 \n");
}

#[test]
fn counting_a_pack_is_reading_it() {
    // `sizeof...(values)` is the only use a body needs in order to mean
    // something: counting the arguments is using them. The unused-parameter
    // warning is for a pack nothing mentions at all, which the next test pins.
    let (outcome, diagnostics) = compile_text(
        "t.ft",
        r#"template <Ts ...type>
fn count(values ...Ts) int {
    return sizeof...(values) as int;
}

fn main() int {
    return count(1, 2);
}
"#,
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert!(!diagnostics.contains("never read"), "{diagnostics}");
}

#[test]
fn a_pack_no_occurrence_mentions_is_never_read() {
    let (outcome, diagnostics) = compile_text(
        "t.ft",
        r#"template <Ts ...type>
fn ignore(values ...Ts) void { }

fn main() int {
    ignore(1, 2);
    return 0;
}
"#,
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Success, "{diagnostics}");
    assert!(
        diagnostics.contains("`values` is never read"),
        "{diagnostics}"
    );
}

#[test]
fn a_pack_must_be_the_last_parameter() {
    // Anything after the pack would have no argument to receive: the pack is
    // the rest of the list.
    let (outcome, diagnostics) = compile_text(
        "t.ft",
        r#"template <Ts ...type>
fn bad(values ...Ts, n int) void { }

fn main() int {
    return 0;
}
"#,
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("a parameter pack must be the last parameter"),
        "{diagnostics}"
    );
}

#[test]
fn a_pack_cannot_have_a_default_argument() {
    let (outcome, diagnostics) = compile_text(
        "t.ft",
        r#"template <Ts ...type>
fn bad(values ...Ts = 1) void { }

fn main() int {
    return 0;
}
"#,
        &[EmitKind::Llvm],
    );
    assert_eq!(outcome, Outcome::Failure, "{diagnostics}");
    assert!(
        diagnostics.contains("a parameter pack cannot have a default argument"),
        "{diagnostics}"
    );
}

#[test]
fn a_pack_expansion_may_pass_a_print_with_a_literal() {
    // The expanded call carries a literal next to the pack element. It used to
    // crash: every expansion numbered its generated spans from zero, so two
    // instantiations filed their answers under the same span and one call's
    // receiver was lowered with another's. The spans are unique for the whole
    // run now.
    let source = r#"import std;

template <Ts ...type>
fn show(values ...Ts) void except {
    (std.out.println("x =", values)??, ...);
}

fn main() int except {
    show(1, 2, 3)??;
    return 0;
}
"#;
    let scratch = Scratch::new("print-in-expansion");
    assert_eq!(
        scratch.run("print-in-expansion", source),
        "x =1\nx =2\nx =3\n"
    );
}
