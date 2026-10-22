//! Methods of a templated type (a type family).
//!
//! `template <T type> type Crate { v T; }` declares a family, and a function
//! whose receiver is `&Crate<T>` is a *method of the family* rather than a
//! function: it is checked and emitted once per member, because `T` means a
//! different type for each. These tests pin what a program can observe about
//! that -- which is mostly that one member's body is not another's.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-family-{label}-{}", std::process::id()));
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
fn a_family_method_returns_each_member_s_type() {
    // The method's return type is `T`, so the `int` member must return an
    // `int` and the `double` member a `double`. Reading the written type again
    // in code generation gave both whichever member was checked last, and the
    // IR verifier rejected the result.
    let source = r#"import std;

template <T type>
type Crate {
    v T;
}

fn twice(this &const Crate<T>) T {
    return this.v + this.v;
}

fn main() int {
    a Crate<int> = .{21};
    b Crate<double> = .{2.5};
    C.printf("%d %.1f\n", a.twice(), b.twice());
    return 0;
}
"#;
    let scratch = Scratch::new("returns");
    assert_eq!(scratch.run("returns", source), "42 5.0\n");
}

#[test]
fn a_family_method_is_checked_against_its_own_member() {
    // Two members, two bodies: the `int` one uses integer arithmetic and the
    // `double` one floating-point arithmetic, and both are correct.
    let source = r#"import std;

template <T type>
type Pair {
    first T;
    second T;
}

fn sum(this &const Pair<T>) T {
    return this.first + this.second;
}

fn main() int {
    a Pair<int> = .{20, 22};
    b Pair<double> = .{1.5, 2.5};
    C.printf("%d %.1f\n", a.sum(), b.sum());
    return 0;
}
"#;
    let scratch = Scratch::new("checked");
    assert_eq!(scratch.run("checked", source), "42 4.0\n");
}

#[test]
fn a_family_method_may_call_another_method_of_the_same_member() {
    let source = r#"import std;

template <T type>
type Pair {
    first T;
    second T;
}

fn sum(this &const Pair<T>) T {
    return this.first + this.second;
}

fn doubled(this &const Pair<T>) T {
    return this.sum() + this.sum();
}

fn main() int {
    a Pair<int> = .{20, 1};
    b Pair<double> = .{1.0, 1.0};
    C.printf("%d %.1f\n", a.doubled(), b.doubled());
    return 0;
}
"#;
    let scratch = Scratch::new("calls");
    assert_eq!(scratch.run("calls", source), "42 4.0\n");
}

#[test]
fn a_family_may_be_callable_and_reads_its_reference_parameters() {
    // A comparator is the family a container takes: `Less<int>`'s `__call__`
    // takes two `&const int` and compares them. Two things have to hold for
    // that to work, and both were broken: the family's parameters are what the
    // call substitutes (`E` is `int`, not a name), and a reference operand is
    // read through -- including the right-hand one, which the checker used to
    // leave as the address.
    let source = r#"import std;

template <E type>
type Less {}

template <E type>
fn __call__(this &const Less<E>, a &const E, b &const E) bool {
    return a < b;
}

template <E type>
type Greater {}

template <E type>
fn __call__(this &const Greater<E>, a &const E, b &const E) bool {
    return a > b;
}

fn main() int {
    less Less<int> = .{};
    greater Greater<int> = .{};
    x int = 3;
    y int = 7;
    C.printf("%d %d %d %d\n",
        less(x, y) as int, less(y, x) as int,
        greater(x, y) as int, greater(y, x) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("callable");
    assert_eq!(scratch.run("callable", source), "1 0 0 1\n");
}
