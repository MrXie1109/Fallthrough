//! The compiled-module file: a module's export table, its interface, and the
//! object it was compiled into, as one file.
//!
//! # What a `.pcm` is for
//!
//! Importing a module means parsing it, resolving its names, and checking its
//! declarations -- three passes that produce one product: the export table. A
//! `.pcm` is that product written down, so an importer reads the answer instead
//! of recomputing it.
//!
//! # The file
//!
//! ```text
//! "FTMPCM\0"         8 bytes   magic
//! u32                          format version
//! u64                          compiler stamp      (the compiler that wrote it)
//! u64                          target stamp        (pointer width, object model)
//! u64                          source hash         (the module's own source)
//! repeated (name, hash)        the modules it was compiled against
//! section TABLE                len + bytes   the export table (fallc_sema::pcm)
//! section INTERFACE            len + bytes   the `.it` text, for readers of the cache
//! section OBJECT               len + bytes   the `.o` path this table was built with
//! ```
//!
//! Each section is a `u32` length and its bytes, so a reader can skip one it
//! does not want and a writer can add one without moving the others.
//!
//! # Why the dependency hashes are in the table, not in a hash of them
//!
//! The cache key for the *object* is a closure over the whole dependency graph:
//! an object contains inlined code from everything below it, so a change at the
//! bottom invalidates the top. The export table is not like that. What a module
//! exports is a property of its own source; a dependency that changed under it
//! matters only if that module's *own* text changed too, and then its own hash
//! says so. Keeping the dependencies as a list rather than folding them into a
//! number is what lets a one-line change at the bottom of the standard library
//! keep every table above it.

use std::path::{Path, PathBuf};

/// The bytes every `.pcm` starts with.
const MAGIC: &[u8; 8] = b"FTMPCM\0\0";

/// The format's version.
///
/// Bumped whenever the encoding changes shape. A file from another version is
/// ignored rather than read, because a table read under the wrong layout is
/// worse than no table: the importer would believe an answer no module gave.
const VERSION: u32 = 1;

/// A module's compiled table, as it lies on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CompiledModule {
    /// Every module this one was compiled against, and the source hash of each.
    pub dependencies: Vec<(String, u64)>,
    /// The module's own source hash.
    pub source_hash: u64,
    /// The key its object is cached under: a hash over its whole dependency
    /// closure.
    ///
    /// Recorded because a reader that hits this file still has to name the
    /// library the program links, and that key is otherwise computed after
    /// every module is loaded -- which a hit is exactly the work it skips.
    pub library_hash: u64,
    /// The export table, encoded by [`fallc_sema::pcm::encode`].
    pub table: Vec<u8>,
    /// The module's interface, as source text.
    ///
    /// Kept for the reader of the cache -- `fallc --emit interface`, a person
    /// looking at what a module publishes -- and not for the compiler, which
    /// reads `table`.
    pub interface: Vec<u8>,
    /// The object this table was compiled alongside, if one was.
    ///
    /// The object is the implementation, and the table is the declarations; a
    /// reader that wants both finds one from the other.
    pub object: Option<PathBuf>,
}

/// Returns the file a module's table is cached in.
///
/// The name is the module's own, with its dots turned into directories, so a
/// reader can tell which file a table came from and a stale directory is
/// removed as a whole. The version is in the name rather than only in the
/// header: a cache written by another format version is a different file, not a
/// file to be read and rejected on every build.
pub(super) fn path_in(directory: &Path, module: &str) -> PathBuf {
    directory.join(format!("{}.pcm", module.replace('.', "/")))
}

/// Renders a file's bytes.
///
/// A function of its argument alone: the same table written twice gives the
/// same bytes, which is what makes a file comparable and its content usable as
/// a cache key.
pub(super) fn write(module: &CompiledModule) -> Vec<u8> {
    use fallc_sema::pcm::Writer;
    let mut out = Writer::new();
    for byte in MAGIC {
        out.u8(*byte);
    }
    out.u32(VERSION);
    out.u64(compiler_stamp());
    out.u64(target_stamp());
    out.u64(module.source_hash);
    out.u64(module.library_hash);
    // Sorted, because a set of dependencies has no order: reordering an import
    // list must not rewrite the file.
    let mut dependencies = module.dependencies.clone();
    dependencies.sort();
    out.list(&dependencies, |out, (name, hash)| {
        out.str(name);
        out.u64(*hash);
    });
    out.count(module.table.len());
    for byte in &module.table {
        out.u8(*byte);
    }
    out.count(module.interface.len());
    for byte in &module.interface {
        out.u8(*byte);
    }
    match &module.object {
        Some(path) => {
            out.u8(1);
            out.str(&path.to_string_lossy());
        }
        None => out.u8(0),
    }
    out.finish()
}

