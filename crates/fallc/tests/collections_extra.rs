//! `std.Bitset`, `std.Ring` and `std.List`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-colextra-{label}-{}", std::process::id()));
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
fn a_bitset_packs_and_counts_its_bits() {
    let source = r#"import std;

fn main() int except {
    bits Bitset = Bitset(70usize);
    bits.set(0);
    bits.set(3);
    bits.set(69);
    std.print("{} {} {} {}\n", bits.length(), bits.count_ones(), bits.count_zeros(), bits.test(3) as int)?!;
    bits.toggle(3);
    std.print("{} {} {}\n", bits.test(3) as int, bits.count_ones(), bits.any() as int)?!;
    other Bitset = Bitset(70usize);
    other.set(0);
    bits.intersect(&other);
    std.print("{} {}\n", bits.test(0) as int, bits.test(69) as int)?!;
    bits.clear();
    std.print("{}\n", bits.none() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("bitset");
    assert_eq!(
        scratch.run("bitset", source),
        "70 3 67 1\n0 2 1\n1 0\n1\n"
    );
}

#[test]
fn a_ring_reuses_its_slots_in_order() {
    let source = r#"import std;

fn main() int except {
    ring Ring<int> = Ring<int>(3usize);
    std.print("{} {} {} {}\n",
        ring.push(1) as int, ring.push(2) as int,
        ring.push(3) as int, ring.push(4) as int)?!;
    std.print("{} {} {} {} {}\n", ring.length(), ring.capacity(),
        ring.full() as int, ring.front().value()?!, ring.back().value()?!)?!;
    std.print("{} {}\n", ring.pop().value()?!, ring.pop().value()?!)?!;
    ring.push(5);
    std.print("{} {}\n", ring.pop().value()?!, ring.length())?!;
    ring.clear();
    std.print("{}\n", ring.empty() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("ring");
    assert_eq!(
        scratch.run("ring", source),
        "1 1 1 0\n3 3 1 1 3\n1 2\n3 1\n1\n"
    );
}

#[test]
fn a_list_pushes_pops_and_walks() {
    let source = r#"import std;

fn main() int except {
    list List<int> = List<int>();
    list.push_back(2);
    list.push_back(3);
    list.push_front(1);
    std.print("{} {} {}\n", list.length(), list.front().value()?!, list.back().value()?!)?!;
    for (value &const int : list) {
        std.print("{} ", value)?!;
    }
    std.print("\n")?!;
    std.print("{} {} {}\n", list.pop_front().value()?!, list.pop_back().value()?!, list.length())?!;
    list.clear();
    std.print("{}\n", list.empty() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("list");
    assert_eq!(
        scratch.run("list", source),
        "3 1 3\n1 2 3 \n1 3 1\n1\n"
    );
}
