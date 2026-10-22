//! Every example must compile and run.
//!
//! An example is documentation that claims to be true. The two example files
//! that once could not be compiled at all were exactly the ones no test looked
//! at, so this test looks at all of them: it compiles each, links it, runs it,
//! and checks that it exited the way its `main` says it should.
//!
//! The assertion on the *exit code* matters. A program whose `main` returns 15
//! and a program that segfaults after printing nothing are both "not zero", so
//! the expected value is written out per example rather than inferred.

use std::path::{Path, PathBuf};
use std::process::Command;

use fallc::cli::Options;
use fallc::pipeline::run;

/// An example, and what running it should produce.
struct Example {
    /// The file's name without its extension.
    name: &'static str,
    /// The exit code its `main` returns.
    ///
    /// A `return` value is an exit code, so this is the program's answer rather
    /// than a pass/fail flag.
    exit_code: i32,
    /// A line the program must print, which is what catches an example that
    /// links and runs but computes nothing.
    ///
    /// `None` for the examples that print nothing.
    expect_output: Option<&'static str>,
}

/// The examples, with what each one does when it works.
const EXAMPLES: &[Example] = &[
    Example { name: "hello", exit_code: 0, expect_output: None },
    Example { name: "hello_c", exit_code: 0, expect_output: Some("Hello, world!") },
    Example { name: "loops", exit_code: 15, expect_output: None },
    Example { name: "ops", exit_code: 49, expect_output: None },
    Example { name: "vars", exit_code: 7, expect_output: None },
    Example { name: "types", exit_code: 0, expect_output: None },
    Example { name: "casts", exit_code: 1, expect_output: None },
    Example { name: "const", exit_code: 82, expect_output: None },
    Example { name: "ptr", exit_code: 42, expect_output: None },
    Example { name: "switch", exit_code: 30, expect_output: None },
    Example { name: "point", exit_code: 0, expect_output: Some("(42, 37)") },
    Example { name: "math", exit_code: 0, expect_output: Some("sqrt") },
    Example { name: "literals", exit_code: 0, expect_output: Some("zhong = 20013") },
    // The one with the most to get wrong: a fall-through, a shared label, the
    // loop `else`, and `break` suppressing it.
    Example { name: "controlflow", exit_code: 0, expect_output: Some("while else ran:      103") },
    // The standard library's two containers, which is where a mistake in the
    // ownership rules shows up as a wrong answer rather than as a crash.
    Example { name: "strings", exit_code: 0, expect_output: Some("reversed: cba") },
    // The copy line is the one that catches a shallow copy: with the aliasing
    // bug this printed `copy[0]: -1, numbers[0]: -1`, and a substring check on
    // `total: 60` would still have passed. The `back() = -7` line catches a
    // reference-returning method being read as a value, and a method overloaded
    // on its receiver's const-ness picking the wrong one.
    // Editing in the middle, on both containers: the tail moves, and the
    // string's terminator moves with it.
    Example {
        name: "edit",
        exit_code: 0,
        expect_output: Some("string erase:  removed >, left >abc"),
    },
    Example {
        name: "arrays",
        exit_code: 60,
        expect_output: Some("after back() = -7: -7 (front 7)"),
    },
    // Exceptions: a raise, a catch that inspects `name()`/`what()`, a
    // propagation through a function that does not handle it, and `finally`.
    Example {
        name: "exceptions",
        exit_code: 0,
        expect_output: Some("expected: cannot be negative"),
    },
    // The standard library beyond `Array` and `String`: the containers, the
    // numbers, the devices, the lock, and the path. The file it writes is
    // removed by the run itself, so the expected output is all it leaves.
    Example {
        name: "library",
        exit_code: 0,
        expect_output: Some("hash map: 2 3 1"),
    },
    // A pack, asked three ways: how many elements, what they add up to, and
    // what one of them is. The `count<>` line is the one that catches an empty
    // pack being rejected where it should plainly count zero.
    Example {
        name: "packs",
        exit_code: 0,
        expect_output: Some("count<>: 0"),
    },
];

/// The directory the examples live in.
fn examples_directory() -> PathBuf {
    // The test runs from the crate's root, so the examples are two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Compiles one example into a temporary executable.
///
/// The compiler is driven through `Options` and `run`, which is the same path
/// the command line takes, so this exercises argument handling and the real
/// pipeline rather than a shortcut only the tests use.
///
/// The output goes to a per-test temporary directory rather than into the
/// repository, so running the tests leaves no build products behind.
fn build_example(name: &str) -> (bool, PathBuf) {
    let source = examples_directory().join(format!("{name}.ft"));
    let directory =
        std::env::temp_dir().join(format!("fallc-example-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&directory);
    let output = directory.join(name);
    let options = Options::parse(&[
        source.display().to_string(),
        "-o".to_owned(),
        output.display().to_string(),
    ])
    .expect("the arguments are well formed");
    let outcome = run(&options);
    let built = outcome == std::process::ExitCode::SUCCESS;
    (built, output)
}

#[test]
fn every_example_compiles_and_runs() {
    let mut problems = Vec::new();
    for example in EXAMPLES {
        let (built, executable) = build_example(example.name);
        if !built {
            problems.push(format!(
                "`{}` did not compile (the compiler reported above)",
                example.name
            ));
            continue;
        }
        let run = Command::new(&executable).output();
        let run = match run {
            Ok(run) => run,
            Err(error) => {
                problems.push(format!("`{}` could not be run: {error}", example.name));
                continue;
            }
        };
        let code = run.status.code();
        if code != Some(example.exit_code) {
            problems.push(format!(
                "`{}` exited with {code:?}, expected {}",
                example.name, example.exit_code
            ));
        }
        if let Some(expected) = example.expect_output {
            let printed = String::from_utf8_lossy(&run.stdout);
            if !printed.contains(expected) {
                problems.push(format!(
                    "`{}` printed {printed:?}, which does not contain {expected:?}",
                    example.name
                ));
            }
        }
        let _ = std::fs::remove_file(&executable);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn every_example_file_is_covered_by_this_test() {
    // A new example that is not listed above would otherwise go unchecked,
    // which is how two of them came to be unbuildable.
    let mut found: Vec<String> = std::fs::read_dir(examples_directory())
        .expect("the examples directory exists")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "ft" {
                return None;
            }
            Some(path.file_stem()?.to_str()?.to_owned())
        })
        .collect();
    found.sort();
    let mut listed: Vec<String> = EXAMPLES.iter().map(|e| e.name.to_owned()).collect();
    listed.sort();
    assert_eq!(
        found, listed,
        "every example must be listed in EXAMPLES, so that it is compiled and run"
    );
}
