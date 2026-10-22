//! Returning aggregates, and passing them by value.
//!
//! An aggregate too large for LLVM's SSA instruction selection -- every array,
//! and a record over the size limit -- travels through a hidden out-pointer
//! instead of a return value. These tests are programs that are compiled and
//! run, because the point of the convention is what the program does: a value
//! that is copied into a local, indexed, summed, returned again, or assigned
//! over an existing local must arrive intact.
//!
//! Small records are the other half of the rule. They are still returned by
//! value, so the tests cover one too: the convention is chosen by type and
//! size, and both branches have to work.

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
            std::env::temp_dir().join(format!("fallc-aggregate-{label}-{}", std::process::id()));
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn a_small_record_is_still_returned_by_value() {
    // A record below the size limit keeps the ordinary convention. It is read
    // through a field of the call result, which is what needs the result to
    // have storage at all.
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn make_point(x int, y int) Point {
    return .{x, y};
}

fn main() int {
    p Point = make_point(3, 4);
    C.printf("%d %d %d\n", p.x, p.y, make_point(5, 6).y);
    return 0;
}
"#;
    let scratch = Scratch::new("small-record");
    assert_eq!(scratch.run("small-record", source), "3 4 6\n");
}

#[test]
fn a_large_array_is_returned_and_indexed() {
    // The return slot is written by the callee and read here as an array. The
    // checksum is what catches a copy of the wrong bytes, not just a crash.
    let source = r#"import std;

N const usize = 32;

fn make_grid() [N][N]int {
    g [N][N]int;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            g[i][j] = (i * 100 + j) as int;
        }
    }
    return g;
}

fn sum(g [N][N]int) int {
    s int = 0;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            s += g[i][j];
        }
    }
    return s;
}

fn main() int {
    m [N][N]int = make_grid();
    C.printf("%d %d\n", m[1][2], sum(m));
    return 0;
}
"#;
    let scratch = Scratch::new("large-array");
    // `1 * 100 + 2`, and sum(i*100 + j) over 32x32 = 32 * 496 * 101.
    assert_eq!(scratch.run("large-array", source), "102 1603072\n");
}

#[test]
fn an_array_parameter_is_passed_by_value() {
    // An aggregate parameter is passed as an address and copied into the
    // callee's own storage, so writing to the parameter cannot reach the
    // argument -- that is what "by value" has to mean.
    let source = r#"import std;

N const usize = 16;

fn make_grid() [N][N]int {
    g [N][N]int;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            g[i][j] = (i * 100 + j) as int;
        }
    }
    return g;
}

fn doubled(g [N][N]int) [N][N]int {
    h [N][N]int = g;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            h[i][j] = h[i][j] * 2;
        }
    }
    h[0][0] = 0;
    return h;
}

fn total(g [N][N]int) int {
    s int = 0;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            s += g[i][j];
        }
    }
    return s;
}

fn main() int {
    m [N][N]int = make_grid();
    d [N][N]int = doubled(m);
    C.printf("%d %d %d\n", m[0][0], total(m), total(d));
    return 0;
}
"#;
    let scratch = Scratch::new("array-parameter");
    // The original's [0][0] is still 0; the doubled copy has had its own
    // [0][0] zeroed, so the parameter and the argument are separate objects.
    // sum(i*100 + j) over 16x16 is 16 * 120 * 101 = 193920.
    assert_eq!(scratch.run("array-parameter", source), "0 193920 387840\n");
}

#[test]
fn a_returned_array_can_be_initialised_assigned_and_chained() {
    // Every path an aggregate result takes: a local initialised from it,
    // another local initialised from that, an assignment over existing
    // storage, and `return f();` that hands the same slot on.
    let source = r#"import std;

N const usize = 8;

fn make_grid() [N][N]int {
    g [N][N]int;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            g[i][j] = (i * 10 + j) as int;
        }
    }
    return g;
}

fn pass_through(g [N][N]int) [N][N]int {
    return g;
}

fn chained() [N][N]int {
    return make_grid();
}

fn main() int {
    a [N][N]int = chained();
    a[3][3] = 999;
    b [N][N]int = pass_through(a);
    c [N][N]int;
    c = make_grid();
    d [N][N]int;
    d = a;
    C.printf("%d %d %d %d %d\n", a[3][3], b[3][3], c[3][3], d[3][3], a[0][1]);
    return 0;
}
"#;
    let scratch = Scratch::new("chains");
    // The edited element travels through the copy; the other values are the
    // generated ones.
    assert_eq!(scratch.run("chains", source), "999 999 33 999 1\n");
}

