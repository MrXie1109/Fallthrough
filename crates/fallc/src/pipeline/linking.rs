//! Producing an executable: object files and the linker.
//!
//! The builtins a module calls -- the debug-mode panic and the value renderers
//! -- are defined inside the module's own IR, so linking needs nothing beyond
//! the objects and the libraries a program declared.

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
/// Debug-mode guards call `__builtin_panic`, and `str(x)` calls the renderers;
/// both are defined by code generation itself (see
/// `fallc_ir::codegen::runtime`), so the link line is the objects and whatever
/// the program asked for.
pub(super) fn link_executable(
    objects: &[PathBuf],
    output: &Path,
    links: &[LinkRequest],
) -> Result<(), String> {
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
    // Every builtin the code calls is defined inside the objects themselves
    // (see `fallc_ir::codegen::runtime`), so the archive needs nothing added
    // for a plain `cc` link to succeed.
    let mut command = std::process::Command::new(&archiver);
    command.arg("rcs").arg(output);
    for object in objects {
        command.arg(object);
    }
    let result = command
        .output()
        .map_err(|error| format!("cannot run `{archiver}` to archive: {error}"))?;
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

    let mut command = std::process::Command::new(&linker);
    if shared {
        command.arg("-shared");
    }
    // Every object of the program, in the order the files were given, so that
    // the link line reads the way the command line did.
    for object in objects {
        command.arg(object);
    }
    command.arg("-o").arg(output);

    command.args(linker_arguments(links));

    let result = command
        .output()
        .map_err(|error| format!("cannot run `{linker}` to link: {error}"))?;

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
