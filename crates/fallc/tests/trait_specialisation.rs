//! Type traits written as type families with template specialisation.
//!
//! The category predicates -- `IsInteger` and the rest of the standard
//! library's traits -- are not compiler built-ins. A trait is a type family
//! whose body holds a constant member `VALUE`: the generic body answers
//! `false`, and a full or partial specialisation answers `true` for the types
//! that satisfy the predicate. Asking a query instantiates the family, the
//! specialisation machinery chooses the body, and `VALUE` folds to the answer.
//!
//! This is what makes the standard library's predicates ordinary declarations:
//! a program can define its own family the same way, and the compiler has no
//! list to keep in step with the type system.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-trait-spec-{label}-{}", std::process::id()));
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
fn the_standard_library_defines_is_integer_with_specialisation() {
    // `IsInteger` is `std.type_traits.IsInteger`, a type family imported with
    // `import std.type_traits`. `int` is `i32`, so the specialisation for an integer type
    // answers; `f64` has none and reads the generic body's `false`.
    let source = r#"import std.type_traits;
import C;

fn main() int {
    C.printf("%d %d %d %d %d\n",
        IsInteger(int) as int,
        IsInteger(f64) as int,
        IsInteger(isize) as int,
        IsInteger(bool) as int,
        IsInteger(u8) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("integer");
    assert_eq!(scratch.run("integer", source), "1 0 1 0 1\n");
}

#[test]
fn a_query_answers_per_instantiation_inside_a_template() {
    // The query is written against the template parameter, so the answer is
    // chosen when the body is instantiated -- and `if constexpr` keeps only the
    // live branch, which is what makes the split compile at all.
    let source = r#"import std.type_traits;
import C;

template <T type>
fn kind() int {
    if (IsInteger(T)) @constexpr {
        return 1;
    }
    return 0;
}

fn main() int {
    C.printf("%d %d %d %d\n", kind<int>(), kind<double>(), kind<u64>(), kind<bool>());
    return 0;
}
"#;
    let scratch = Scratch::new("template");
    assert_eq!(scratch.run("template", source), "1 0 1 0\n");
}

#[test]
fn a_program_may_declare_its_own_trait_family() {
    // Nothing about the mechanism belongs to the standard library: a family in
    // the program declares the generic answer, a full specialisation covers
    // `int`, and a partial one covers every pointer.
    let source = r#"import std.type_traits;
import C;

template <T type>
type Classified {
    VALUE const bool = false;
}

template <>
type Classified<int> {
    VALUE const bool = true;
}

template <T type>
type Classified<*T> {
    VALUE const bool = true;
}

fn main() int {
    p *int = nullptr;
    C.printf("%d %d %d\n",
        Classified(int) as int,
        Classified(f64) as int,
        Classified(type(p)) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("user-family");
    assert_eq!(scratch.run("user-family", source), "1 0 1\n");
}

#[test]
fn a_family_declared_later_in_the_file_still_answers() {
    // Declarations are order-independent, and a trait query is no exception:
    // the family is collected before any body is checked, so `main` may name a
    // family declared after it.
    let source = r#"import std.type_traits;
import C;

fn main() int {
    C.printf("%d\n", Classified(int) as int);
    return 0;
}

template <T type>
type Classified {
    VALUE const bool = false;
}

template <>
type Classified<int> {
    VALUE const bool = true;
}
"#;
    let scratch = Scratch::new("later");
    assert_eq!(scratch.run("later", source), "1\n");
}

#[test]
fn the_other_category_traits_are_families_too() {
    // Every category predicate moved to `std.type_traits`: the primitive ones
    // through full specialisations, pointers and references through partial
    // patterns that match a shape rather than a single spelling.
    let source = r#"import std.type_traits;
import C;

// Every reference shape is its own type, so every pattern has to be written.
fn references(a &int, b &const int, c &&int, d &&const int) int {
    return IsReference(type(a)) as int
        + IsReference(type(b)) as int
        + IsReference(type(c)) as int
        + IsReference(type(d)) as int;
}

// As is every pointer qualifier.
fn pointers(a *int, b *const int, c const *int) int {
    return IsPointer(type(a)) as int
        + IsPointer(type(b)) as int
        + IsPointer(type(c)) as int;
}

fn main() int {
    p *int = nullptr;
    x int = 7;
    C.printf("%d %d %d\n",
        IsFloating(f64) as int,
        IsFloating(int) as int,
        IsNumeric(f32) as int);
    C.printf("%d %d %d %d\n",
        IsNumeric(int) as int,
        IsNumeric(bool) as int,
        IsNumeric(byte) as int,
        IsNumeric(f64) as int);
    C.printf("%d %d %d\n",
        IsPointer(type(p)) as int,
        IsPointer(int) as int,
        pointers(nullptr, nullptr, nullptr));
    C.printf("%d %d\n",
        IsReference(type(x)) as int,
        references(x, x, x, x));
    C.printf("%d %d %d %d\n",
        IsVoid(void) as int,
        IsVoid(int) as int,
        IsBool(bool) as int,
        IsBool(int) as int);
    C.printf("%d %d %d\n",
        IsCharacter(char) as int,
        IsCharacter(c16) as int,
        IsCharacter(int) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("categories");
    assert_eq!(
        scratch.run("categories", source),
        "1 0 1\n1 0 0 1\n1 0 3\n0 4\n1 0 1 0\n1 1 0\n"
    );
}

#[test]
fn a_category_query_answers_inside_a_template() {
    // The same per-instantiation choice works for every predicate, including
    // `void`, which is not a type a value can have.
    let source = r#"import std.type_traits;
import C;

template <T type>
fn floating() int {
    if (IsFloating(T)) @constexpr {
        return 1;
    }
    return 0;
}

template <T type>
fn absent() int {
    if (IsVoid(T)) @constexpr {
        return 1;
    }
    return 0;
}

fn main() int {
    C.printf("%d %d %d %d\n",
        floating<f64>(),
        floating<int>(),
        absent<void>(),
        absent<int>());
    return 0;
}
"#;
    let scratch = Scratch::new("categories-template");
    assert_eq!(scratch.run("categories-template", source), "1 0 1 0\n");
}

#[test]
fn an_arity_mistake_is_reported_for_a_family_trait() {
    // The query is a family now, but it is still a question about exactly one
    // type, and a wrong number of arguments is the same diagnostic.
    let (outcome, printed) = fallc::pipeline::compile_text(
        "test.ft",
        "import std.type_traits;\n\nfn main() int {\n    if (IsFloating(int, int)) @constexpr {\n        return 1;\n    }\n    return 0;\n}\n",
        &[fallc::cli::EmitKind::Llvm],
    );
    assert_eq!(outcome, fallc::Outcome::Failure, "{printed}");
    assert!(
        printed.contains("`IsFloating` is about 1 type(s), but 2 were given"),
        "{printed}"
    );
}