/// Reads a file's bytes back.
///
/// `None` for anything this compiler did not write, which the caller treats as
/// a cache miss: the module is compiled from its source, and that is the answer
/// that cannot be wrong.
pub(super) fn read(bytes: &[u8]) -> Option<CompiledModule> {
    use fallc_sema::pcm::Reader;
    let mut input = Reader::new(bytes);
    for expected in MAGIC {
        if input.u8().ok()? != *expected {
            return None;
        }
    }
    if input.u32().ok()? != VERSION {
        return None;
    }
    if input.u64().ok()? != compiler_stamp() {
        return None;
    }
    if input.u64().ok()? != target_stamp() {
        return None;
    }
    let source_hash = input.u64().ok()?;
    let library_hash = input.u64().ok()?;
    let dependencies = input
        .list(|input| {
            let name = input.str()?;
            Ok((name, input.u64()?))
        })
        .ok()?;
    let table_len = input.count().ok()?;
    let table = input.take_bytes(table_len).ok()?.to_vec();
    let interface_len = input.count().ok()?;
    let interface = input.take_bytes(interface_len).ok()?.to_vec();
    let object = match input.u8().ok()? {
        0 => None,
        1 => Some(PathBuf::from(input.str().ok()?)),
        _ => return None,
    };
    if !input.at_end() {
        return None;
    }
    Some(CompiledModule {
        dependencies,
        source_hash,
        library_hash,
        table,
        interface,
        object,
    })
}

/// A number that changes when the compiler changes.
///
/// The table is a structure this compiler's `fallc_sema` defines, and a
/// compiler that changed it would read an old file under a new layout. The
/// version above covers deliberate changes; this covers the build itself, so
/// that a compiler rebuilt from a different commit never trusts a file the
/// previous one wrote.
fn compiler_stamp() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // The build's own identity, when one was stamped in. `option_env!` is
    // compiled into this binary, so a rebuild from another source is another
    // number without reading anything at run time.
    option_env!("FALLC_BUILD_STAMP")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .hash(&mut hasher);
    VERSION.hash(&mut hasher);
    hasher.finish()
}

/// A number that changes when the target changes.
///
/// A table names types whose layout depends on the pointer width: `usize` is
/// four bytes on one target and eight on another. A cache shared between them
/// would hand a program the wrong sizes.
fn target_stamp() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fallc_ir::Backend::pointer_bits().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod probes_compilable {
    use super::*;

    fn sample() -> CompiledModule {
        CompiledModule {
            dependencies: vec![("std.array".to_owned(), 7), ("std".to_owned(), 9)],
            source_hash: 42,
            library_hash: 43,
            table: vec![1, 2, 3, 4],
            interface: b"export fn get() int;\n".to_vec(),
            object: Some(PathBuf::from("/tmp/thing.o")),
        }
    }

    #[test]
    fn a_file_survives_a_round_trip() {
        let original = sample();
        let bytes = write(&original);
        assert_eq!(read(&bytes).as_ref(), Some(&original));
    }

    #[test]
    fn writing_twice_gives_the_same_bytes() {
        let module = sample();
        assert_eq!(write(&module), write(&module));
    }

    #[test]
    fn dependencies_are_written_in_order() {
        let mut forwards = sample();
        forwards.dependencies = vec![("a".to_owned(), 1), ("b".to_owned(), 2)];
        let mut backwards = forwards.clone();
        backwards.dependencies.reverse();
        assert_eq!(write(&forwards), write(&backwards));
    }

    #[test]
    fn a_foreign_file_is_refused() {
        assert!(read(b"not a module file").is_none());
        let bytes = write(&sample());
        assert!(read(&bytes[..bytes.len() / 2]).is_none());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(read(&longer).is_none());
    }

    #[test]
    fn an_object_is_optional() {
        let mut module = sample();
        module.object = None;
        let bytes = write(&module);
        assert_eq!(read(&bytes).map(|file| file.object), Some(None));
    }
}
