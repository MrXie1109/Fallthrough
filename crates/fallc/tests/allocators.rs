//! `std.allocators`: the C-heap default and the arena.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-alloc-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn the_default_allocator_hands_out_and_takes_back() {
    let source = r#"import std;
import std.allocators;

fn main() int except {
    heap DefaultAllocator = DefaultAllocator();
    block *byte = heap.alloc(16usize);
    std.print("{}\n", (block != nullptr) as int)??;
    heap.free(block);
    std.print("freed\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("default");
    assert_eq!(scratch.run("default", source), "1\nfreed\n");
}

#[test]
fn an_arena_bumps_a_cursor_and_resets() {
    let source = r#"import std;
import std.allocators;

fn main() int except {
    arena Arena = Arena(64usize);
    first *byte = arena.alloc(8usize);
    second *byte = arena.alloc(8usize);
    std.print("{} {} {}\n", arena.available(), (first != second) as int, (first != nullptr) as int)??;
    arena.reset();
    std.print("{}\n", arena.available())??;
    return 0;
}
"#;
    let scratch = Scratch::new("arena");
    assert_eq!(scratch.run("arena", source), "48 1 1\n64\n");
}
