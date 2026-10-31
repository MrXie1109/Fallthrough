//! Moving a value instead of copying it.
//!
//! `std.String` and `std.Array<T>` own a heap buffer, which is what makes a
//! move observable: a move hands the buffer over, so the source and the
//! destination name the *same* address, while a copy allocates a second buffer
//! and the addresses differ. Both spellings print the same text, so a test that
//! only checked the text would pass either way -- these check the addresses.
//!
//! The sources are compiled and the programs are run, rather than inspecting
//! what the compiler recorded: `import std` is resolved by the real module
//! loader, and the address in the output is the address the allocator returned.
//!
//! Three calls are chosen by the checker rather than by the call site, so all
//! three are checked at the call the program wrote, in the IR as well as by what
//! the program prints:
//!
//! - `String b = make();` must reach the move constructor, because only a
//!   temporary may be taken from.
//! - `s = make();` must reach the move form of `__assign__`, for the same
//!   reason, and for the same reason the receiver's old buffer must be released.
//! - `b = a;` must reach the *copying* form, because `a` outlives the
//!   statement. A move there would empty the named source, which is invisible
//!   until its buffer is read as null.
//!
//! The moved-from states are also checked. A move leaves the source empty --
//! null buffer, zero length and capacity -- and that value is a `String` the
//! destructor will be run on, so it has to be safe: `data()` still a C string,
//! and a null buffer that `__drop__` may free without a double free. The
//! language has no `move`, so the bit pattern the move constructor writes is
//! made by hand, field for field, in the same order.

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
            std::env::temp_dir().join(format!("fallc-moves-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn write(&self, source: &str) -> PathBuf {
        let path = self.directory.join("main.ft");
        std::fs::write(&path, source).expect("the source can be written");
        path
    }

    fn parse(&self, args: &[String]) -> Options {
        Options::parse(args).expect("the arguments are well formed")
    }

    /// Compiles the source into an executable and runs it.
    ///
    /// Panics when it does not build, so that the failure the compiler printed
    /// is the one the test shows.
    fn run(&self, label: &str, source: &str) -> String {
        let main = self.write(source);
        let executable = self.directory.join(label);
        let options = self.parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ]);
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

    /// Compiles the source to LLVM IR and returns it.
    fn ir(&self, label: &str, source: &str) -> String {
        let main = self.write(source);
        let assembly = self.directory.join(format!("{label}.ll"));
        let options = self.parse(&[
            main.display().to_string(),
            "--emit=llvm".to_owned(),
            "-o".to_owned(),
            assembly.display().to_string(),
        ]);
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        std::fs::read_to_string(&assembly).expect("the IR can be read")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// The address on a line beginning with `label`, without the label.
///
/// Only the first word after the label: the address is what the move changed,
/// and the rest of the line says what else came with it.
fn address(output: &str, label: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no `{label}` line in:\n{output}"))
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("`{label}` names no address in:\n{output}"))
        .to_owned()
}

/// The body of the definition that starts at `marker`.
///
/// A mangled symbol is what tells two overloads apart in the IR, and the
/// definition is what says what the chosen overload does.
fn body_after<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .find(marker)
        .unwrap_or_else(|| panic!("no `{marker}` in:\n{ir}"));
    let rest = &ir[start..];
    let end = rest.find("\n}").expect("a definition is closed");
    &rest[..end]
}

/// The body of the code the program itself wrote.
///
/// A program whose modules bring a `__setup__` block -- `std` has one, because
/// its devices are connected before anything runs -- has its own `main` emitted
/// as `__ft_user_main`, with a wrapper called `main` around it that runs the
/// setup first. These tests are about the code the program wrote, so that is the
/// function to read.
fn program_main(ir: &str) -> &str {
    if ir.contains("define i32 @__ft_user_main()") {
        body_after(ir, "define i32 @__ft_user_main()")
    } else {
        body_after(ir, "define i32 @main()")
    }
}

/// Returns every line of IR that calls a function whose mangled name contains
/// `text`.
fn calls_to<'a>(ir: &'a str, text: &str) -> Vec<&'a str> {
    ir.lines()
        .filter(|line| line.contains("call void @") && line.contains(text))
        .collect()
}

