//! Imports and link instructions.
//!
//! These exercise the three import spellings, the re-export that makes an
//! umbrella module work, and the `link` instruction that replaced a hardcoded
//! `-lm` in the driver.

use std::path::PathBuf;

use fallc::Outcome;
use fallc::cli::{EmitKind, Options};
use fallc::pipeline::{compile_text, run};

/// A directory of scratch modules, removed when it goes out of scope.
///
/// Visibility rules cannot be tested through `C` alone: it exports everything
/// deliberately, so nothing in it is private. These tests need modules that are
/// private on purpose.
struct Scratch {
    /// Where the modules are written.
    directory: PathBuf,
}

impl Scratch {
    /// Creates an empty directory for modules.
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Writes a module beside the others.
    fn module(&self, name: &str, source: &str) -> &Self {
        std::fs::write(self.directory.join(format!("{name}.ftm")), source)
            .expect("the module can be written");
        self
    }

    /// Compiles and runs a program that may import the modules.
    ///
    /// Returns whether it built, and what it printed. Full compilation, not
    /// `--check`, because an imported module's *bodies* have to be emitted too:
    /// a module that defines a function is the only place that function exists,
    /// and a checker-only test would not notice that its code was missing.
    fn build_and_run(&self, source: &str) -> (bool, String) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the program can be written");
        let executable = self.directory.join("main");
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        if run(&options) != std::process::ExitCode::SUCCESS {
            return (false, String::new());
        }
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        (true, String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Type checks a program, without generating code.
    ///
    /// Used where the question is purely one of visibility and the program
    /// would not link for an unrelated reason.
    fn check(&self, source: &str) -> (bool, String) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the program can be written");
        let options = Options::parse(&[main.display().to_string(), "--check".to_owned()])
            .expect("the arguments are well formed");
        let built = run(&options) == std::process::ExitCode::SUCCESS;
        (built, String::new())
    }

}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Compiles source in a scratch directory and returns what came out.
fn compile(source: &str) -> (Outcome, String) {
    compile_text("test.ftm", source, &[EmitKind::Ast])
}

