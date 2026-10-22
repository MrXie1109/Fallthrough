//! Compiling several files into one program, and the One Definition Rule.
//!
//! `fallc a.ft b.ft -o prog` produces one program from several translation
//! units, so a function may be defined in one file and called from another.
//! That is the C model, and with it comes C's rule: one definition per symbol
//! across the program, but declarations as numerous as you like.

use std::path::PathBuf;

use fallc::cli::{EmitKind, Options};
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!("fallc-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, source).expect("the source can be written");
        path
    }

    /// Compiles the named files into one program, and runs it.
    ///
    /// Returns whether it built, the compiler's exit status, and what the
    /// program printed.
    fn build(&self, sources: &[PathBuf], output: &str) -> (bool, String) {
        let executable = self.directory.join(output);
        let mut args: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
        args.push("-o".to_owned());
        args.push(executable.display().to_string());
        let options = Options::parse(&args).expect("the arguments are well formed");
        if run(&options) != std::process::ExitCode::SUCCESS {
            return (false, String::new());
        }
        let run_output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        (true, String::from_utf8_lossy(&run_output.stdout).into_owned())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_function_defined_in_one_file_is_callable_from_another() {
    let scratch = Scratch::new("multi-ok");
    let library = scratch.write(
        "lib.ft",
        "import C;\nexport fn square(n int) int { return n * n; }\nexport fn announce() int {\n    C.printf(\"called\\n\");\n    return 0;\n}\n",
    );
    // The declaration has no body, so it is not a definition: the symbol comes
    // from the other file.
    let app = scratch.write(
        "app.ft",
        "import C;\nfn square(n int) int;\nfn announce() int;\nfn main() int {\n    announce();\n    C.printf(\"square(7) = %d\\n\", square(7));\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[app, library], "prog");
    assert!(built, "a cross-file call must link");
    assert!(output.contains("called"), "{output}");
    assert!(output.contains("square(7) = 49"), "{output}");
}

#[test]
fn two_modules_may_each_keep_a_private_helper() {
    // A private free function belongs to its module, so two modules may each
    // define `helper` with the same signature: the symbol carries the module.
    let scratch = Scratch::new("private-helper");
    let first = scratch.write(
        "first.ftm",
        "fn helper(n int) int {\n    return n + 1;\n}\n\nexport fn from_first() int {\n    return helper(1);\n}\n",
    );
    let second = scratch.write(
        "second.ftm",
        "fn helper(n int) int {\n    return n + 10;\n}\n\nexport fn from_second() int {\n    return helper(2);\n}\n",
    );
    let app = scratch.write(
        "app.ft",
        "import C;\nimport first;\nimport second;\nfn main() int {\n    C.printf(\"%d %d\\n\", first.from_first(), second.from_second());\n    return 0;\n}\n",
    );
    let _ = (&first, &second);
    let (built, output) = scratch.build(&[app], "prog");
    assert!(built, "two private helpers must not collide");
    assert_eq!(output.trim(), "2 12");
}

#[test]
fn a_symbol_defined_in_two_files_is_rejected() {
    let scratch = Scratch::new("odr-clash");
    let first = scratch.write("a.ft", "fn main() int { return 0; }\nfn shared() int { return 1; }\n");
    let second = scratch.write("b.ft", "fn shared() int { return 2; }\n");
    let (built, _) = scratch.build(&[first, second], "prog");
    assert!(
        !built,
        "two definitions of one symbol must be rejected rather than linked"
    );
}

#[test]
fn a_symbol_declared_in_several_files_is_fine() {
    // A declaration says the definition is elsewhere, so repeating it is the
    // whole point of having declarations at all.
    let scratch = Scratch::new("odr-decl");
    let library = scratch.write("lib.ft", "export fn answer() int { return 42; }\n");
    let first = scratch.write(
        "a.ft",
        "fn answer() int;\nfn helper() int { return answer(); }\n",
    );
    let second = scratch.write(
        "b.ft",
        "fn answer() int;\nfn main() int { return answer(); }\n",
    );
    let (built, _) = scratch.build(&[first, second, library], "prog");
    assert!(built, "a repeated declaration must be accepted");
}

#[test]
fn one_file_still_works_on_its_own() {
    // The single-file path is the common case and must not be disturbed by the
    // multi-file one.
    let scratch = Scratch::new("single");
    let main = scratch.write("main.ft", "import C;\nfn main() int {\n    C.puts(\"alone\");\n    return 0;\n}\n");
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built);
    assert_eq!(output.trim(), "alone");
}

#[test]
fn emit_all_writes_every_artefact() {
    // `-o` names the program; every other artefact is derived from it, so
    // asking for everything gives everything rather than one file overwritten
    // by the next stage.
    let scratch = Scratch::new("emit-all");
    let main = scratch.write(
        "main.ft",
        "type Point { x int; y int; }\nfn main() int { return 0; }\n",
    );
    let output = scratch.directory.join("prog");
    let options = Options::parse(&[
        main.display().to_string(),
        "--emit=all".to_owned(),
        "-o".to_owned(),
        output.display().to_string(),
    ])
    .expect("the arguments are well formed");
    assert_eq!(run(&options), std::process::ExitCode::SUCCESS);

    for suffix in ["", ".o", ".ll", ".types", ".ast", ".tokens"] {
        let path = PathBuf::from(format!("{}{suffix}", output.display()));
        assert!(path.is_file(), "`{}` was not written", path.display());
    }
}

#[test]
fn output_names_the_file_for_any_emit_kind() {
    // It used to apply only to the LLVM and object outputs, so `-o` with
    // `--emit=ast` silently printed to standard output instead.
    let scratch = Scratch::new("emit-o");
    let main = scratch.write("main.ft", "fn main() int { return 0; }\n");
    for (kind, suffix) in [
        (EmitKind::Tokens, "tok"),
        (EmitKind::Ast, "ast"),
        (EmitKind::Llvm, "ll"),
    ] {
        let target = scratch.directory.join(format!("out.{suffix}"));
        let options = Options::parse(&[
            main.display().to_string(),
            format!("--emit={}", kind_name(kind)),
            "-o".to_owned(),
            target.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(run(&options), std::process::ExitCode::SUCCESS);
        assert!(target.is_file(), "`{}` was not written", target.display());
        let written = std::fs::read_to_string(&target).expect("the file can be read");
        assert!(!written.is_empty(), "`{}` is empty", target.display());
    }
}

#[test]
fn a_type_is_reachable_through_the_umbrella_that_re_exports_it() {
    // `std.Array` is what a program that imported the umbrella writes, and
    // `std.array.Array` is what names the same thing through its own module.
    // Both work, and both mean one type: a program may use either without
    // thinking about which file the type lives in.
    let scratch = Scratch::new("umbrella-type");
    let main = scratch.write(
        "main.ft",
        "import C;\nimport std;\nfn main() int {\n    a std.Array<int> = std.Array<int>();\n    a.push(3);\n    C.printf(\"%d\\n\", a[0]);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a type named through the umbrella must resolve");
    assert_eq!(output.trim(), "3");
}

#[test]
fn a_constant_is_reachable_through_the_module_that_exports_it() {
    // A constant is the third kind of module member: neither callable nor a
    // type. `C.EOF` is the value it names.
    let scratch = Scratch::new("module-constant");
    let limits = scratch.write("lim.ftm", "export LIMIT const int = 42;\n");
    let main = scratch.write(
        "main.ft",
        "import C;\nimport lim;\nfn main() int {\n    C.printf(\"%d\\n\", lim.LIMIT);\n    return 0;\n}\n",
    );
    let _ = &limits;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an exported constant must be nameable");
    assert_eq!(output.trim(), "42");
}

#[test]
fn a_destructor_defined_in_another_module_runs() {
    // A destructor is a function, and its call is emitted by the module that
    // *uses* the type -- which is the module that has not been given the
    // definition yet, because the defining module is lowered afterwards.
    // Looking only at what this module already contains found nothing, so
    // every imported type's destructor was silently skipped.
    //
    // The destructor is `export`ed, as `std.String`'s is: a name a module does
    // not publish is its own business, and that rule does not change because
    // the name happens to be `__drop__`.
    let scratch = Scratch::new("drop-across");
    let library = scratch.write(
        "thing.ftm",
        "import C;\nexport type Thing { v int; }\nexport fn __drop__(this &Thing) void {\n    C.printf(\"dropped %d\\n\", this.v);\n}\nexport fn make_thing(v int) Thing {\n    t Thing = .{v};\n    return t;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport thing;\nfn main() int {\n    t Thing = thing.make_thing(9);\n    C.printf(\"alive %d\\n\", t.v);\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a destructor must cross a module boundary");
    // The value is alive while `main` runs, and destroyed as it leaves.
    assert!(output.contains("alive 9"), "{output}");
    assert!(output.contains("dropped 9"), "{output}");
}

#[test]
fn an_auto_return_is_known_to_the_importing_module() {
    // A definition without a return type has one, deduced from its own body --
    // which only its module can see. The export carries the resolved
    // signature, so an importer can use the result instead of being told to
    // write the type out.
    let scratch = Scratch::new("auto-across");
    let library = scratch.write(
        "lib.ftm",
        "export type Pair {\n    a int;\n    b int;\n}\n\nexport fn make(n int) {\n    p Pair = .{n, n * 2};\n    return p;\n}\n\nexport fn label(n int) {\n    return n > 0 ? \"pos\" : \"neg\";\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport lib;\nfn main() int {\n    p lib.Pair = lib.make(4);\n    C.printf(\"%d %d %s\\n\", p.a, p.b, lib.label(-1));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a deduced return type must cross a module boundary");
    assert_eq!(output.trim(), "4 8 neg");
}

#[test]
fn an_enum_crosses_a_module_boundary() {
    // An enum is a type whose members are named constants of it. The type, the
    // members' values and the base type all have to cross: `Color.BLUE` in
    // another file is the number this module folded, and a function taking a
    // `Color` takes this module's enum.
    let scratch = Scratch::new("enum-across");
    let library = scratch.write(
        "kinds.ftm",
        "export enum Color int {\n    RED = 1,\n    GREEN,\n    BLUE = 7,\n}\n\nexport fn describe(c Color) int {\n    return c * 2;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom kinds import Color;\nimport kinds;\n\nfn main() int {\n    c Color = Color.BLUE;\n    d Color = kinds.Color.GREEN;\n    C.printf(\"%d %d %d\\n\", c, d, kinds.describe(c));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an enum must cross a module boundary");
    assert_eq!(output.trim(), "7 2 14");
}

#[test]
fn an_interface_crosses_a_module_boundary() {
    // The value's layout is the same everywhere, so what crosses is the method
    // list. A type from another module satisfies it, the table is built where
    // the conversion is written, and an interface may become a narrower one.
    let scratch = Scratch::new("interface-across");
    let library = scratch.write(
        "shapes.ftm",
        "export type Square {\n    side int;\n}\n\nexport fn area(this &const Square) int {\n    return this.side * this.side;\n}\n\nexport fn grow(this &Square, by int) int {\n    this.side += by;\n    return area(this);\n}\n\nexport interface IShape(it T) {\n    it.area() -> int,\n}\n\nexport interface IMetric(it T) {\n    it.area() -> int,\n}\n\nexport interface ICount(it T) {\n    it.grow(int) -> int,\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom shapes import Square, IShape, ICount, IMetric;\n\nfn main() int {\n    s Square = .{3};\n    a IShape = s;\n    narrow IMetric = a;\n    c ICount = s;\n    C.printf(\"%d %d %d\\n\", a.area(), narrow.area(), c.grow(2));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an interface must cross a module boundary");
    assert_eq!(output.trim(), "9 9 25");
}

#[test]
fn a_throwing_interface_method_crosses_a_module_boundary() {
    let scratch = Scratch::new("interface-across-throws");
    let library = scratch.write(
        "engine.ftm",
        "export type Engine {\n    fuel int;\n}\n\nexport fn start(this &Engine) int except {\n    if (this.fuel == 0) {\n        raise \"E\": \"empty\";\n    }\n    return this.fuel;\n}\n\nexport interface IEngine(it T) {\n    it.start()?! -> int,\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom engine import Engine, IEngine;\n\nfn main() int {\n    e Engine = .{0};\n    it IEngine = e;\n    try {\n        C.printf(\"%d\\n\", it.start());\n    } except {\n        C.printf(\"caught\\n\");\n    }\n    e.fuel = 5;\n    it2 IEngine = e;\n    try {\n        C.printf(\"%d\\n\", it2.start());\n    } except {\n        C.printf(\"caught 2\\n\");\n    }\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a throwing interface method must cross a module boundary");
    assert_eq!(output.trim(), "caught\n5");
}

#[test]
fn an_imported_interface_checks_its_restrictions_where_the_box_is_written() {
    // A restriction is an expression, so it travels with the tree it was
    // written in. Boxing in another module asks the declaring module's
    // question: `Square` has `area`, so the first box holds; the second one adds
    // `scale`, which it does not have, and the box is refused.
    let scratch = Scratch::new("interface-restriction-across");
    let main = scratch.write(
        "main.ft",
        "from shapes import Square, IShape;\n\nfn main() int {\n    s Square = .{3};\n    it IShape = s;\n    return it.area();\n}\n",
    );
    let library = scratch.write(
        "shapes.ftm",
        "export type Square {\n    side int;\n}\n\nexport fn area(this &const Square) int {\n    return this.side * this.side;\n}\n\nexport interface IShape(it T) {\n    it.area() -> int,\n}\n",
    );
    let (built, output) = scratch.build(std::slice::from_ref(&main), "prog");
    assert!(built, "the box holds while the restriction does: {output}");
    let _ = &library;
    // Now the same interface also asks for a method the type does not have.
    scratch.write(
        "shapes.ftm",
        "export type Square {\n    side int;\n}\n\nexport fn area(this &const Square) int {\n    return this.side * this.side;\n}\n\nexport interface IShape(it T) {\n    it.area() -> int,\n    it.scale(int) -> int,\n}\n",
    );
    let (built, _) = scratch.build(&[main], "prog2");
    assert!(
        !built,
        "a restriction the type fails must be reported where the box is written"
    );
}

#[test]
fn a_named_import_brings_every_kind_of_member() {
    // A module exports functions, types, constants, variables, enums and
    // interfaces, and `from m import x` names any of them. Asking only about
    // functions reported a name the module *does* export as private.
    let scratch = Scratch::new("import-kinds");
    let library = scratch.write(
        "stuff.ftm",
        "export SMALL const u8 = 200;\n\nexport type Point {\n    x int;\n}\n\nexport enum Kind int {\n    A,\n    B,\n}\n\nexport interface IKind(it T) {\n    it.kind() -> int,\n}\n\nexport fn kind(this &const Point) int {\n    return 1;\n}\n\nexport fn twice(n int) int {\n    return n * 2;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom stuff import SMALL, Point, Kind, IKind, twice;\n\nfn take(v u8) int {\n    return v as int;\n}\n\nfn main() int {\n    p Point = .{1};\n    k Kind = Kind.B;\n    it IKind = p;\n    C.printf(\"%d %d %d %d\\n\", take(SMALL), k, it.kind(), twice(21));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "every kind of exported member must be importable by name");
    assert_eq!(output.trim(), "200 1 1 42");
}

#[test]
fn a_star_import_brings_every_kind_of_member() {
    let scratch = Scratch::new("import-star");
    let library = scratch.write(
        "all.ftm",
        "export LIMIT const int = 42;\n\nexport type Pair {\n    a int;\n    b int;\n}\n\nexport enum Tint int {\n    RED,\n    BLUE,\n}\n\nexport interface IPair(it T) {\n    it.sum() -> int,\n}\n\nexport fn sum(this &const Pair) int {\n    return this.a + this.b;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom all import *;\n\nfn main() int {\n    p Pair = .{1, 2};\n    t Tint = Tint.BLUE;\n    it IPair = p;\n    C.printf(\"%d %d %d\\n\", LIMIT, t, it.sum());\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "`from m import *` must bring every kind of member");
    assert_eq!(output.trim(), "42 1 3");
}

#[test]
fn an_aligned_field_keeps_its_offset_across_a_module() {
    // A field's `alignas` is part of the type's *layout*: the module that
    // declares the type writes `b` at offset 16, so a module that dropped the
    // alignment read it at offset 4 and got the bytes of something else.
    let scratch = Scratch::new("align-across");
    let library = scratch.write(
        "aligned.ftm",
        "export type Holder {\n    a byte;\n    b int @align(16);\n}\n\nexport fn get(this &const Holder) int {\n    return this.b;\n}\n\nexport fn make(v int) Holder {\n    h Holder;\n    h.b = v;\n    return h;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom aligned import Holder, get, make;\n\nfn main() int {\n    h Holder = make(42);\n    C.printf(\"%d %d %d\\n\", h.b, get(h), sizeof(Holder) as int);\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a field's alignment must cross a module boundary");
    assert_eq!(output.trim(), "42 42 32");
}

#[test]
fn a_mutable_field_is_writable_across_a_module() {
    // `mutable` is part of the type too: a `const` member function may write
    // that field, and a module that imported the type has to agree.
    let scratch = Scratch::new("mutable-across");
    let library = scratch.write(
        "counter.ftm",
        "export type Counter {\n    n int;\n    total int mutable;\n}\n\nexport fn bump(this &const Counter) int {\n    this.total += 1;\n    return this.total;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom counter import Counter, bump;\n\nfn main() int {\n    c Counter = .{1, 0};\n    C.printf(\"%d %d\\n\", bump(c), bump(c));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a `mutable` field must stay writable across a module");
    assert_eq!(output.trim(), "1 2");
}

#[test]
fn an_imported_function_template_is_instantiated_here() {
    // A template declared in a module travels as its *body*, registered under
    // the symbol its declaration mangles to. Instantiating it here means
    // checking that tree, not this one: node ids are indices into a single tree.
    let scratch = Scratch::new("template-across");
    let library = scratch.write(
        "lib.ftm",
        "export\ntemplate <T type>\nfn twice(v T) T {\n    return v + v;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom lib import twice;\n\nfn main() int {\n    C.printf(\"%d\\n\", twice(21));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an imported function template must instantiate");
    assert_eq!(output.trim(), "42");
}

#[test]
fn a_template_body_uses_its_own_modules_private_type() {
    // A body is checked where it is instantiated, but its names belong to the
    // module that wrote it: `Helper` is private there, and `WIDTH` is its
    // constant. Neither exists in the importing file.
    let scratch = Scratch::new("template-private");
    let library = scratch.write(
        "lib.ftm",
        "type Helper {\n    x int;\n}\n\nWIDTH const int = 3;\n\nexport\ntemplate <T type>\nfn pick(v int) T {\n    h Helper = .{v};\n    return (h.x + WIDTH) as T;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "from lib import pick;\n\nfn main() int {\n    return pick<int>(5);\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a template body must see its own module's declarations");
    assert_eq!(output.trim(), "");
}

#[test]
fn a_type_declared_here_shadows_an_imported_family() {
    // A file's own declaration is what its names mean, so an imported family
    // called `Box` steps aside for a `type Box` the program writes -- and the
    // module's templates still use *their* `Box`, which is a different type.
    let scratch = Scratch::new("shadow-family");
    let _library = scratch.write(
        "boxmod.ftm",
        "export\ntemplate <T type>\ntype Box {\n    v T;\n}\n\nexport fn make(v int) Box<int> {\n    b Box<int>;\n    b.v = v;\n    return b;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport std;\nfrom boxmod import make;\n\ntype Box {\n    value int;\n}\n\nfn main() int {\n    b Box = .{3};\n    C.printf(\"%d %d\\n\", b.value, make(4).v);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a local type must shadow an imported family name");
    assert_eq!(output.trim(), "3 4");
}

#[test]
fn an_interface_inside_a_template_body_is_the_modules_own() {
    // A conversion inside a body looks the interface up by name, through the
    // body's module's tables: the private type, the interface and the method
    // all belong to that module.
    let scratch = Scratch::new("template-interface");
    let library = scratch.write(
        "lib.ftm",
        "export type Node {\n    v int;\n}\n\nexport fn value(this &const Node) int {\n    return this.v;\n}\n\nexport interface IValue(it T) {\n    it.value() -> int,\n}\n\nexport\ntemplate <T type>\nfn as_value(x T) int {\n    n Node = .{1};\n    it IValue = n;\n    return it.value();\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom lib import as_value;\n\nfn main() int {\n    C.printf(\"%d\\n\", as_value<int>(0));\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an interface inside a template body must resolve there");
    assert_eq!(output.trim(), "1");
}

#[test]
fn two_families_with_one_name_are_reported() {
    // The two cannot be told apart by name, so the program is refused with a
    // diagnostic instead of one family's members standing in for the other's.
    let scratch = Scratch::new("family-clash");
    let _library = scratch.write(
        "boxmod.ftm",
        "export\ntemplate <T type>\ntype Box {\n    v T;\n}\n\nexport fn make(v int) Box<int> {\n    b Box<int>;\n    b.v = v;\n    return b;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport std;\nimport boxmod;\n\ntemplate <T type>\ntype Box {\n    value T;\n}\n\nfn main() int {\n    b Box<int> = .{3};\n    C.printf(\"%d %d\\n\", b.value, boxmod.make(4).v);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(!built, "two families under one name must be refused");
    assert_eq!(output, "");
}

#[test]
fn a_nested_alias_of_an_exported_type_crosses_a_module() {
    // `Outer.Inner` is a name the owner introduces; an importer that could name
    // `Outer` but not its member would have half of the type's namespace.
    let scratch = Scratch::new("nested-across");
    let _library = scratch.write(
        "nest.ftm",
        "export type Outer {\n    v int;\n\n    type Inner = int;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom nest import Outer;\n\nfn main() int {\n    o Outer = .{1};\n    i Outer.Inner = 7;\n    C.printf(\"%d %d\\n\", o.v, i);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a nested alias must cross a module boundary");
    assert_eq!(output.trim(), "1 7");
}

#[test]
fn an_exported_macro_reaches_an_importer() {
    // `export define` publishes the definition itself: a macro is expanded
    // before parsing, so the module is read at the token level before this file
    // is expanded. What its body uses travels with it -- here `mac` is written
    // in terms of a macro `inner` exported.
    let scratch = Scratch::new("macro-across");
    let inner = scratch.write(
        "inner.ftm",
        "export define BASE 3\n\nexport define DOUBLED ($BASE * 2)\n",
    );
    let umbrella = scratch.write("umb.ftm", "export from inner import *;\n");
    let main = scratch.write(
        "main.ft",
        "import C;\nimport umb;\n\nfn main() int {\n    C.printf(\"%d %d\\n\", $BASE, $DOUBLED);\n    return 0;\n}\n",
    );
    let _ = (&inner, &umbrella);
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an exported macro must reach an importer");
    assert_eq!(output.trim(), "3 6");
}

#[test]
fn a_local_definition_shadows_an_imported_macro() {
    // A file's own `define` is its own: importing a module must not make its
    // macro impossible to override, exactly as a local declaration shadows an
    // imported name.
    let scratch = Scratch::new("macro-shadow");
    let _library = scratch.write("limit.ftm", "export define LIMIT 3\n");
    let main = scratch.write(
        "main.ft",
        "import C;\nimport limit;\n\ndefine LIMIT 100\n\nfn main() int {\n    C.printf(\"%d\\n\", $LIMIT);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a local macro must shadow an imported one");
    assert_eq!(output.trim(), "100");
}

#[test]
fn a_deprecation_crosses_a_module_boundary() {
    // `@deprecated` is part of what a declaration publishes: an importer that
    // used the name without a word would be told nothing about the replacement.
    let scratch = Scratch::new("deprecated-across");
    let library = scratch.write(
        "lib.ftm",
        "@deprecated(\"use fresh\")\nexport fn stale() int {\n    return 2;\n}\n\nexport fn fresh() int {\n    return 9;\n}\n\n@deprecated(\"set BUILD instead\")\nexport BUILD u64 = 7;\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom lib import stale, BUILD;\n\nfn main() int {\n    C.printf(\"%d %d\\n\", stale(), BUILD as int);\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a deprecation must cross a module boundary");
    assert_eq!(output.trim(), "2 7");
}

#[test]
fn a_declaration_removed_by_enable_if_is_not_exported() {
    // `@enable_if(false)` removes the declaration for this build. Publishing it
    // anyway let an importer call a function nothing defines, and the link
    // failed with no diagnostic that mentioned the declaration.
    let scratch = Scratch::new("disabled-across");
    let _library = scratch.write(
        "lib.ftm",
        "@enable_if(false)\nexport fn gone() int {\n    return 1;\n}\n\nexport fn kept() int {\n    return 2;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom lib import gone;\n\nfn main() int {\n    C.printf(\"%d\\n\", gone());\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(!built, "a removed declaration must not be exported");
    assert_eq!(output, "");
}

#[test]
fn an_exported_alias_is_the_type_it_names() {
    // An alias introduces no type of its own: `type Id = usize;` makes `Id`
    // *be* `usize`. Exported as a nominal type it became an empty struct, and no
    // value of the aliased type fitted it -- including a field written through
    // the alias, which is why the alias is substituted before the type leaves
    // the module.
    let scratch = Scratch::new("alias-across");
    let library = scratch.write(
        "ids.ftm",
        "export type Id = usize;\n\nexport type Boxed {\n    id Id;\n    n int;\n}\n\nexport fn make(v Id) Boxed {\n    b Boxed = .{v, 2};\n    return b;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom ids import Id, Boxed, make;\n\nfn main() int {\n    x Id = 7;\n    b Boxed = make(x);\n    C.printf(\"%d %d %d %d\\n\", x, sizeof(Id) as int, sizeof(Boxed) as int, b.id as int);\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an exported alias must be the type it names");
    assert_eq!(output.trim(), "7 8 16 7");
}

#[test]
fn an_exported_global_is_one_object_for_the_program() {
    // A module-level variable is one object for the whole run, so an importer
    // reads and writes the *same* storage. `export` used to be accepted and
    // silently ignored on a variable, which meant another module could not see
    // it at all.
    let scratch = Scratch::new("global-across");
    let library = scratch.write(
        "state.ftm",
        "export counter int = 7;\n\nexport seed auto = 11;\n\nexport fn bump() int {\n    counter += 1;\n    return counter;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom state import counter, seed, bump;\nimport state;\n\nfn main() int {\n    C.printf(\"%d %d\\n\", counter, seed);\n    counter = 20;\n    C.printf(\"%d %d %d\\n\", bump(), counter, state.counter);\n    state.counter = 30;\n    C.printf(\"%d\\n\", counter);\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an exported global must be one object for the program");
    assert_eq!(output.trim(), "7 11\n21 21 21\n30");
}

#[test]
fn an_unexported_global_is_not_importable() {
    // The other half of the rule: a variable a module did not publish is its
    // own business, exactly as an unexported function is.
    let scratch = Scratch::new("global-private");
    let _library = scratch.write("state.ftm", "hidden int = 7;\n");
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom state import hidden;\n\nfn main() int {\n    C.printf(\"%d\\n\", hidden);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "prog");
    assert!(!built, "an unexported global must not be importable");
    assert_eq!(output, "");
}

#[test]
fn an_exported_trait_is_importable_by_name() {
    // A trait's requirements are expressions, so they travel with the declaring
    // module's tree; its name is published like any other member, and naming it
    // in a `from` import is not "does not export".
    let scratch = Scratch::new("trait-across");
    let library = scratch.write(
        "rules.ftm",
        "export traits Incrementable(it T) {\n    ++it,\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nfrom rules import Incrementable;\n\ntype Counts {\n    n int;\n}\n\nfn __inc__(this &Counts) Counts {\n    this.n += 1;\n    return this;\n}\n\nfn main() int {\n    if (Incrementable(Counts)) @constexpr {\n        C.printf(\"yes\\n\");\n    }\n    return 0;\n}\n",
    );
    let _ = &library;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "an exported trait must be importable by name");
    assert_eq!(output.trim(), "yes");
}

#[test]
fn a_union_keeps_its_layout_across_a_module_boundary() {
    // A union's layout *is* what makes it a union. Reading one as a struct on
    // the other side of a boundary would put the members side by side, and
    // every value crossing would be wrong -- so the kind travels with the
    // fields.
    let scratch = Scratch::new("union-across");
    let value = scratch.write(
        "val.ftm",
        "export union Value {\n    i int;\n    b byte;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport val;\nfn main() int {\n    x Value;\n    x.i = 65;\n    C.printf(\"%d %d %lu\\n\", x.i, x.b as int, sizeof(Value));\n    return 0;\n}\n",
    );
    let _ = &value;
    let (built, output) = scratch.build(&[main], "prog");
    assert!(built, "a union must cross a module boundary");
    // The byte and the int overlap, and the type is the largest member wide.
    assert_eq!(output.trim(), "65 65 4");
}

/// Returns the name `--emit` uses for a kind.
fn kind_name(kind: EmitKind) -> &'static str {
    match kind {
        EmitKind::Tokens => "tokens",
        EmitKind::Ast => "ast",
        EmitKind::Types => "types",
        EmitKind::Llvm => "llvm",
        EmitKind::Object => "obj",
        EmitKind::Static => "static",
        EmitKind::Shared => "shared",
        EmitKind::Executable => "exe",
    }
}

#[test]
fn a_local_module_may_share_a_name_with_a_standard_one() {
    // A file of one's own called `math.ftm` competes with the C bindings'
    // `C/math.ftm`, which is only reachable as `C.math`. Registering every
    // module under its last segment made the two the same name, and which one
    // a program got depended on the order modules happened to be loaded in.
    let scratch = Scratch::new("module-name");
    let local = scratch.write(
        "math.ftm",
        "export fn triple(n int) int {\n    return n * 3;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport math;\nfn main() int {\n    C.printf(\"%d\\n\", math.triple(3));\n    return 0;\n}\n",
    );
    // Three times, because the fault was that one run could differ from the
    // next: a module name is its identity, and the answer must not move.
    for attempt in 0..3 {
        let (built, output) = scratch.build(std::slice::from_ref(&main), "name");
        assert!(built, "attempt {attempt} did not build");
        assert_eq!(output.trim(), "9", "attempt {attempt}: {output}");
    }
    let _ = local;
}

#[test]
fn two_files_providing_one_module_name_are_reported() {
    // `lib/sub.ftm` and the program both import `dup`, and each finds a
    // different file. One of them silently winning is what made a build depend
    // on load order, so both are named instead.
    let scratch = Scratch::new("module-clash");
    let directory = scratch.directory.clone();
    std::fs::create_dir_all(directory.join("lib")).expect("a subdirectory can be created");
    std::fs::write(
        directory.join("dup.ftm"),
        "export fn one() int {\n    return 1;\n}\n",
    )
    .expect("the source can be written");
    std::fs::write(
        directory.join("lib/dup.ftm"),
        "export fn two() int {\n    return 2;\n}\n",
    )
    .expect("the source can be written");
    std::fs::write(
        directory.join("lib/sub.ftm"),
        "import dup;\nexport fn value() int {\n    return dup.two();\n}\n",
    )
    .expect("the source can be written");
    let main = scratch.write(
        "main.ft",
        "import C;\nimport lib.sub;\nimport dup;\nfn main() int {\n    C.printf(\"%d\\n\", dup.one() + sub.value());\n    return 0;\n}\n",
    );
    let (built, _) = scratch.build(&[main], "clash");
    assert!(!built, "two files claiming one module name must be refused");
}

#[test]
fn an_imported_type_can_be_constructed_by_its_name() {
    // `Type(args)` is a construction wherever the type is named: written bare,
    // or through the module it came from. Only the bare form worked, because
    // the qualified one went through the *function* lookup of the module and a
    // type is not a function.
    let scratch = Scratch::new("imported-ctor");
    scratch.write(
        "pair.ftm",
        "export type Pair {\n    left int;\n    right int;\n}\nexport fn __init__(this &Pair, a int, b int) void {\n    this.left = a;\n    this.right = b;\n}\nexport fn sum(this &const Pair) int {\n    return this.left + this.right;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport pair;\nfn main() int {\n    a pair.Pair = pair.Pair(3, 4);\n    b Pair = Pair(5, 6);\n    C.printf(\"%d %d\\n\", a.sum(), b.sum());\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "ctor");
    assert!(built, "an imported type must be constructible");
    assert_eq!(output.trim(), "7 11");
}

#[test]
fn an_exported_type_may_mention_itself() {
    // A linked list is the shape this exists for: the module declares `Node`
    // with a `next *Node`, and a program it is imported into builds one. The
    // importing side has to learn the fields across the boundary, which the
    // export fixpoint does in rounds.
    let scratch = Scratch::new("recursive-export");
    scratch.write(
        "node.ftm",
        "import std.util;\nexport type Node {\n    value int;\n    next *Node;\n}\nexport fn make(value int) *Node {\n    node *Node = util.alloc(sizeof(Node)) as *Node;\n    node->value = value;\n    node->next = nullptr;\n    return node;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import C;\nimport node;\nfn main() int {\n    first *Node = node.make(7);\n    second *Node = node.make(9);\n    first->next = second;\n    C.printf(\"%d %d\\n\", first->value, first->next->value);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "recursive");
    assert!(built, "a recursive type must cross a module boundary");
    assert_eq!(output.trim(), "7 9");
}

#[test]
fn an_exported_forward_declaration_crosses_as_an_incomplete_type() {
    // `export type Node;` publishes the name and nothing else, so an importer
    // may write `*Node` -- the shape a module uses to hand out opaque handles.
    // The declaration used to be dropped from the export table, which reported
    // every importer as naming a type the module does not export.
    let scratch = Scratch::new("forward-across");
    let _library = scratch.write(
        "node.ftm",
        "export type Node;\n\nexport fn make() *Node {\n    return nullptr;\n}\n\nexport fn missing(p *Node) int {\n    return p == nullptr ? 1 : 0;\n}\n",
    );
    let main = scratch.write(
        "main.ft",
        "import std;\nfrom node import Node, make, missing;\n\nfn main() int {\n    p *Node = make();\n    C.printf(\"%d\\n\", missing(p));\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "fwd");
    assert!(built, "an exported declaration must be nameable elsewhere");
    assert_eq!(output.trim(), "1");
}

#[test]
fn a_declaration_after_the_definition_does_not_hide_it() {
    // A module may declare a type it has already defined -- a header-style
    // repeat. The definition is what an importer has to see, so the declaration
    // must not put the type back to incomplete.
    let scratch = Scratch::new("forward-after");
    let _library = scratch.write(
        "node.ftm",
        "export type Node {\n    value int;\n}\n\nexport type Node;\n",
    );
    let main = scratch.write(
        "main.ft",
        "import std;\nfrom node import Node;\n\nfn main() int {\n    n Node = .{7};\n    C.printf(\"%d\\n\", n.value);\n    return 0;\n}\n",
    );
    let (built, output) = scratch.build(&[main], "after");
    assert!(built, "a repeated declaration must not make the type incomplete");
    assert_eq!(output.trim(), "7");
}
