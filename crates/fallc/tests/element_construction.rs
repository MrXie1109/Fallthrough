//! Constructing an owned element in place.
//!
//! `std.Array<T>` keeps its elements in raw storage, so an element has to be
//! *constructed* there rather than assigned over whatever the allocator left
//! behind. The difference is only observable for an element that owns memory:
//! a `String` element must get a buffer of its own, and an `Array<int>` element
//! must own an inner buffer rather than share the source's.
//!
//! The addresses are the proof. Two `String`s that print the same text can
//! share one buffer (the shallow copy) or own two (the deep copy), and the
//! program prints `data()` to tell them apart. A source mutated after the push
//! is the second check: the element must not change with it.
//!
//! Destruction is observed through the allocator, as in `container_moves.rs`:
//! a released element buffer is handed back to the next request of the same
//! size, and a leak prints nothing, so the test asserts the reuse positively.
//!
//! `Array<int>` is the other half of the rule. A primitive's lifetime methods
//! are built in -- construction and assignment are a store, destruction is
//! nothing -- so the same generic body is a plain store for it, and the test
//! checks that push and pop still work.

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
            std::env::temp_dir().join(format!("fallc-elements-{label}-{}", std::process::id()));
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
fn body_after<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .find(marker)
        .unwrap_or_else(|| panic!("no `{marker}` in:\n{ir}"));
    let rest = &ir[start..];
    let end = rest.find("\n}").expect("a definition is closed");
    &rest[..end]
}

#[test]
fn pushing_a_named_string_constructs_a_distinct_element() {
    // The regression this file exists for. `a.push(s)` used to store `s` into
    // the slot bitwise, which left the element and `s` sharing one buffer. The
    // addresses must differ, and mutating `s` afterwards must leave the
    // element's text alone -- reading freed bytes was the visible symptom once
    // `s` was destroyed.
    let source = r#"import std;

fn main() int {
    a Array<String>;
    s String = "a heap string long enough that a second copy needs its own buffer";
    C.printf("source %p\n", s.data());
    a.push(s);
    C.printf("element %p\n", a[0].data());
    C.printf("distinct %d\n", a[0].data() != s.data());
    s.push('!');
    C.printf("element [%s]\n", a[0].data());
    C.printf("source [%s]\n", s.data());
    return 0;
}
"#;
    let scratch = Scratch::new("push-string");
    let output = scratch.run("push_string", source);
    assert_ne!(
        address(&output, "element"),
        address(&output, "source"),
        "the element shares the source's buffer:\n{output}"
    );
    assert!(
        output.contains("distinct 1"),
        "the element shares the source's buffer:\n{output}"
    );
    assert!(
        output.contains(
            "element [a heap string long enough that a second copy needs its own buffer]"
        ),
        "the element changed when the source did:\n{output}"
    );
    assert!(
        output.contains(
            "source [a heap string long enough that a second copy needs its own buffer!]"
        ),
        "the source was changed by the push:\n{output}"
    );
}

#[test]
fn copying_an_array_of_strings_copies_the_elements() {
    // The copy constructor and copy assignment go through the same in-place
    // construction. Each element of the copy has to own a buffer, so changing
    // one array's text cannot change the other's.
    let source = r#"import std;

fn main() int {
    a Array<String>;
    s String = "short";
    a.push(s);
    b Array<String> = a;
    C.printf("copy distinct %d\n", a[0].data() != b[0].data());
    b[0].push('!');
    C.printf("a [%s]\n", a[0].data());
    C.printf("b [%s]\n", b[0].data());

    assigned Array<String>;
    assigned.push(s);
    assigned = a;
    C.printf("assign distinct %d\n", a[0].data() != assigned[0].data());
    return 0;
}
"#;
    let scratch = Scratch::new("copy-strings");
    let output = scratch.run("copy_strings", source);
    assert!(output.contains("copy distinct 1"), "{output}");
    assert!(output.contains("a [short]"), "{output}");
    assert!(output.contains("b [short!]"), "{output}");
    assert!(output.contains("assign distinct 1"), "{output}");
}