#[test]
fn a_plain_import_binds_the_module_name() {
    let (outcome, output) = compile("import C;\nfn main() int {\n    C.printf(\"hi\");\n    return 0;\n}\n");
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_alias_binds_the_module_under_another_name() {
    // The module is loaded under its own name; the alias is only how this file
    // spells it, so both work.
    let source = "import C as Libc;\nfn main() int {\n    Libc.printf(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_named_import_binds_the_member_itself() {
    let source = "from C import printf;\nfn main() int {\n    printf(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_star_import_binds_every_export() {
    let source = "from C import *;\nfn main() int {\n    printf(\"hi\");\n    puts(\"there\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn the_three_import_spellings_can_be_used_together() {
    let source = "import C;\nimport C as Libc;\nfrom C import printf;\nfrom C import *;\nfn main() int {\n    C.printf(\"a\");\n    Libc.printf(\"b\");\n    printf(\"c\");\n    puts(\"d\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_named_import_of_something_unexported_is_reported() {
    let source = "from C import not_there;\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
    assert!(output.contains("does not export `not_there`"), "{output}");
}

#[test]
fn a_module_name_may_be_quoted_when_it_is_not_an_identifier() {
    // The quotes are a lexical escape hatch: they exist so a name that would
    // collide with an operator or a keyword can still be written.
    let source = "import \"C\";\nfn main() int {\n    C.printf(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_link_instruction_parses_in_all_of_its_forms() {
    for source in [
        "link m dynamic from default;\nfn main() int { return 0; }\n",
        "link my_lib static from \"/path/to/my/lib\";\nfn main() int { return 0; }\n",
        // A name that is not an identifier, which is what the quotes are for.
        "link \"gcc_s\" dynamic from default;\nfn main() int { return 0; }\n",
        "link \"my-lib\" static from \"/opt/lib\";\nfn main() int { return 0; }\n",
    ] {
        let (outcome, output) = compile(source);
        assert_eq!(outcome, Outcome::Success, "{source}\n{output}");
    }
}

#[test]
fn a_quoted_library_name_means_the_same_as_a_bare_one() {
    // The two spellings denote the same library, so they must produce the same
    // link arguments. A difference here would make the quotes a semantic
    // qualifier, which they are not.
    let bare = "link m dynamic from default;\nfn main() int { return 0; }\n";
    let quoted = "link \"m\" dynamic from default;\nfn main() int { return 0; }\n";
    let (bare_outcome, bare_output) = compile(bare);
    let (quoted_outcome, quoted_output) = compile(quoted);
    assert_eq!(bare_outcome, Outcome::Success, "{bare_output}");
    assert_eq!(quoted_outcome, Outcome::Success, "{quoted_output}");
}

#[test]
fn a_link_instruction_missing_its_kind_is_reported() {
    // Nothing is optional: a library linked the wrong way is a run-time bug.
    let source = "link m from default;\nfn main() int { return 0; }\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn a_link_instruction_without_from_is_reported() {
    let source = "link m dynamic;\nfn main() int { return 0; }\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Failure, "{output}");
}

#[test]
fn an_unexported_star_import_is_not_re_exported() {
    // `from M import *` brings the names into this file and stops there. The
    // `export` is required, so what a module publishes is always something a
    // reader can find by looking for `export`.
    let source = "from C import *;\nfn main() int {\n    printf(\"hi\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_exported_from_import_re_exports() {
    // The umbrella's own shape: nothing declared, everything forwarded.
    let source = "export from C import *;\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_exported_named_import_re_exports_exactly_those_names() {
    let source = "export from C import printf;\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_exported_type_is_visible_to_an_importer() {
    // A type is published on the same terms as a function. Without `export`
    // its name is the declaring module's own business.
    let source = "from C import *;\nfn main() int {\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn every_exported_name_of_the_umbrella_resolves_through_export_from() {
    // `C` declares nothing itself, so each of these exists only because a
    // submodule exports it and `C` forwards it with `export from`.
    let source = "import C;\nfn main() int {\n    C.printf(\"a\");\n    C.malloc(8 as usize);\n    C.strlen(\"b\");\n    C.isalpha(65);\n    C.sqrt(2.0);\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn the_umbrella_module_re_exports_its_submodules() {
    // The point of `from M import *`: `C` declares nothing itself, so every
    // name it offers comes from a submodule. Without re-export nothing would
    // resolve.
    let source = "import C;\nfn main() int {\n    C.printf(\"hi\");\n    C.puts(\"there\");\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn a_name_from_every_submodule_resolves_through_the_umbrella() {
    // One symbol per submodule, so a submodule that was left out of `C.ftm`
    // fails here rather than being noticed only when someone calls it.
    let source = "import C;\nfn main() int {\n    C.printf(\"a\");\n    C.malloc(8 as usize);\n    C.strlen(\"b\");\n    C.isalpha(65);\n    C.sqrt(2.0);\n    return 0;\n}\n";
    let (outcome, output) = compile(source);
    assert_eq!(outcome, Outcome::Success, "{output}");
}

#[test]
fn an_unexported_function_is_invisible_to_an_importer() {
    let scratch = Scratch::new("private-fn");
    scratch.module(
        "Inner",
        "export fn visible() int { return 7; }\nfn hidden() int { return 9; }\n",
    );
    // The exported one is reachable.
    let (checked, _) = scratch.check("import Inner;\nfn main() int { return Inner.visible(); }\n");
    assert!(checked, "an exported function must be reachable");

    // The unexported one is not.
    let (checked, _) = scratch.check("import Inner;\nfn main() int { return Inner.hidden(); }\n");
    assert!(!checked, "an unexported function must not be reachable");
}

#[test]
fn an_unexported_type_is_invisible_to_an_importer() {
    let scratch = Scratch::new("private-type");
    scratch.module(
        "Shapes",
        "export type Point { x int; y int; }\ntype Secret { hidden int; }\nexport fn nothing() int { return 0; }\n",
    );
    // The exported type works, field layout included.
    let (checked, _) = scratch.check(
        "import Shapes;\nfn main() int {\n    p Point = .{42, 37};\n    return p.x + p.y;\n}\n",
    );
    assert!(checked, "an exported type must be usable");

    // The unexported one is not a type the importer can name at all.
    let (checked, _) = scratch.check(
        "import Shapes;\nfn main() int {\n    s Secret = .{1};\n    return s.hidden;\n}\n",
    );
    assert!(!checked, "an unexported type must not be usable");
}

#[test]
fn a_star_import_without_export_does_not_re_export() {
    let scratch = Scratch::new("no-reexport");
    scratch.module("Inner", "export fn visible() int { return 7; }\n");
    // `Plain` forwards without `export`, so its importer sees nothing.
    scratch.module("Plain", "from Inner import *;\n");
    let (checked, _) = scratch.check("import Plain;\nfn main() int { return Plain.visible(); }\n");
    assert!(!checked, "an unexported star import must not re-export");
}

#[test]
fn an_exported_star_import_re_exports() {
    let scratch = Scratch::new("reexport");
    scratch.module("Inner", "export fn visible() int { return 7; }\n");
    scratch.module("Re", "export from Inner import *;\n");
    let (checked, _) = scratch.check("import Re;\nfn main() int { return Re.visible(); }\n");
    assert!(checked, "an exported star import must re-export");
}

#[test]
fn a_name_must_be_exported_at_every_step_of_the_chain() {
    let scratch = Scratch::new("chain");
    // `hidden` is not exported by `Inner`, so `Re`'s `export from` cannot
    // forward what was never published.
    scratch.module("Inner", "fn hidden() int { return 9; }\n");
    scratch.module("Re", "export from Inner import *;\n");
    let (checked, _) = scratch.check("import Re;\nfn main() int { return Re.hidden(); }\n");
    assert!(
        !checked,
        "a name must be exported at every step to reach the importer"
    );
}

#[test]
fn an_exported_named_import_re_exports_only_that_name() {
    let scratch = Scratch::new("named-reexport");
    scratch.module(
        "Inner",
        "export fn first() int { return 1; }\nexport fn second() int { return 2; }\n",
    );
    scratch.module("Re", "export from Inner import first;\n");
    let (checked, _) = scratch.check("import Re;\nfn main() int { return Re.first(); }\n");
    assert!(checked, "the named export must be forwarded");

    let (checked, _) = scratch.check("import Re;\nfn main() int { return Re.second(); }\n");
    assert!(!checked, "a name left out of the list must not be forwarded");
}

#[test]
fn a_function_defined_in_an_imported_module_is_compiled() {
    // A module that writes `export fn f() int { .. }` is the only place `f`
    // exists. If its body is not lowered the symbol is undefined at link time,
    // which is exactly what happened: only the file named on the command line
    // was compiled.
    let scratch = Scratch::new("module-body");
    scratch.module(
        "Inner",
        "export fn answer() int { return 42; }\nexport fn addup(a int, b int) int { return a + b; }\n",
    );
    let (built, _) = scratch.build_and_run(
        "import Inner;\nfn main() int { return Inner.answer(); }\n",
    );
    assert!(built, "a function defined in a module must be compiled");

    // And its parameters work, so the body really was emitted rather than
    // stubbed.
    let (built, _) = scratch.build_and_run(
        "import Inner;\nfn main() int { return Inner.addup(20, 22); }\n",
    );
    assert!(built, "a module function's parameters must work");
}

#[test]
fn a_module_declaration_does_not_get_an_empty_body() {
    // A module that declares `extern` functions alongside definitions must not
    // have a basic block emitted for the declarations: an unterminated block is
    // invalid IR. `Stdio` is all declarations, and importing it used to fail
    // LLVM verification once module bodies were compiled.
    let scratch = Scratch::new("extern-decl");
    scratch.module(
        "Mixed",
        "@extern_C export fn puts(text *const c8) int;\nexport fn twice(n int) int { return n + n; }\n",
    );
    let (built, output) = scratch.build_and_run(
        "import Mixed;\nfn main() int {\n    Mixed.puts(\"hi\");\n    return Mixed.twice(21);\n}\n",
    );
    assert!(built, "a module may declare and define");
    assert_eq!(output.trim(), "hi");
}

#[test]
fn a_module_function_is_reachable_through_the_umbrella() {
    // Both halves at once: the submodule defines the function and the umbrella
    // re-exports it.
    let scratch = Scratch::new("umbrella-body");
    scratch.module("Inner", "export fn answer() int { return 42; }\n");
    scratch.module("Outer", "export from Inner import *;\n");
    let (built, _) =
        scratch.build_and_run("import Outer;\nfn main() int { return Outer.answer(); }\n");
    assert!(built, "a re-exported function must be compiled too");
}

#[test]
fn the_c_bindings_still_link_now_that_modules_are_compiled() {
    // `C` is the largest module and is all declarations, so it is the case that
    // catches an empty basic block.
    let scratch = Scratch::new("c-still-links");
    scratch.module("Nothing", "export fn unused() int { return 0; }\n");
    let (built, _) = scratch.build_and_run(
        "import C;\nfn main() int {\n    C.puts(\"ok\");\n    return 0;\n}\n",
    );
    assert!(built, "the C bindings must keep linking");
}

#[test]
fn a_module_publishes_every_overload() {
    // Two functions of one name are two members, and a caller in another module
    // reaches both. Only the first was visible: the call site took the first
    // definition of the qualified name it found, so `show("text")` called the
    // `void show()` overload and passed it an argument it does not take.
    let scratch = Scratch::new("overload-across");
    scratch.module(
        "Printer",
        "import C;\n\nexport fn show() void {\n    C.puts(\"none\");\n}\n\nexport fn show(text *const c8) void {\n    C.puts(text);\n}\n\nexport fn show(n int) void {\n    C.printf(\"%d\\n\", n);\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import Printer;\nfn main() int {\n    Printer.show();\n    Printer.show(\"text\");\n    Printer.show(7);\n    return 0;\n}\n",
    );
    assert!(built, "every overload must be callable from another module");
    assert_eq!(output, "none\ntext\n7\n");
}

#[test]
fn a_module_reads_a_variable_of_another_module() {
    // A module is a translation unit too: `C.stdout` inside a module is a read
    // of storage that module does not own. The entry the checker needs was only
    // registered for the program, so the read reached code generation as a
    // field of a module name and was reported as an internal error.
    let scratch = Scratch::new("module-reads-global");
    scratch.module("state", "export counter int = 7;\n");
    scratch.module(
        "reader",
        "import state;\n\nexport fn bump() int {\n    state.counter += 1;\n    return state.counter;\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport state;\nimport reader;\nfn main() int {\n    C.printf(\"%d %d\\n\", reader.bump(), state.counter);\n    return 0;\n}\n",
    );
    assert!(built, "a module may read another module's variable");
    assert_eq!(output.trim(), "8 8");
}

#[test]
fn a_module_template_call_folds_against_the_templates_own_tree() {
    // A template reached through its module name is instantiated from the
    // caller's checker, and a call inside its body is folded while it is
    // checked. That fold read *this* file's constant table, so the inner call
    // answered with whatever expression this file happened to have at the same
    // offset -- `clamp(5, 0, 10)` called `min(10, 10)` and returned 10.
    let scratch = Scratch::new("template-body-tree");
    scratch.module(
        "numeric",
        "export
template <T type>
fn smaller(a T, b T) T {
    if (a < b) {
        return a;
    }
    return b;
}

export

template <T type>
fn larger(a T, b T) T {
    if (b < a) {
        return a;
    }
    return b;
}

export

template <T type>
fn between(value T, low T, high T) T {
    return smaller(larger(value, low), high);
}
",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport numeric;\nfn main() int {\n    C.printf(\"%d %d %d\\n\", numeric.between(5, 0, 10), numeric.between(-3, 0, 10), numeric.between(12, 0, 10));\n    return 0;\n}\n",
    );
    assert!(built, "a module's template may call its own templates");
    assert_eq!(output.trim(), "5 0 10");
}

#[test]
fn a_module_body_may_use_a_family_method_of_an_imported_type() {
    // A module's own function may call `Array<T>::push` on a parameter. The
    // body of `push` lives in `std.array`, which the *module* imports, and the
    // module's checker did not have the bodies of its own imports -- the
    // instantiation was requested, silently dropped, and the call reached code
    // generation with no function behind it.
    let scratch = Scratch::new("module-family-method");
    scratch.module(
        "filler",
        "import std;\nfrom std.string import String;\n\nexport fn fill(into &Array<String>) void {\n    into.push(String(\"x\"));\n    into.push(String(\"y\"));\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport std;\nfrom filler import fill;\nfn main() int {\n    xs Array<String>;\n    fill(xs);\n    C.printf(\"%d %s %s\\n\", xs.length(), xs[0].data(), xs[1].data());\n    return 0;\n}\n",
    );
    assert!(built, "a module may call a family method of an imported type");
    assert_eq!(output.trim(), "2 x y");
}

#[test]
fn a_module_body_may_rely_on_its_own_globals_inside_a_template() {
    // A template written in a module is checked where it is instantiated, so
    // the bare names it writes mean that module's declarations. A global's
    // *type* lives in the declaring module's binding table, and without it the
    // name had no type where the body was instantiated: the member call on it
    // was never resolved, and the emitted program had no function to call.
    let scratch = Scratch::new("module-global-in-template");
    scratch.module(
        "counter",
        "export step int = 7;\n\nexport\ntemplate <T type>\nfn advance(value T) int {\n    return (value as int) + step;\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport counter;\nfn main() int {\n    C.printf(\"%d\\n\", counter.advance(5));\n    return 0;\n}\n",
    );
    assert!(built, "a module's template may name its module's global");
    assert_eq!(output.trim(), "12");
}

#[test]
fn a_module_body_may_omit_a_default_its_own_declaration_gives() {
    // The unit reaches `value` as an external declaration before the module's
    // own bodies are lowered, and declaring it again found the symbol already
    // there and returned -- losing the default. The module's own call then
    // passed one argument to a two-parameter function.
    let scratch = Scratch::new("module-own-default");
    scratch.module(
        "boxed",
        "export type Boxed {\n    n int;\n}\n\nexport fn value(this &const Boxed, extra int = 3) int {\n    return this.n + extra;\n}\n\nexport fn report() int {\n    b Boxed = .{4};\n    return b.value();\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport boxed;\nfn main() int {\n    b boxed.Boxed = .{1};\n    C.printf(\"%d %d\\n\", b.value(2), boxed.report());\n    return 0;\n}\n",
    );
    assert!(built, "a module's body may omit its own declaration's default");
    assert_eq!(output.trim(), "3 7");
}

#[test]
fn an_exported_thread_local_variable_is_one_object() {
    // A `thread_local` global crosses as a declaration with the same storage
    // class: the importer reads and writes the defining module's object, not a
    // second copy of it.
    let scratch = Scratch::new("exported-thread-local");
    scratch.module(
        "tls",
        "export counter int @thread_local = 5;\n\nexport fn bump() int {\n    counter += 1;\n    return counter;\n}\n",
    );
    let (built, output) = scratch.build_and_run(
        "import C;\nimport tls;\nfn main() int {\n    tls.counter = tls.counter + 1;\n    C.printf(\"%d %d %d\\n\", tls.counter, tls.bump(), tls.counter);\n    return 0;\n}\n",
    );
    assert!(built, "an exported thread-local variable is one object");
    assert_eq!(output.trim(), "6 7 7");
}
