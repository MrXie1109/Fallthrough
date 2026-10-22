//! Producing an executable: object files, the linker, and the runtime stub.
//!
//! The runtime is a small C file that a program using exceptions links against;
//! it is built once per run and cached, and the linker is then asked for an
//! executable with the objects and the libraries a program declared.

use super::*;

/// Derives an output path from the input, replacing or adding an extension.
///
/// `suffix` of `""` drops the extension entirely, which is what an executable
/// wants.
pub(super) fn default_output_path(input: &Path, suffix: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("a.out");
    let mut name = String::from(stem);
    if suffix.is_empty() {
        // An executable with no suffix. Windows wants `.exe`, and a program
        // built there is run by its name either way; the suffix is added only
        // to the *default* path, so an explicit `-o` is taken as written.
        if crate::modules::target_platform() == "windows" {
            name.push_str(".exe");
        }
    } else {
        name.push('.');
        name.push_str(suffix);
    }
    input.with_file_name(name)
}

/// The C compiler this build uses as a linker driver.
///
/// It is the linker because it knows where the platform's startup files and
/// system libraries are; `ld` alone would need them spelled out. The variable is
/// `CC`, which is what a build system already sets, and the default is the
/// platform's own: `cc` on POSIX and `clang` on Windows, where `cc` is not
/// usually a name. A Windows build needs an LLVM or MinGW toolchain on `PATH`
/// (the link line is GNU-flavoured: `-l`, `-L`, `-o`).
pub(super) fn linker_driver() -> String {
    if let Ok(value) = std::env::var("CC") {
        return value;
    }
    if let Ok(value) = std::env::var("FALLC_LINKER") {
        return value;
    }
    if crate::modules::target_platform() == "windows" {
        "clang".to_owned()
    } else {
        "cc".to_owned()
    }
}

/// Returns a path for an intermediate object file, in the same directory as
/// the input so that linking needs no cross-filesystem move.
pub(super) fn temporary_object_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("out");
    input.with_file_name(format!("{stem}.fallc-tmp.o"))
}

/// Links an object file into an executable.
///
/// # The runtime stub
///
/// Debug-mode guards call `__builtin_panic`, which stands in for `std.panic`
/// and has no definition in the module. The stub is compiled from
/// `runtime/builtins.c` on demand, so a program that never traps still links
/// without it.
///
/// `std.panic` is not implemented yet: this stub is the interim abort, and when
/// the module defines one, the compiler links the standard library instead and
/// the stub goes away.
pub(super) fn link_executable(objects: &[PathBuf], output: &Path, links: &[LinkRequest]) -> Result<(), String> {
    link_objects(objects, output, links, false)
}

/// Links object files into a shared library.
///
/// The objects must have been built position independently; the module library
/// is, and the unit's object is when a shared output was asked for (see the
/// driver).
pub(super) fn link_shared(
    objects: &[PathBuf],
    output: &Path,
    links: &[LinkRequest],
) -> Result<(), String> {
    link_objects(objects, output, links, true)
}

/// Archives object files into a static library.
///
/// A static library is not a link: it is the objects in one file, and the linker
/// picks the members it needs. `ar rcs` is the platform's own archiver, the same
/// one a C build would use, so the result can be passed to `cc` like any other
/// `.a`.
pub(super) fn create_static_library(objects: &[PathBuf], output: &Path) -> Result<(), String> {
    let archiver = if let Ok(value) = std::env::var("AR") {
        value
    } else if crate::modules::target_platform() == "windows" {
        "llvm-ar".to_owned()
    } else {
        "ar".to_owned()
    };
    // The runtime stub goes in with the objects: the archive has to be usable
    // by a plain `cc` link, and debug guards in the unit's object call the
    // builtins the stub defines. A consumer that links two of these archives
    // pulls the member from the first and the second stays unused.
    let stub = build_runtime_stub()?;
    let mut command = std::process::Command::new(&archiver);
    command.arg("rcs").arg(output);
    for object in objects {
        command.arg(object);
    }
    if let Some(stub) = &stub {
        command.arg(stub);
    }
    let result = command
        .output()
        .map_err(|error| format!("cannot run `{archiver}` to archive: {error}"))?;
    if let Some(stub) = &stub {
        let _ = fs::remove_file(stub);
    }
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "archiving failed:\n{}",
            String::from_utf8_lossy(&result.stderr).trim()
        ))
    }
}

fn link_objects(
    objects: &[PathBuf],
    output: &Path,
    links: &[LinkRequest],
    shared: bool,
) -> Result<(), String> {
    // A C compiler is used as the linker driver because it knows where the
    // system libraries are. Using `ld` directly would need the platform's
    // startup files spelled out.
    let linker = linker_driver();

    let stub = build_runtime_stub()?;

    let mut command = std::process::Command::new(&linker);
    if shared {
        command.arg("-shared");
    }
    // Every object of the program, in the order the files were given, so that
    // the link line reads the way the command line did.
    for object in objects {
        command.arg(object);
    }
    if let Some(stub) = &stub {
        command.arg(stub);
    }
    command.arg("-o").arg(output);

    command.args(linker_arguments(links));

    let result = command
        .output()
        .map_err(|error| format!("cannot run `{linker}` to link: {error}"))?;

    // The stub is only needed for this link, so it is removed either way.
    if let Some(stub) = &stub {
        let _ = fs::remove_file(stub);
    }

    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "linking failed:\n{}",
            String::from_utf8_lossy(&result.stderr).trim()
        ))
    }
}