#[test]
fn a_popped_element_releases_its_buffer() {
    // The element's buffer is freed when the element is popped. The reuse is
    // made unambiguous by giving the source a much larger capacity than the
    // copied element: only the element's buffer is in the probe's size class,
    // so the address the probe receives is the one the popped element gave up.
    let source = r#"import std;

fn scoped() String {
    a Array<String>;
    s String;
    s.reserve(100);
    s.resize(40, 'x');
    a.push(s);
    C.printf("element %p\n", a[0].data());
    returned String = a.pop();
    C.printf("returned %p\n", returned.data());
    return returned;
}

fn main() int {
    kept String = scoped();
    C.printf("kept %p\n", kept.data());
    probe String = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    C.printf("reused %p\n", probe.data());
    C.printf("kept [%s] probe [%s]\n", kept.data(), probe.data());
    return 0;
}
"#;
    let scratch = Scratch::new("pop-release");
    let output = scratch.run("pop_release", source);
    assert_ne!(
        address(&output, "returned"),
        address(&output, "element"),
        "the popped value shares the buffer that was just released:\n{output}"
    );
    assert_eq!(
        address(&output, "reused"),
        address(&output, "element"),
        "the popped element's buffer was not released:\n{output}"
    );
    assert!(
        output.contains("kept [xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx]"),
        "the returned value was damaged by the release:\n{output}"
    );
    assert!(
        output.contains("probe [xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx]"),
        "the probe's text is wrong:\n{output}"
    );
}

#[test]
fn destroying_an_array_releases_its_elements() {
    // The array's destructor drops each element before freeing the buffer. The
    // probe is the same size as one element's buffer and different from the
    // source's, so the address it receives is one the elements gave up.
    let source = r#"import std;

fn scoped() void {
    a Array<String>;
    s String;
    s.reserve(100);
    s.resize(40, 'y');
    a.push(s);
    C.printf("element %p\n", a[0].data());
}

fn main() int {
    scoped();
    probe String = "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy";
    C.printf("reused %p\n", probe.data());
    C.printf("probe [%s]\n", probe.data());
    return 0;
}
"#;
    let scratch = Scratch::new("drop-release");
    let output = scratch.run("drop_release", source);
    assert_eq!(
        address(&output, "reused"),
        address(&output, "element"),
        "the array destructor did not release the element's buffer:\n{output}"
    );
    assert!(
        output.contains("probe [yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy]"),
        "{output}"
    );
}

#[test]
fn pushing_a_named_inner_array_copies_it() {
    // `Array<Array<int>>` was the case that already worked through assignment;
    // it has to keep working now that the element is constructed. A named inner
    // array is copied, so the two buffers differ, and growing the source after
    // the push leaves the element alone.
    let source = r#"import std;

fn main() int {
    inner Array<int>;
    inner.push(5);
    inner.push(6);
    outer Array<Array<int>>;
    outer.push(inner);
    C.printf("element distinct %d first %d\n", outer[0].data() != inner.data(), outer[0][0]);
    inner.push(7);
    C.printf("element len %lu inner len %lu\n", outer[0].length(), inner.length());

    copy Array<Array<int>> = outer;
    C.printf("copy distinct %d first %d\n", copy[0].data() != outer[0].data(), copy[0][0]);

    assigned Array<Array<int>>;
    one Array<int>;
    one.push(9);
    assigned.push(one);
    assigned = outer;
    C.printf("assign distinct %d first %d\n", assigned[0].data() != outer[0].data(), assigned[0][0]);
    return 0;
}
"#;
    let scratch = Scratch::new("nested-copy");
    let output = scratch.run("nested_copy", source);
    assert!(output.contains("element distinct 1 first 5"), "{output}");
    assert!(output.contains("element len 2 inner len 3"), "{output}");
    assert!(output.contains("copy distinct 1 first 5"), "{output}");
    assert!(output.contains("assign distinct 1 first 5"), "{output}");
}

