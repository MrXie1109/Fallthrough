//! The module graph: which modules a compilation unit needs, and what each one
//! provides.
//!
//! # Naming
//!
//! A module's name is its file's stem: `C.ftm` is module `C`. There is no
//! declaration inside the file that names it, and no way for a file to claim a
//! name other than its own.
//!
//! # Visibility
//!
//! A module's declarations are **private by default**. `export` on a
//! declaration makes it visible to importing modules, and nothing else is
//! reachable. That is why a module's public surface is exactly its `export`
//! items, and why an unexported name is reported as "not exported" rather than
//! "not found" — the distinction matters when diagnosing.
//!
//! # Loading
//!
//! `import C;` names a module, not a path. The loader looks for `C.ftm` beside
//! the importing file first, then in each directory of the compiler's search
//! path. A dotted name is a path in the module tree — `std.Panic` is
//! `std/Panic.ftm` — and a module is one file, so it cannot be split across
//! files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fallc_ast::Ast;
use fallc_diag::{Diagnostic, DiagnosticBag, DiagnosticCode};
use fallc_span::FileId;

/// Identifies a loaded module.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ModuleId(u32);

impl ModuleId {
    /// The module being compiled, which is always first.
    pub const ROOT: ModuleId = ModuleId(0);

    /// Creates a module id from its raw index.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        ModuleId(raw)
    }

    /// Returns the raw index.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "module#{}", self.0)
    }
}

/// What a module provides to its importers.
#[derive(Clone, Debug)]
pub struct Module {
    /// The module's name, which is its file's stem.
    pub name: String,
    /// The file it was loaded from.
    pub path: PathBuf,
    /// The file's id in the session's source map.
    pub file: FileId,
    /// The parsed contents.
    pub ast: Ast,
    /// The names this module exports, sorted.
    ///
    /// Sorted so that membership can be tested by binary search and so that
    /// diagnostics list them in a stable order.
    pub exports: Vec<String>,
}

impl Module {
    /// Returns `true` if this module exports `name`.
    #[must_use]
    pub fn exports_name(&self, name: &str) -> bool {
        self.exports
            .binary_search_by(|export| export.as_str().cmp(name))
            .is_ok()
    }
}

/// The default file extension for a Fallthrough module.
pub const MODULE_EXTENSION: &str = "ftm";

/// Where the modules shipped with the compiler live, as a directory name.
///
/// The driver looks for `standard/` beside the compiler, above the compiled
/// file, and above the working directory; `FALLTHROUGH_STANDARD` names the
/// directory outright when it is set.
pub const STANDARD_MODULE_DIRECTORY: &str = "standard";

/// Every module participating in one compilation.
#[derive(Debug, Default)]
pub struct ModuleGraph {
    modules: Vec<Module>,
    /// Module ids by name, so a repeated `import` reuses the first load.
    by_name: HashMap<String, ModuleId>,
    diagnostics: DiagnosticBag,
}

impl ModuleGraph {
    /// Creates a graph with no modules.
    #[must_use]
    pub fn new() -> Self {
        ModuleGraph::default()
    }

    /// Registers the module being compiled and returns its id.
    ///
    /// The root is always [`ModuleId::ROOT`] and is always first, so it can be
    /// referred to without a lookup.
    pub fn add_root(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        file: FileId,
        ast: Ast,
    ) -> ModuleId {
        let name = name.into();
        let id =
            ModuleId(u32::try_from(self.modules.len()).expect("module count exceeds u32::MAX"));
        self.modules.push(Module {
            name: name.clone(),
            path: path.into(),
            file,
            exports: Vec::new(),
            ast,
        });
        self.by_name.insert(name, id);
        id
    }

    /// Adds a loaded module, or returns the existing one with that name.
    pub fn add_module(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        file: FileId,
        ast: Ast,
    ) -> ModuleId {
        let name = name.into();
        if let Some(&existing) = self.by_name.get(&name) {
            return existing;
        }
        let id =
            ModuleId(u32::try_from(self.modules.len()).expect("module count exceeds u32::MAX"));
        self.modules.push(Module {
            name: name.clone(),
            path: path.into(),
            file,
            exports: Vec::new(),
            ast,
        });
        self.by_name.insert(name, id);
        id
    }

    /// Returns the module with `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this graph.
    #[must_use]
    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.as_raw() as usize]
    }

    /// Returns a mutable reference to the module with `id`.
    pub fn module_mut(&mut self, id: ModuleId) -> &mut Module {
        &mut self.modules[id.as_raw() as usize]
    }

    /// Looks a module up by name.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<ModuleId> {
        self.by_name.get(name).copied()
    }

    /// Returns every module, the root first.
    #[must_use]
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// Returns the number of modules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Returns `true` if only the root is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Records that `module` exports `name`.
    ///
    /// The name is inserted in sorted position, because
    /// [`Module::exports_name`] binary-searches.
    pub fn add_export(&mut self, module: ModuleId, name: impl Into<String>) {
        let name = name.into();
        let exports = &mut self.modules[module.as_raw() as usize].exports;
        if let Err(position) = exports.binary_search(&name) {
            exports.insert(position, name);
        }
    }

    /// Records a diagnostic raised while loading.
    pub fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Takes the diagnostics raised while loading.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics).into_vec()
    }

    /// Returns the diagnostics raised while loading.
    #[must_use]
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }
}