#[test]
fn a_returned_array_can_be_indexed_without_a_binding() {
    // `make_grid()[i][j]` asks a temporary for an address. The call must run
    // once and the row must not be loaded as an aggregate value on the way.
    let source = r#"import std;

N const usize = 8;

fn make_grid() [N][N]int {
    g [N][N]int;
    for (i usize = 0; i < N; i += 1) {
        for (j usize = 0; j < N; j += 1) {
            g[i][j] = (i * 10 + j) as int;
        }
    }
    return g;
}

fn main() int {
    C.printf("%d %d\n", make_grid()[2][3], make_grid()[7][1]);
    return 0;
}
"#;
    let scratch = Scratch::new("temporary-index");
    assert_eq!(scratch.run("temporary-index", source), "23 71\n");
}

#[test]
fn the_ir_never_loads_a_returned_array_as_a_value() {
    // The regression is visible in the IR even when a small array would run:
    // the return must be a `memcpy` out of the source's address, and there must
    // be no `load [N x [N x T]]` at all. A value of that shape is what LLVM 18
    // crashes on while selecting instructions.
    let source = r#"N const usize = 8;

fn make_grid() [N][N]int {
    g [N][N]int;
    g[0][0] = 7;
    return g;
}

fn main() int {
    m [N][N]int = make_grid();
    return m[0][0];
}
"#;
    let (outcome, ir) = compile_text("t.ft", source, &[EmitKind::Llvm]);
    assert_eq!(outcome, Outcome::Success, "{ir}");
    assert!(
        !ir.contains("load [8 x [8 x i32]]"),
        "the returned array was loaded as a value:\n{ir}"
    );
    assert!(
        ir.contains("llvm.memcpy"),
        "the returned array was not copied through its address:\n{ir}"
    );
}

#[test]
fn a_function_that_may_raise_returns_an_aggregate() {
    // The exception travels *inside* the result, so an `except` function's
    // result is a struct whose value field is the aggregate. Returning that
    // struct is what carries both halves; refusing the combination left a
    // `?!` function unable to return an array at all.
    let source = r#"import std;

fn row(v int) [4]int except {
    r [4]int;
    r[0] = v;
    r[1] = v + 1;
    return r;
}

fn main() int {
    try {
        a [4]int = row(7);
        C.printf("%d %d\n", a[0], a[1]);
    } except {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-array");
    assert_eq!(scratch.run("throwing-array", source), "7 8\n");
}

#[test]
fn a_throwing_lambda_returns_an_aggregate() {
    // The same shape, one level down: a lambda that may raise returns the
    // aggregate through the same result struct.
    let source = r#"import std;

fn main() int {
    f auto = lambda(v int) {
        r [4]int;
        r[0] = v;
        r[1] = v * 2;
        return r;
    };
    try {
        a [4]int = f(9);
        C.printf("%d %d\n", a[0], a[1]);
    } except {
        C.printf("caught\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("throwing-lambda-array");
    assert_eq!(scratch.run("throwing-lambda-array", source), "9 18\n");
}

#[test]
fn module_level_scalars_keep_their_written_values() {
    // A global's initialiser is part of the program image, so it has to become
    // an LLVM constant. Floats and characters had no constant at all and were
    // silently zeroed; the negative float also exercises the sign folding.
    let source = r#"import std;

pi double = 3.5;
neg double = -2.25;
letter c8 = 'Z';
ready bool = true;

fn main() int {
    C.printf("%.1f %.2f %c %d\n", pi, neg, letter, ready as int);
    return 0;
}
"#;
    let scratch = Scratch::new("global-scalars");
    assert_eq!(scratch.run("global-scalars", source), "3.5 -2.25 Z 1\n");
}

#[test]
fn module_level_aggregates_keep_their_written_values() {
    // A positional initialiser becomes a constant struct or array, recursively,
    // and a pointer can be initialised with the address of the private global a
    // string literal is given.
    let source = r#"import std;

type Inner {
    a int;
    b int;
}

type Outer {
    inner Inner;
    c int;
}

origin Outer = .{.{1, 2}, 3};
letters [4]const c8 = "abc";
message *const c8 = "hello";

fn main() int {
    C.printf("%d %d %d %s %c\n", origin.inner.a, origin.inner.b, origin.c, message, letters[1]);
    return 0;
}
"#;
    let scratch = Scratch::new("global-aggregates");
    assert_eq!(
        scratch.run("global-aggregates", source),
        "1 2 3 hello b\n"
    );
}
