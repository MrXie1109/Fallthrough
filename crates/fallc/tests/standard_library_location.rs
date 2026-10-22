//! Where the standard library is found, and where it is not.
//!
//! Which directory the compiler is *run from* must not decide whether a program
//! can `import std`: the same command has to work from `/tmp`, from a crate
//! directory, and from the workspace root. The search therefore starts at the
//! compiler's own location and at the file being compiled, and only then looks
//! above the current directory.

use std::path::PathBuf;

/// A temporary directory holding one program.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-stdlib-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn the_standard_library_is_found_from_any_working_directory() {
    // The program lives outside the repository, and the compiler is run with a
    // working directory that is not the repository either. The binary's own
    // location is what has to find `standard/`.
    let scratch = Scratch::new("elsewhere");
    let program = scratch.directory.join("prog.ft");
    std::fs::write(
        &program,
        "import std;\nimport C;\n\nfn main() int {\n    C.printf(\"ok\\n\");\n    return 0;\n}\n",
    )
    .expect("the source can be written");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
        .arg(&program)
        .arg("--check")
        .current_dir(&scratch.directory)
        .output()
        .expect("the compiler can be run");
    let printed = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "the library was not found from another directory:\n{printed}"
    );
}