/// Finds the file for a module named `name`.
///
/// The importer's own directory is searched first, so a module beside the file
/// that uses it needs no configuration. The extra directories are the driver's
/// search path, tried in order.
///
/// Returns `None` when no candidate exists, which the caller reports as an
/// unresolved import.
#[must_use]
pub fn find_module_file(name: &str, importer: &Path, search_path: &[PathBuf]) -> Option<PathBuf> {
    // A dotted name is a path within the module tree: `std.Panic` is
    // `std/Panic.ftm`. That is what lets a standard library be organised into
    // directories while a program still names one thing.
    let relative = name.replace('.', "/");
    // A module that has an implementation per platform states it in the file
    // name: `threads.linux.ftm` is what `import threads;` loads on Linux, and
    // `threads.windows.ftm` on Windows. The *module name* stays `threads`, so
    // importers, re-exports and the name's identity do not change; only the
    // file the loader reads does. A module without a platform file is portable
    // and is looked up as before.
    let platform = target_platform();
    let candidates = [
        format!("{relative}.{platform}.{MODULE_EXTENSION}"),
        format!("{relative}.{MODULE_EXTENSION}"),
    ];

    // Beside the importing file.
    if let Some(directory) = importer.parent() {
        for filename in &candidates {
            let candidate = directory.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // Then the search path, in order.
    for directory in search_path {
        for filename in &candidates {
            let candidate = directory.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The operating system this build targets.
///
/// The compiler targets the machine it runs on, so this is normally the host's
/// name. `FALLC_TARGET_OS` overrides it, which is what lets the platform files of
/// another system be parsed and type-checked on this one: the loader picks
/// `sync_sys.windows.ftm` for a program that is only being checked, and nothing
/// has to link.
#[must_use]
pub fn target_platform() -> String {
    std::env::var("FALLC_TARGET_OS")
        .unwrap_or_else(|_| std::env::consts::OS.to_owned())
}

/// Derives a module name from a file path, by dropping the extension.
#[must_use]
pub fn module_name_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("main")
        .to_owned()
}

/// Reports an import that could not be resolved.
#[must_use]
pub fn unresolved_import(name: &str, span: fallc_span::Span) -> Diagnostic {
    Diagnostic::error(format!("cannot find module `{name}`"))
        .with_code(DiagnosticCode::driver(5))
        .with_span(span)
        .with_note(format!("looked for `{name}.{MODULE_EXTENSION}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_span::FileId;

    fn graph_with_root() -> ModuleGraph {
        let mut graph = ModuleGraph::new();
        graph.add_root("main", "main.ftm", FileId::from_raw(0), Ast::new());
        graph
    }

    #[test]
    fn the_root_is_always_first() {
        let graph = graph_with_root();
        assert_eq!(ModuleId::ROOT.as_raw(), 0);
        assert_eq!(graph.module(ModuleId::ROOT).name, "main");
    }

    #[test]
    fn adding_a_module_twice_reuses_it() {
        // A module imported by two files must be loaded once, or its types
        // would be distinct and nothing would type check across the boundary.
        let mut graph = graph_with_root();
        let first = graph.add_module("C", "C.ftm", FileId::from_raw(1), Ast::new());
        let second = graph.add_module("C", "C.ftm", FileId::from_raw(1), Ast::new());
        assert_eq!(first, second);
        assert_eq!(graph.len(), 2);
    }

    #[test]
    fn lookups_by_name_work() {
        let mut graph = graph_with_root();
        let c = graph.add_module("C", "C.ftm", FileId::from_raw(1), Ast::new());
        assert_eq!(graph.by_name("C"), Some(c));
        assert_eq!(graph.by_name("Nope"), None);
    }

    #[test]
    fn exports_stay_sorted_and_are_findable() {
        let mut graph = graph_with_root();
        let c = graph.add_module("C", "C.ftm", FileId::from_raw(1), Ast::new());
        // Deliberately out of order.
        for name in ["printf", "abort", "malloc", "puts"] {
            graph.add_export(c, name);
        }
        assert_eq!(
            graph.module(c).exports,
            vec!["abort", "malloc", "printf", "puts"]
        );
        assert!(graph.module(c).exports_name("printf"));
        assert!(!graph.module(c).exports_name("fprintf"));
    }

    #[test]
    fn adding_the_same_export_twice_does_not_duplicate_it() {
        let mut graph = graph_with_root();
        let c = graph.add_module("C", "C.ftm", FileId::from_raw(1), Ast::new());
        graph.add_export(c, "printf");
        graph.add_export(c, "printf");
        assert_eq!(graph.module(c).exports, vec!["printf"]);
    }

    #[test]
    fn a_module_name_is_the_file_stem() {
        assert_eq!(module_name_of(Path::new("dir/C.ftm")), "C");
        assert_eq!(module_name_of(Path::new("main.ftm")), "main");
    }
}