/// Whether any of these calls names an rvalue reference in its parameter list.
///
/// `W` is how `mangle_type` spells `&&T` and `R` spells `&T`, so `_W0` on a
/// call to `__assign__` is the move form and `_R1` is the copying one.
fn calls_a_move<'a>(calls: impl Iterator<Item = &'a str>) -> bool {
    calls.into_iter().any(|line| line.contains("_W0"))
}

const MOVE_STRING: &str = r#"import std;

fn make() String {
    a String = "hello, this text is long enough to need a heap buffer";
    C.printf("inner %p\n", a.data());
    return a;
}

fn main() int {
    b String = make();
    C.printf("moved %p\n", b.data());
    C.printf("text %s\n", b.data());
    C.printf("len %lu\n", b.length());
    return 0;
}
"#;

const MOVE_ARRAY: &str = r#"import std;

fn make() Array<int> {
    a Array<int>;
    a.push(7);
    a.push(8);
    C.printf("inner %p\n", a.data());
    return a;
}

fn main() int {
    b Array<int> = make();
    C.printf("moved %p\n", b.data());
    C.printf("len %lu first %d\n", b.length(), b[0]);
    return 0;
}
"#;

const MOVE_ASSIGN_STRING: &str = r#"import std;

fn make() String {
    t String = "temporary text, longer than the old one";
    C.printf("inner %p\n", t.data());
    return t;
}

fn main() int {
    s String = "old buffer";
    C.printf("before %p\n", s.data());
    s = make();
    C.printf("after %p\n", s.data());
    C.printf("text %s\n", s.data());
    C.printf("len %lu\n", s.length());
    return 0;
}
"#;

const MOVE_ASSIGN_ARRAY: &str = r#"import std;

fn make() Array<int> {
    a Array<int>;
    a.push(7);
    a.push(8);
    C.printf("inner %p\n", a.data());
    return a;
}

fn main() int {
    b Array<int>;
    b.push(1);
    C.printf("before %p len %lu\n", b.data(), b.length());
    b = make();
    C.printf("after %p len %lu first %d\n", b.data(), b.length(), b[0]);
    return 0;
}
"#;

const COPY_ASSIGN_STRING: &str = r#"import std;

fn main() int {
    a String = "source text";
    b String = "destination";
    b = a;
    C.printf("distinct %d\n", a.data() != b.data());
    C.printf("a alive %d\n", a.data() != nullptr);
    C.printf("a %s\n", a.data());
    C.printf("b %s\n", b.data());
    a.push('!');
    C.printf("a pushed %s\n", a.data());
    C.printf("b unchanged %s\n", b.data());
    return 0;
}
"#;

const COPY_ASSIGN_ARRAY: &str = r#"import std;

fn main() int {
    a Array<int>;
    a.push(1);
    a.push(2);
    b Array<int>;
    b.push(9);
    b = a;
    C.printf("distinct %d\n", a.data() != b.data());
    C.printf("a len %lu\n", a.length());
    C.printf("b len %lu\n", b.length());
    C.printf("a first %d\n", a[0]);
    b.push(3);
    C.printf("after push a len %lu b len %lu\n", a.length(), b.length());
    return 0;
}
"#;

#[test]
fn constructing_a_string_from_a_call_takes_its_buffer() {
    // The address is the proof. `make` builds a string with a heap buffer and
    // reports where it is; the binding that receives the value must report the
    // same address. A copy would have allocated one of its own, and the text
    // and length would be identical either way.
    let scratch = Scratch::new("move-string");
    let output = scratch.run("move_string", MOVE_STRING);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "moved"),
        "the buffer was copied rather than moved:\n{output}"
    );
    assert!(
        output.contains("text hello, this text is long enough to need a heap buffer"),
        "{output}"
    );
    assert!(
        output.contains("len 53"),
        "the length travelled with it: {output}"
    );
}

#[test]
fn constructing_an_array_from_a_call_takes_its_buffer() {
    // The same for `Array<int>`, whose move form transfers the buffer rather
    // than the elements. The move is chosen for a *temporary*, so this is also
    // the test that a call is recognised as one.
    let scratch = Scratch::new("move-array");
    let output = scratch.run("move_array", MOVE_ARRAY);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "moved"),
        "the buffer was copied rather than moved:\n{output}"
    );
    assert!(output.contains("len 2 first 7"), "{output}");
}

