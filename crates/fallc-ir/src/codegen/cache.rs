//! Cache keys and locations for compiled modules.
//!
//! A module is compiled once and its bitcode reused by later compilations: a
//! programme that imports the standard library must not rebuild all of it every
//! time. The cache lives outside the source tree only in the sense that it is
//! build output; it is keyed by everything that can change the generated code,
//! so a stale entry can never be used silently.
//!
//! # Key
//!
//! The key is a hash of:
//!
//! * the module's source text, because that is what is compiled;
//! * the compiler's version, because a code-generation change must invalidate
//!   every entry even when no source changed;
//! * the target platform and the optimisation level, because both change what
//!   LLVM emits.
//!
//! # Location
//!
//! `FALLC_CACHE` names the directory when set; otherwise the cache is
//! `<workspace>/target/fallc-cache`, next to the other build output. A cache
//! that cannot be created is not fatal: the caller compiles the module as it
//! would without a cache.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::{BuildMode, OptLevel};

/// Everything that decides whether a cached module is still valid.
#[derive(Clone, Copy, Debug)]
pub struct CacheKey<'a> {
    /// A hash of everything the module's code generation depends on.
    ///
    /// The sources are no longer in hand when a module is lowered; the loader
    /// hashes the module and its dependency closure and carries the value.
    pub cache_hash: u64,
    /// The target the code is generated for, as the driver names it.
    pub target: &'a str,
    /// The optimisation level, as the driver names it.
    pub opt: &'a str,
}

impl CacheKey<'_> {
    /// Returns the entry name for this module.
    ///
    /// The two hashers are seeded differently so that a collision has to occur
    /// in two independent 64-bit hashes; the result is 32 hex digits and the
    /// `.bc` suffix the cache files use.
    pub fn entry_name(&self) -> String {
        use std::collections::hash_map::DefaultHasher;

        fn hash_with(seed: u64, key: &CacheKey<'_>) -> u64 {
            let mut hasher = DefaultHasher::new();
            seed.hash(&mut hasher);
            env!("CARGO_PKG_VERSION").hash(&mut hasher);
            key.cache_hash.hash(&mut hasher);
            key.target.hash(&mut hasher);
            key.opt.hash(&mut hasher);
            hasher.finish()
        }

        format!("{:016x}{:016x}.bc", hash_with(0, self), hash_with(1, self))
    }
}

/// Returns the bitcode path for a key, creating the cache directory if needed.
pub fn entry_path(key: &CacheKey<'_>) -> Option<PathBuf> {
    cache_directory().map(|directory| directory.join(key.entry_name()))
}

/// Returns the directory module bitcode is cached in.
///
/// The cache belongs to the *user*, not to whoever built the compiler, so it
/// lives where the platform keeps caches. In order:
///
/// 1. `FALLC_CACHE`, when set -- for a shared cache, or a read-only home;
/// 2. `$XDG_CACHE_HOME/fallthrough`, which is the same directory `$HOME/.cache`
///    normally names;
/// 3. `$HOME/.cache/fallthrough` when `XDG_CACHE_HOME` is unset;
/// 4. the system temporary directory, so a machine with no home still compiles.
///
/// An earlier version derived the directory from `CARGO_MANIFEST_DIR`, which is
/// baked in when the compiler is built. That pointed an installed compiler at
/// the source tree of whoever built it: the cache was written there, and a user
/// without that checkout silently got no cache at all.
pub fn cache_directory() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("FALLC_CACHE") {
        let path = PathBuf::from(configured);
        return std::fs::create_dir_all(&path).ok().map(|()| path);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir);
    let path = base.join("fallthrough");
    std::fs::create_dir_all(&path).ok().map(|()| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(cache_hash: u64, target: &str, opt: &str) -> CacheKey<'_> {
        CacheKey { cache_hash, target, opt }
    }

    #[test]
    fn every_input_that_changes_code_changes_the_key() {
        let base = key(1, "x86_64-linux", "0");
        let name = base.entry_name();
        assert_ne!(name, key(2, "x86_64-linux", "0").entry_name());
        assert_ne!(name, key(1, "aarch64-linux", "0").entry_name());
        assert_ne!(name, key(1, "x86_64-linux", "2").entry_name());
        assert_eq!(name, key(1, "x86_64-linux", "0").entry_name());
    }

    #[test]
    fn the_entry_path_is_the_cache_directory_plus_the_name() {
        let key = key(1, "x86_64-linux", "0");
        let Some(path) = entry_path(&key) else {
            return;
        };
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), key.entry_name());
        assert!(path.parent().is_some());
    }
}

/// Returns the key an imported-module library is cached under.
///
/// The modules are taken in load order: it decides the order their setups run
/// in and which definition of a duplicated symbol wins, so another order is
/// another library.
pub fn library_hash<'a>(modules: impl IntoIterator<Item = (&'a str, u64)>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, hash) in modules {
        name.hash(&mut hasher);
        hash.hash(&mut hasher);
    }
    hasher.finish()
}

/// Returns the bitcode and manifest paths of a module library, or `None` when
/// no cache directory is available.
///
/// The driver uses this to decide, before checking anything, whether the
/// modules it loaded can be reused; code generation uses the same paths when it
/// links the library.
#[must_use]
pub fn library_entries(
    directory: &Path,
    modules: &[(&str, u64)],
    mode: BuildMode,
    optimization: Option<OptLevel>,
) -> (PathBuf, PathBuf) {
    let target = format!(
        "{:?}",
        inkwell::targets::TargetMachine::get_default_triple()
    );
    // The optimisation level belongs to the key when machine code is what is
    // cached: two levels produce different code for the same modules, and a
    // separate entry is what keeps one from answering for the other. With the
    // level in the key, the artifacts keep their own names.
    let opt = match optimization {
        Some(level) => format!("{mode:?}/{}", level.spelling()),
        None => format!("{mode:?}"),
    };
    let key = CacheKey {
        cache_hash: library_hash(modules.iter().copied()),
        target: &target,
        opt: &opt,
    };
    let entry = directory.join(key.entry_name());
    // `.o` is the compiled functions and `.tl` the templates that go with
    // them; the bitcode path caches the module itself, which is why it has no
    // `.o`.
    match optimization {
        Some(_) => (entry.with_extension("o"), entry.with_extension("tl")),
        None => (entry.clone(), entry.with_extension("tl")),
    }
}
