//! The library has one interface and an implementation per platform.
//!
//! On this machine only the host's platform files can be *run*, but the other
//! platform's files can be parsed and type-checked: the loader's platform name
//! is `FALLC_TARGET_OS` when it is set, and a check needs nothing to link. A
//! syntax error or a type error in `*.windows.ftm` is exactly what this catches
//! before anyone builds on Windows.

use std::path::PathBuf;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-cross-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    /// Checks `source` for `target`, returning whether it was accepted and what
    /// the compiler printed.
    fn check(&self, target: &str, source: &str) -> (bool, String) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_fallc"))
            .arg("--check")
            .arg(&main)
            .env("FALLC_TARGET_OS", target)
            .output()
            .expect("the compiler can be run");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

const PROGRAM: &str = r#"import std;
import std.os;
import meta;

fn main() int except {
    std.out.println("platform is ", $meta.PLATFORM)??;
    std.out.println("pid is ", os.pid())??;
    return 0;
}
"#;

#[test]
fn the_library_checks_for_windows_on_any_host() {
    // Loading `std` pulls in every platform-selected module: threads, paths,
    // the entropy source. Checking for Windows therefore reads
    // `sync_sys.windows.ftm`, `os_sys.windows.ftm` and
    // `random_entropy.windows.ftm`.
    let scratch = Scratch::new("windows");
    let (accepted, output) = scratch.check("windows", PROGRAM);
    assert!(
        accepted,
        "the library must type-check for Windows:\n{output}"
    );
}

#[test]
fn the_library_checks_for_the_host() {
    let scratch = Scratch::new("host");
    let target = std::env::consts::OS;
    let (accepted, output) = scratch.check(target, PROGRAM);
    assert!(
        accepted,
        "the library must type-check for this host:\n{output}"
    );
}