#[test]
fn assigning_a_string_from_a_call_takes_its_buffer() {
    // Moving *into* a value that already exists: the receiver owns a buffer, so
    // the move form releases that buffer first and then takes the source's.
    // The address after the assignment must be the one `make` reported, and the
    // receiver's own buffer must be gone -- the two would be the same address
    // only if nothing was released.
    let scratch = Scratch::new("move-assign-string");
    let output = scratch.run("move_assign_string", MOVE_ASSIGN_STRING);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "after"),
        "the assignment copied rather than moved:\n{output}"
    );
    assert_ne!(
        address(&output, "before"),
        address(&output, "after"),
        "the receiver kept its own buffer:\n{output}"
    );
    assert!(
        output.contains("text temporary text, longer than the old one"),
        "{output}"
    );
    assert!(output.contains("len 39"), "{output}");
}

#[test]
fn assigning_an_array_from_a_call_takes_its_buffer() {
    // The same for `Array<int>`. A copy would leave the receiver's own buffer in
    // place and copy the elements into it; the addresses say which happened.
    let scratch = Scratch::new("move-assign-array");
    let output = scratch.run("move_assign_array", MOVE_ASSIGN_ARRAY);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "after"),
        "the assignment copied rather than moved:\n{output}"
    );
    assert!(output.contains("len 2 first 7"), "{output}");
}

#[test]
fn assigning_from_a_name_copies_a_string() {
    // A name outlives the statement, so assignment must copy. A move would take
    // the buffer out of `a` and leave it null -- `a alive 0` -- and the next
    // write through `a` would fault instead of appending.
    let scratch = Scratch::new("copy-string");
    let output = scratch.run("copy_assign_string", COPY_ASSIGN_STRING);
    assert!(
        output.contains("distinct 1"),
        "the buffers are one: {output}"
    );
    assert!(
        output.contains("a alive 1"),
        "assigning from a name emptied it: {output}"
    );
    assert!(output.contains("a source text"), "{output}");
    assert!(output.contains("b source text"), "{output}");
    assert!(output.contains("a pushed source text!"), "{output}");
    assert!(
        output.contains("b unchanged source text"),
        "the appended byte reached the destination: {output}"
    );
}

#[test]
fn assigning_from_a_name_copies_an_array() {
    // The same for `Array<int>`. A steal would leave `a` empty, and reading
    // `a[0]` afterwards would abort rather than print the first element.
    let scratch = Scratch::new("copy-array");
    let output = scratch.run("copy_assign_array", COPY_ASSIGN_ARRAY);
    assert!(output.contains("distinct 1"), "{output}");
    assert!(
        output.contains("a len 2"),
        "the source was emptied: {output}"
    );
    assert!(output.contains("b len 2"), "{output}");
    assert!(output.contains("a first 1"), "{output}");
    assert!(
        output.contains("after push a len 2 b len 3"),
        "the destination shares the source's buffer: {output}"
    );
}

#[test]
fn appending_and_inserting_another_string_copies_it() {
    // There is deliberately no `&&String` overload of `push` or `insert`: the
    // text has to end up after (or in the middle of) the receiver's bytes, so
    // its buffer cannot be taken unless the receiver is empty, and a second
    // overload would be selected by declaration order rather than by the
    // argument being dying. This is the test that a *named* argument is still a
    // copy: `t` keeps its text after both operations.
    let source = r#"import std;

fn main() int {
    a String = "a:";
    t String = "t";
    a.push(t);
    b String = "b:";
    b.insert(0, t);
    C.printf("a [%s]\n", a.data());
    C.printf("b [%s]\n", b.data());
    C.printf("t [%s]\n", t.data());
    return 0;
}
"#;
    let scratch = Scratch::new("string-append");
    let output = scratch.run("append", source);
    assert!(output.contains("a [a:t]"), "{output}");
    assert!(output.contains("b [tb:]"), "{output}");
    assert!(
        output.contains("t [t]"),
        "the appended argument was emptied: {output}"
    );
}

