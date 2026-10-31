//! Cache keys and locations for compiled modules.
//!
//! The implementation lives in `fallc-ir`, because code generation is what
//! looks entries up; the driver re-exports it so the two stages cannot disagree
//! about how a key is built.

pub(super) use fallc_ir::codegen::cache::cache_directory;

/// Returns the cache directory for this build of the compiler.
///
/// The entry key covers the module's sources, the target, and the optimisation
/// level -- but not this compiler's own code generation, which changes with
/// every rebuild during development. Objects produced by an older compiler must
/// not be silently reused, so each build gets a directory of its own, named by
/// the compiler binary's size and modification time. Stale directories are
/// harmless build output; removing the cache root clears them all.
pub(super) fn build_directory() -> Option<std::path::PathBuf> {
    let base = cache_directory()?;
    let directory = base.join(build_identity());
    std::fs::create_dir_all(&directory).ok().map(|()| directory)
}

/// Names the compiler build the cache belongs to.
fn build_identity() -> String {
    use std::hash::{Hash, Hasher};

    let binary = std::env::current_exe().ok();
    let metadata = binary
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // The path distinguishes two compilers of the same size built at the same
    // second; the size and time distinguish two builds at the same path.
    binary.hash(&mut hasher);
    if let Some(metadata) = &metadata {
        metadata.len().hash(&mut hasher);
        metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|time| time.as_nanos())
            .hash(&mut hasher);
    }
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