/// Translates the `link` instructions into linker arguments.
///
/// A name is passed as `-l`, which is how a linker is normally told about a
/// library: `-lm` finds `libm.so` on the search path. A name that already looks
/// like a file — one containing a `/`, or ending in `.a` or `.so` — is passed
/// through as itself instead, so a library the program has a full path for can
/// still be named here.
///
/// The kind decides how the library is reached. A shared library is what `-l`
/// finds by default, so a dynamic link needs nothing more. An archive is asked
/// for by file name with `-l:`, which is a GNU extension but is what makes
/// `static` mean anything: without it, `-l` would happily pick the shared
/// object beside the archive and the instruction would be a no-op.
pub(super) fn linker_arguments(links: &[LinkRequest]) -> Vec<String> {
    let mut arguments = Vec::new();
    for link in links {
        if let Some(directory) = &link.directory {
            arguments.push(format!("-L{directory}"));
        }
        // The spelling of the name does not matter here: `m` and `"m"` are the
        // same library, so both are expanded the same way.
        match link.kind {
            fallc_ast::LinkKind::Dynamic => {
                arguments.push(format!("-l{}", link.qualifier()));
            }
            // `-l:libNAME.a` names the archive exactly, which is what excludes
            // the shared object of the same name.
            fallc_ast::LinkKind::Static => {
                arguments.push(format!("-l:lib{}.a", link.qualifier()));
            }
        }
    }
    arguments
}

/// Collects the `link` instructions of a file and its imports.
///
/// An imported module's link instructions apply to anything that imports it:
/// `standard/C.ftm` says which libraries the C bindings need, and every program
/// that uses them gets them without repeating the instruction.
pub(super) fn collect_links(ast: &Ast, imported: &[&Ast]) -> Vec<LinkRequest> {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tree in std::iter::once(ast).chain(imported.iter().copied()) {
        for &item in &tree.items {
            let fallc_ast::ItemKind::Link(link) = &tree.item(item).kind else {
                continue;
            };
            let directory = match &link.location {
                fallc_ast::LinkLocation::Default => None,
                fallc_ast::LinkLocation::Directory(path) => Some(path.clone()),
            };
            // The same library named twice is linked once: a module imported by
            // two others would otherwise repeat the instruction.
            if !seen.insert((link.library.clone(), directory.clone())) {
                continue;
            }
            links.push(LinkRequest {
                library: link.library.clone(),
                kind: link.kind,
                directory,
            });
        }
    }
    links
}

/// Finds `runtime/builtins.c`, or `None` if this build has no stub.
///
/// # Why it is not a bare relative path
///
/// `runtime/builtins.c` used to be opened as a relative path, which resolves
/// against the compiler process's working directory. That works when the
/// compiler is run from the repository root and nowhere else: run from a test,
/// or from an installed location, the stub is silently not found and every
/// program that can trap fails to link with an undefined `__builtin_panic`.
///
/// The search therefore starts from the executable's own directory and walks
/// up, which finds the file both in a cargo build tree and in an unpacked
/// distribution. The working directory is tried last, for the case of a
/// compiler run from a checkout it does not live in.
pub(super) fn find_runtime_stub() -> Option<PathBuf> {
    let relative = Path::new("runtime").join("builtins.c");
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        // `target/debug/fallc` is three levels from the root and a test binary
        // at `target/debug/deps/fallc-..` is four, so the walk is generous
        // rather than exact: it stops at the first directory holding the file.
        let mut directory = executable.parent().map(Path::to_path_buf);
        for _ in 0..6 {
            let Some(current) = directory else {
                break;
            };
            roots.push(current.clone());
            directory = current.parent().map(Path::to_path_buf);
        }
    }
    if let Ok(working) = std::env::current_dir() {
        roots.push(working);
    }
    roots
        .into_iter()
        .map(|root| root.join(&relative))
        .find(|candidate| candidate.is_file())
}

/// Returns a counter value unique within this process.
///
/// Used to name a temporary file so that two compilations running at the same
/// time do not collide. A process id alone is not enough, because a test binary
/// compiles many programs in parallel within one process.
pub(super) fn next_stub_serial() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// Compiles the runtime stub if the source is present.
///
/// Returns `None` when there is no stub to build, which is the case for a
/// released compiler that links a real standard library instead.
pub(super) fn build_runtime_stub() -> Result<Option<PathBuf>, String> {
    let Some(source) = find_runtime_stub() else {
        return Ok(None);
    };
    // The name is unique per call, not per process. Two compilations in one
    // process -- which is what a test binary running its tests in parallel
    // does -- would otherwise share this path, and whichever finished first
    // would delete the file the other was still about to link.
    let object = std::env::temp_dir().join(format!(
        "fallc-builtins-{}-{}.o",
        std::process::id(),
        next_stub_serial()
    ));
    let compiler = linker_driver();

    let mut command = std::process::Command::new(&compiler);
    command.arg("-c").arg(&source).arg("-o").arg(&object);
    // Position-independent code is a POSIX shared-object requirement; a
    // Windows compiler warns about the flag and ignores it, so it is only asked
    // for where it means something.
    if crate::modules::target_platform() != "windows" {
        command.arg("-fPIC");
    }
    let result = command
        .output()
        .map_err(|error| format!("cannot compile the runtime stub: {error}"))?;

    if result.status.success() {
        Ok(Some(object))
    } else {
        Err(format!(
            "cannot compile `{}`:\n{}",
            source.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        ))
    }
}

/// Derives a module name from an input path.
///
/// A module's name is the stem of the file that declares it, so `standard/C.ftm`
/// is module `C`. A path with no usable stem falls back to `main`.
pub(super) fn module_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("main")
        .to_owned()
}