#[test]
fn constructing_a_string_from_a_name_copies_it() {
    // Construction prefers the move form only for a *dying* value. A name is
    // not dying, so this must reach the copying constructor and give two
    // buffers. The source stays intact.
    let source = r#"import std;

fn main() int {
    a String = "source";
    b String = a;
    C.printf("distinct %d\n", a.data() != b.data());
    C.printf("a %s\n", a.data());
    C.printf("b %s\n", b.data());
    return 0;
}
"#;
    let scratch = Scratch::new("copy-init");
    let output = scratch.run("copy_init", source);
    assert!(output.contains("distinct 1"), "{output}");
    assert!(output.contains("a source"), "{output}");
    assert!(output.contains("b source"), "{output}");
}

#[test]
fn the_move_constructor_takes_the_buffer_without_copying_it() {
    // The address test proves *that* the move happened; the IR shows how. The
    // move constructor must store the source's pointer and empty the source,
    // with no `memcpy` and no allocation -- a copy hidden behind a move-shaped
    // call would show up here even when the program's output looked right.
    let source = r#"import std;

fn make() String {
    a String = "hello";
    return a;
}

fn main() int {
    b String = make();
    return 0;
}
"#;
    let scratch = Scratch::new("move-ir");
    let ir = scratch.ir("move", source);
    let main = program_main(&ir);
    let calls = calls_to(main, "__init__");
    assert!(
        calls_a_move(calls.iter().copied()),
        "the call did not reach the move constructor: {calls:?}\n{ir}"
    );
    let body = body_after(&ir, "define void @__init___R0N6_String_W0N6_String");
    assert!(
        !body.contains("@memcpy"),
        "the move constructor copies the text: {body}"
    );
    // `alloca` is storage for the parameters, not an allocation: the call to an
    // allocator is what would show up as a `@`-prefixed symbol.
    assert!(
        !body.contains("@alloc") && !body.contains("@malloc"),
        "the move constructor allocates: {body}"
    );
    assert!(
        body.contains("store ptr null"),
        "the source's buffer was not emptied: {body}"
    );
}

#[test]
fn assignment_from_a_call_reaches_the_move_form() {
    // `s = make();` may take from the temporary. The call site is where this is
    // visible: the copying form and the moving form are different symbols, and
    // only the dying argument may reach the second.
    let source = r#"import std;

fn make() String {
    t String = "temporary";
    return t;
}

fn main() int {
    s String = "old";
    s = make();
    return 0;
}
"#;
    let scratch = Scratch::new("assign-move-ir");
    let ir = scratch.ir("assign_move", source);
    let main = program_main(&ir);
    let calls = calls_to(main, "__assign__");
    assert!(
        calls_a_move(calls.iter().copied()),
        "the assignment did not reach the move form: {calls:?}\n{ir}"
    );
}

#[test]
fn assignment_from_a_name_reaches_the_copying_form() {
    // The one ordering this file exists to protect: if the `&&` form of
    // `__assign__` were declared first, the checker would pick it here too -- a
    // named value is not dying, so the copy form has to be the one that fits,
    // and declaration order is what decides the rest. The copy form is what a
    // named source must reach, so the call site in the IR is the check.
    let source = r#"import std;

fn main() int {
    a String = "source";
    b String = "destination";
    b = a;
    return 0;
}
"#;
    let scratch = Scratch::new("assign-ir");
    let ir = scratch.ir("assign", source);
    let main = program_main(&ir);
    let calls = calls_to(main, "__assign__");
    assert!(
        !calls.is_empty() && calls.iter().any(|line| line.contains("_R1")),
        "the assignment did not reach the copying form: {calls:?}\n{ir}"
    );
    assert!(
        !calls_a_move(calls.iter().copied()),
        "the assignment reached the move form from a name: {calls:?}\n{ir}"
    );
}

#[test]
fn assigning_an_array_from_a_name_reaches_the_copying_form() {
    // The same for `Array<int>`: the two forms are separate symbols and the
    // named source must reach the copying one.
    let source = r#"import std;

fn main() int {
    a Array<int>;
    a.push(1);
    b Array<int>;
    b.push(2);
    b = a;
    return 0;
}
"#;
    let scratch = Scratch::new("assign-array-ir");
    let ir = scratch.ir("assign_array", source);
    let main = program_main(&ir);
    let calls = calls_to(main, "__assign__");
    assert!(
        !calls.is_empty() && calls.iter().any(|line| line.contains("_R1")),
        "the assignment did not reach the copying form: {calls:?}\n{ir}"
    );
    assert!(
        !calls_a_move(calls.iter().copied()),
        "the assignment reached the move form from a name: {calls:?}\n{ir}"
    );
}