#[test]
fn growing_an_array_of_arrays_keeps_the_elements_whole() {
    // Growth constructs each element into the new buffer and destroys the old
    // one. A deep element has to survive that: the outer buffer is new, and
    // every inner buffer is still owned by its element.
    let source = r#"import std;

fn main() int {
    outer Array<Array<int>>;
    index usize = 0;
    while (index < 20) {
        inner Array<int>;
        inner.push(index as int);
        inner.push((index * 10) as int);
        outer.push(inner);
        index += 1;
    }
    C.printf("len %lu\n", outer.length());
    C.printf("first %d %d last %d %d\n", outer[0][0], outer[0][1], outer[19][0], outer[19][1]);
    C.printf("distinct %d\n", outer[0].data() != outer[1].data());
    return 0;
}
"#;
    let scratch = Scratch::new("nested-grow");
    let output = scratch.run("nested_grow", source);
    assert!(output.contains("len 20"), "{output}");
    assert!(output.contains("first 0 0 last 19 190"), "{output}");
    assert!(output.contains("distinct 1"), "{output}");
}

#[test]
fn a_primitive_element_is_a_built_in_store() {
    // `int` cannot declare `__init__` or `__drop__`; its built-in forms are a
    // store and nothing at all. The same generic body therefore works for it,
    // and the operations still have to return the right values.
    let source = r#"import std;

fn main() int {
    v int = 1;
    p *int = &v;
    p->__init__(2);
    p->__assign__(3);
    p->__drop__();
    C.printf("builtin %d\n", v);

    a Array<int>;
    a.push(1);
    a.push(2);
    a.push(3);
    C.printf("len %lu first %d last %d\n", a.length(), a[0], a[2]);
    popped int = a.pop();
    C.printf("popped %d len %lu last %d\n", popped, a.length(), a[1]);
    a.insert(a.begin(), 7);
    C.printf("inserted %d len %lu\n", a[0], a.length());
    erased int = a.erase(a.begin() + 1);
    C.printf("erased %d first %d second %d\n", erased, a[0], a[1]);
    a.resize(3, 9);
    C.printf("resized %lu last %d\n", a.length(), a[2]);
    a.resize(1, 0);
    C.printf("shrunk %lu\n", a.length());
    a.clear();
    C.printf("cleared %lu\n", a.length());
    return 0;
}
"#;
    let scratch = Scratch::new("primitive");
    let output = scratch.run("primitive", source);
    assert!(output.contains("builtin 3"), "{output}");
    assert!(output.contains("len 3 first 1 last 3"), "{output}");
    assert!(output.contains("popped 3 len 2 last 2"), "{output}");
    assert!(output.contains("inserted 7 len 3"), "{output}");
    assert!(output.contains("erased 1 first 7 second 2"), "{output}");
    assert!(output.contains("resized 3 last 9"), "{output}");
    assert!(output.contains("shrunk 1"), "{output}");
    assert!(output.contains("cleared 0"), "{output}");
}

#[test]
fn the_push_body_constructs_the_element_with_init() {
    // The program's output cannot show *which* call made the copy; the IR can.
    // The `Array<String>` instantiation of `push` must call the element's
    // `__init__`, not `__assign__` and not a bare store.
    let source = r#"import std;

fn main() int {
    a Array<String>;
    s String = "a string";
    a.push(s);
    return 0;
}
"#;
    let scratch = Scratch::new("push-ir");
    let ir = scratch.ir("push", source);
    let body = body_after(&ir, "define void @\"push_R0N13_Array<String>_N6_String\"");
    assert!(
        body.contains("call void @__init___R0N6_String_"),
        "pushing a String did not construct the element:\n{body}"
    );
    assert!(
        !body.contains("call void @__assign___R0N6_String_"),
        "pushing a String assigned over the slot instead of constructing it:\n{body}"
    );
}
