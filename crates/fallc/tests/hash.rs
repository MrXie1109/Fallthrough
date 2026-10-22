//! Hashing: the hasher, and the four containers built on it.
//!
//! `HMap` and `HSet` hold one value per key, `HMMap` and `HMSet` any number of
//! them. The order of a walk is a hash's, so these tests check what a hash table
//! promises instead: every key is found, a walk sees every entry once, and the
//! table keeps working after it grows.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-hash-{label}-{}", std::process::id()));
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
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn a_map_finds_what_it_was_given_and_replaces_a_repeated_key() {
    // The second value for one key replaces the first: a map is one value per
    // key, and inserting is the way to change it.
    let source = r#"import std;

fn main() int {
    m HMap<int, int> = HMap<int, int>();
    m.insert(1, 10);
    m.insert(2, 20);
    m.insert(1, 11);
    C.printf("length %d\n", m.length() as int);
    C.printf("one %d two %d\n", *m.find(1), *m.find(2));
    C.printf("missing %d\n", (m.find(9) == nullptr) as int);
    C.printf("has %d\n", m.contains(2) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("map");
    assert_eq!(
        scratch.run("map", source),
        "length 2\none 11 two 20\nmissing 1\nhas 1\n"
    );
}

#[test]
fn a_table_grows_past_its_first_buckets_without_losing_a_key() {
    // More entries than the table started with, so the buckets are rebuilt and
    // the chains relinked: every key still comes back, and a walk sees each
    // entry once.
    let source = r#"import std;

fn main() int {
    m HMap<int, int> = HMap<int, int>();
    for (i int = 0; i < 200; i += 1) {
        m.insert(i, i * 2);
    }
    C.printf("length %d\n", m.length() as int);
    total int = 0;
    seen int = 0;
    for (value &int : m) {
        total += value;
        seen += 1;
    }
    C.printf("seen %d total %d\n", seen, total);
    C.printf("last %d\n", *m.find(199));
    return 0;
}
"#;
    let scratch = Scratch::new("grow");
    // The values are `0, 2, ..., 398`: their sum is 199 * 200.
    assert_eq!(
        scratch.run("grow", source),
        "length 200\nseen 200 total 39800\nlast 398\n"
    );
}

#[test]
fn erasing_a_key_shortens_the_table_and_frees_the_slot() {
    // Erase removes one entry, and the key is then not found; the entry that
    // shared its bucket is still reachable.
    let source = r#"import std;

fn main() int {
    m HMap<int, int> = HMap<int, int>();
    m.insert(4, 40);
    m.insert(12, 120);
    m.insert(20, 200);
    C.printf("erased %d, again %d\n", m.erase(12) as int, m.erase(12) as int);
    C.printf("length %d, twelve %d\n", m.length() as int, (m.find(12) == nullptr) as int);
    C.printf("four %d, twenty %d\n", *m.find(4), *m.find(20));
    return 0;
}
"#;
    let scratch = Scratch::new("erase");
    assert_eq!(
        scratch.run("erase", source),
        "erased 1, again 0\nlength 2, twelve 1\nfour 40, twenty 200\n"
    );
}

#[test]
fn a_set_holds_each_key_once_and_walks_it() {
    // The value is a `Unit`, and the walk hands out the keys.
    let source = r#"import std;

fn main() int {
    s HSet<int> = HSet<int>();
    s.insert(5);
    s.insert(5);
    s.insert(6);
    s.insert(7);
    C.printf("length %d has 5 %d has 9 %d\n", s.length() as int, s.contains(5) as int, s.contains(9) as int);
    total int = 0;
    for (key &const int : s) {
        total += key;
    }
    C.printf("sum %d\n", total);
    return 0;
}
"#;
    let scratch = Scratch::new("set");
    assert_eq!(scratch.run("set", source), "length 3 has 5 1 has 9 0\nsum 18\n");
}

#[test]
fn a_string_key_is_found_by_its_text() {
    // The hash is the string's characters, so a key built at the call site finds
    // the entry the table holds.
    let source = r#"import std;

fn main() int {
    m HMap<String, int> = HMap<String, int>();
    m.insert(String("apple"), 1);
    m.insert(String("pear"), 2);
    C.printf("apple %d pear %d\n", *m.find(String("apple")), *m.find(String("pear")));
    C.printf("fig %d\n", m.contains(String("fig")) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(
        scratch.run("strings", source),
        "apple 1 pear 2\nfig 0\n"
    );
}

#[test]
fn a_multi_container_keeps_every_value_for_a_key() {
    // One entry per call: `count` says how many, a walk sees them all, and
    // `erase_all` takes them together.
    let source = r#"import std;

fn main() int {
    m HMMap<int, int> = HMMap<int, int>();
    m.insert(1, 10);
    m.insert(1, 11);
    m.insert(2, 20);
    C.printf("count %d length %d\n", m.count(1) as int, m.length() as int);
    total int = 0;
    for (value &int : m) {
        total += value;
    }
    C.printf("total %d\n", total);
    C.printf("erased %d, length %d\n", m.erase_all(1) as int, m.length() as int);

    s HMSet<int> = HMSet<int>();
    s.insert(3);
    s.insert(3);
    C.printf("set count %d\n", s.count(3) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("multi");
    assert_eq!(
        scratch.run("multi", source),
        "count 2 length 3\ntotal 41\nerased 2, length 1\nset count 2\n"
    );
}

#[test]
fn a_type_that_says_how_to_hash_itself_is_a_key() {
    // A user type is hashable by declaring `__hash__`, exactly as it is
    // comparable by declaring `__eq__`: the hasher asks the type.
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn __hash__(this &const Point) usize {
    return (this.x * 31 + this.y) as usize;
}

fn __eq__(this &const Point, other &const Point) bool {
    return this.x == other.x && this.y == other.y;
}

fn main() int {
    m HMap<Point, int> = HMap<Point, int>();
    first Point = .{1, 2};
    second Point = .{3, 4};
    absent Point = .{9, 9};
    m.insert(first, 12);
    m.insert(second, 34);
    C.printf("length %d\n", m.length() as int);
    C.printf("hit %d\n", *m.find(first));
    C.printf("miss %d\n", (m.find(absent) == nullptr) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("user");
    assert_eq!(
        scratch.run("user", source),
        "length 2\nhit 12\nmiss 1\n"
    );
}

#[test]
fn an_empty_declaration_builds_its_table() {
    // `m HMap<K, V>;` is a construction, not a zeroed object: the table allocates
    // its buckets when it is built. With the storage left as zeroes a bucket
    // index divided by a capacity of zero, and the first insertion aborted.
    let source = r#"import std;

fn main() int {
    m HMap<String, int>;
    m.insert(String("red"), 1);
    m.insert(String("green"), 2);
    s HSet<int>;
    s.insert(5);
    C.printf("%d %d %d\n", m.length(), s.length(), *m.find(String("green")));
    return 0;
}
"#;
    let scratch = Scratch::new("default");
    assert_eq!(scratch.run("default", source), "2 1 2\n");
}

#[test]
fn a_replaced_key_is_the_new_key() {
    // Replacing a value under an existing key also replaces the key. The entry
    // was found, its key destroyed and the new one moved in; a move assignment
    // that took the address of a *reference* argument passed the slot instead,
    // and the next lookup read a length out of it.
    let source = r#"import std;

fn main() int {
    m HMap<String, int>;
    m.insert(String("red"), 1);
    m.insert(String("green"), 2);
    m.insert(String("red"), 3);
    C.printf("%d %d %d\n", m.length(), *m.find(String("red")), *m.find(String("green")));
    C.printf("index %d\n", m[String("red")]);
    return 0;
}
"#;
    let scratch = Scratch::new("replace");
    assert_eq!(
        scratch.run("replace", source),
        "2 3 2\nindex 3\n"
    );
}