#[test]
fn an_array_of_arrays_moves_its_outer_buffer() {
    // The interesting case for a move: the element is itself a container. The
    // outer buffer transfers whole, so the addresses match and the inner arrays
    // come along with it -- no element is copied at all.
    let source = r#"import std;

fn make() Array<Array<int>> {
    o Array<Array<int>>;
    i Array<int>;
    i.push(5);
    i.push(6);
    o.push(i);
    C.printf("inner %p len %lu\n", o.data(), o.length());
    return o;
}

fn main() int {
    b Array<Array<int>> = make();
    C.printf("moved %p len %lu first %d second %d\n", b.data(), b.length(), b[0][0], b[0][1]);
    return 0;
}
"#;
    let scratch = Scratch::new("array-of-arrays-move");
    let output = scratch.run("array_of_arrays_move", source);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "moved"),
        "the outer buffer was copied rather than moved:\n{output}"
    );
    assert!(
        output.contains("len 1 first 5 second 6"),
        "the elements did not travel with the buffer: {output}"
    );
}

#[test]
fn an_array_of_arrays_copies_its_elements() {
    // An element of `Array<Array<int>>` is an owning value, and the element is
    // constructed in the raw slot through its own `__init__`, so storing one is
    // a deep copy. `Array<String>` works the same way: the element type's
    // methods are looked up in the instantiating file's table, where an
    // imported `String`'s `__init__` and `__drop__` live. `element_construction`
    // covers the `String` case directly.
    let source = r#"import std;

fn main() int {
    inner Array<int>;
    inner.push(5);
    outer Array<Array<int>>;
    outer.push(inner);
    C.printf("element distinct %d first %d\n", outer[0].data() != inner.data(), outer[0][0]);

    copy Array<Array<int>> = outer;
    C.printf("copy distinct %d first %d\n", copy[0].data() != outer[0].data(), copy[0][0]);

    assigned Array<Array<int>>;
    one Array<int>;
    one.push(0);
    assigned.push(one);
    assigned = outer;
    C.printf("assign distinct %d first %d\n", assigned[0].data() != outer[0].data(), assigned[0][0]);
    return 0;
}
"#;
    let scratch = Scratch::new("array-of-arrays-copy");
    let output = scratch.run("array_of_arrays_copy", source);
    assert!(output.contains("element distinct 1 first 5"), "{output}");
    assert!(output.contains("copy distinct 1 first 5"), "{output}");
    assert!(output.contains("assign distinct 1 first 5"), "{output}");
}

#[test]
fn a_moved_from_string_is_still_a_valid_string() {
    // A move leaves the source empty. That state is not reachable from the
    // language -- only a dying temporary is moved from, and nothing can name one
    // -- so what is checked here is what a caller can observe: the destination
    // has the text, and the source is destroyed at the end of its scope without
    // freeing anything twice.
    //
    // Both paths are covered. A short string is inline, so a move copies its
    // bytes into the destination and empties the source to the inline empty
    // state; a long one hands over its buffer and leaves null behind. Either way
    // the source's destructor must be safe to run afterwards, which happens
    // automatically when the moved-from temporary goes out of scope.
    let source = r#"import std;

fn short_one() String {
    s String = "short";
    return s;
}

fn long_one() String {
    s String = "long enough that the text lives in a buffer of its own";
    return s;
}

fn main() int {
    a String = short_one();
    b String = long_one();
    C.printf("short [%s] len %lu\n", a.data(), a.length());
    C.printf("long [%s] len %lu\n", b.data(), b.length());
    a.push('!');
    C.printf("grown short [%s] len %lu\n", a.data(), a.length());
    return 0;
}
"#;
    let scratch = Scratch::new("moved-from");
    let output = scratch.run("moved_from", source);
    assert!(output.contains("short [short] len 5"), "{output}");
    assert!(
        output.contains("long [long enough that the text lives in a buffer of its own] len 54"),
        "{output}"
    );
    assert!(output.contains("grown short [short!] len 6"), "{output}");
}

#[test]
fn a_moved_from_array_is_still_a_valid_array() {
    // The same for `Array<int>`. An empty array is already null and zero, which
    // is why the moved-from state is not a special case there: it is the state
    // the default constructor leaves. It must be usable and destructible.
    let source = r#"import std;

fn moved_from() Array<int> {
    a Array<int>;
    a.push(1);
    C.free(a._data as *byte);
    a._data = nullptr;
    a._len = 0;
    a._cap = 0;
    return a;
}

fn main() int {
    a Array<int> = moved_from();
    C.printf("len %lu empty %d\n", a.length(), a.empty());
    a.push(9);
    C.printf("pushed len %lu first %d\n", a.length(), a[0]);
    return 0;
}
"#;
    let scratch = Scratch::new("moved-from-array");
    let output = scratch.run("moved_from_array", source);
    assert!(output.contains("len 0 empty 1"), "{output}");
    assert!(output.contains("pushed len 1 first 9"), "{output}");
}

#[test]
fn a_string_definition_is_released_at_scope_exit() {
    // Destruction is what makes ownership more than a comment: the buffer of a
    // `String` that goes out of scope comes back to the allocator. That is
    // observed through the allocator -- the next request of the same size is
    // handed the chunk the destructor released -- because nothing else about a
    // freed block is visible from the language.
    let source = r#"import std;

fn scoped() void {
    a String = "0123456789abcdef0123456789";
    C.printf("scoped %p\n", a.data());
}

fn main() int {
    scoped();
    b String = "abcdefghij01234567890123456789";
    C.printf("reused %p\n", b.data());
    return 0;
}
"#;
    let scratch = Scratch::new("scope-release");
    let output = scratch.run("scope_release", source);
    assert_eq!(
        address(&output, "scoped"),
        address(&output, "reused"),
        "the buffer was not released at scope exit:\n{output}"
    );
}

#[test]
fn moving_into_a_string_releases_the_receivers_buffer() {
    // The move form of assignment releases the receiver's own buffer before it
    // takes the source's. Releasing it is invisible unless something asks the
    // allocator for the same size afterwards, which is what `probe` does: the
    // address it gets is the one the receiver gave up.
    let source = r#"import std;

fn make() String {
    t String = "new text that is long enough to allocate";
    C.printf("inner %p\n", t.data());
    return t;
}

fn main() int {
    s String = "0123456789abcdef0123 that allocates";
    C.printf("before %p\n", s.data());
    s = make();
    C.printf("after %p\n", s.data());
    probe String = "abcdefghij0123456789 that allocates";
    C.printf("reused %p\n", probe.data());
    C.printf("text %s\n", s.data());
    return 0;
}
"#;
    let scratch = Scratch::new("release-receiver");
    let output = scratch.run("release_receiver", source);
    assert_eq!(
        address(&output, "inner"),
        address(&output, "after"),
        "the assignment copied rather than moved:\n{output}"
    );
    assert_eq!(
        address(&output, "before"),
        address(&output, "reused"),
        "the receiver's old buffer was not released:\n{output}"
    );
    assert!(
        output.contains("text new text that is long enough to allocate"),
        "{output}"
    );
}

#[test]
fn the_move_assignment_releases_the_receivers_buffer_first() {
    // The body is the proof the addresses cannot fully give: the receiver's
    // buffer has to be freed *before* the source's pointer is stored over it,
    // or the buffer is leaked. Both containers' move forms are checked, since
    // the same rule has to hold for each.
    let source = r#"import std;

fn make_string() String {
    t String = "temporary";
    return t;
}

fn make_array() Array<int> {
    t Array<int>;
    t.push(1);
    return t;
}

fn main() int {
    s String = "old";
    s = make_string();
    a Array<int>;
    a.push(1);
    a = make_array();
    return 0;
}
"#;
    let scratch = Scratch::new("release-ir");
    let ir = scratch.ir("release", source);
    for marker in [
        "define void @__assign___R0N6_String_W0N6_String",
        "define void @\"__assign___R0N10_Array<i32>_W0N10_Array<i32>\"",
    ] {
        let body = body_after(&ir, marker);
        assert!(
            body.contains("@free"),
            "the move form does not release the receiver's buffer: {body}"
        );
        assert!(
            body.contains("store ptr null"),
            "the source's buffer was not emptied: {body}"
        );
    }
}

#[test]
fn a_box_owns_one_value_and_moves_it() {
    // `std.Box<T>` is the language's smart pointer: the box owns the value and
    // frees it when it goes out of scope. A box moves rather than copies, which
    // is why the emptied source reports `empty()` after the move.
    let source = r#"import std;

fn main() int {
    b Box<int> = 7;
    C.printf("get %d\n", b.get());
    b.get() = 9;
    C.printf("set %d\n", b.get());
    moved Box<int> = std.move(b);
    C.printf("source empty %d, moved get %d\n", b.empty(), moved.get());
    return 0;
}
"#;
    let scratch = Scratch::new("box");
    let output = scratch.run("box", source);
    assert!(output.contains("get 7"), "{output}");
    assert!(output.contains("set 9"), "{output}");
    assert!(output.contains("source empty 1, moved get 9"), "{output}");
}

#[test]
fn growing_an_array_moves_its_elements() {
    // A `String` that owns a buffer is not copied when the array reallocates:
    // the buffer is taken over, so the element's bytes stay where they are. The
    // address is what shows it -- a copy would give the element a buffer of its
    // own, and the old one would be freed.
    let source = r#"import std;

fn main() int {
    a Array<String>;
    first String = "the first element, long enough to own a buffer of its own";
    a.push(first);
    C.printf("before %p\n", a[0].data());
    second String = "second element, also long enough to own a buffer";
    third String = "third element, also long enough to own a buffer";
    fourth String = "fourth element, also long enough to own a buffer";
    fifth String = "fifth element, also long enough to own a buffer";
    sixth String = "sixth element, also long enough to own a buffer";
    seventh String = "seventh element, also long enough to own a buffer";
    eighth String = "eighth element, also long enough to own a buffer";
    a.push(second);
    a.push(third);
    a.push(fourth);
    a.push(fifth);
    a.push(sixth);
    a.push(seventh);
    a.push(eighth);
    ninth String = "ninth element, also long enough to own a buffer";
    a.push(ninth);
    C.printf("after %p\n", a[0].data());
    C.printf("text [%s]\n", a[0].data());
    C.printf("tenth [%s]\n", a[8].data());
    return 0;
}
"#;
    let scratch = Scratch::new("grow-moves");
    let output = scratch.run("grow_moves", source);
    let before = address(&output, "before");
    let after = address(&output, "after");
    assert_eq!(
        before, after,
        "growing copied the element instead of moving it:\n{output}"
    );
    assert!(
        output.contains("text [the first element, long enough to own a buffer of its own]"),
        "{output}"
    );
    assert!(
        output.contains("tenth [ninth element, also long enough to own a buffer]"),
        "{output}"
    );
}

#[test]
fn growing_copies_when_the_move_may_raise() {
    // An element type whose move constructor can raise is copied instead: a
    // raise halfway through a reallocation would leave the elements it had not
    // reached yet in the old buffer and the ones it had in the new one. The IR
    // is what shows which constructor the growth loop reached for.
    let source = r#"import std;

type Eager {
    _v int;
}

fn __im_init__(this &Eager, value int) void {
    this._v = value;
}

fn __init__(this &Eager, other &const Eager) void {
    this._v = other._v;
}

fn __init__(this &Eager, other &&Eager) void except {
    if (other._v == 0) {
        raise "EagerError";
    }
    this._v = other._v;
}

fn main() int {
    a Array<Eager>;
    one Eager = 1;
    a.push(one);
    two Eager = 2;
    a.push(two);
    return a.length() as int;
}
"#;
    let scratch = Scratch::new("eager-grow");
    let ir = scratch.ir("eager_grow", source);
    let start = ir
        .find("define void @\"reserve_R0N12_Array<Eager>_I3_u64\"")
        .unwrap_or_else(|| panic!("no instantiation of `reserve` in:\n{ir}"));
    let rest = &ir[start..];
    let end = rest.find("\n}").expect("a definition is closed");
    let body = &rest[..end];
    assert!(
        body.contains("__init___R0N5_Eager_R1N5_Eager"),
        "the growth loop did not copy the element:\n{body}"
    );
    assert!(
        !body.contains("__init___R0N5_Eager_W0N5_Eager"),
        "the growth loop moved an element it may not move:\n{body}"
    );
}
