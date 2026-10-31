//! Trees: the ordered containers.
//!
//! `TSet` and `TMap` hold one key each, `TMSet` and `TMMap` any number of them,
//! and a walk visits the keys in order. The order is a *type* -- `Less<E>` unless
//! the program says otherwise -- so these tests cover both the containers and the
//! comparator that gives them their order.

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
            std::env::temp_dir().join(format!("fallc-tree-{label}-{}", std::process::id()));
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
fn a_set_keeps_its_keys_in_order() {
    // Insertion order is not the order a set has: the walk is by key, and a key
    // is there once however often it was inserted.
    let source = r#"import std;

fn main() int {
    s TSet<int> = TSet<int>();
    s.insert(3);
    s.insert(1);
    s.insert(2);
    s.insert(2);
    C.printf("length %d\n", s.length() as int);
    for (key &const int : s) {
        C.printf("%d ", key);
    }
    C.printf("\n");
    C.printf("has 2 %d, has 5 %d\n", s.contains(2) as int, s.contains(5) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("set");
    assert_eq!(
        scratch.run("set", source),
        "length 3\n1 2 3 \nhas 2 1, has 5 0\n"
    );
}

#[test]
fn a_map_finds_its_values_and_walks_them_in_key_order() {
    // `find` answers the storage of the value, or null when the key is not
    // there; an iterator is the key and the value together.
    let source = r#"import std;

fn main() int {
    m TMap<int, int> = TMap<int, int>();
    m.insert(2, 20);
    m.insert(1, 10);
    m.insert(3, 30);
    found *int = m.find(2);
    C.printf("found %d\n", *found);
    missing *int = m.find(9);
    C.printf("missing %d\n", (missing == nullptr) as int);
    it MapIterator<int, int> = m.begin();
    stop MapIterator<int, int> = m.end();
    while (it != stop) {
        C.printf("%d:%d ", it.key(), *it);
        ++it;
    }
    C.printf("\n");
    C.printf("erased %d, length %d\n", m.erase(1) as int, m.length() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("map");
    assert_eq!(
        scratch.run("map", source),
        "found 20\nmissing 1\n1:10 2:20 3:30 \nerased 1, length 2\n"
    );
}

#[test]
fn a_multi_set_and_a_multi_map_keep_every_value_for_a_key() {
    // The multi containers are one entry per call: the count says how many, and
    // `erase_all` takes them together.
    let source = r#"import std;

fn main() int {
    s TMSet<int> = TMSet<int>();
    s.insert(7);
    s.insert(7);
    s.insert(7);
    s.insert(4);
    C.printf("count %d, all %d\n", s.count(7) as int, s.length() as int);
    C.printf("erased %d, length %d\n", s.erase_all(7) as int, s.length() as int);

    m TMMap<int, int> = TMMap<int, int>();
    m.insert(5, 50);
    m.insert(5, 51);
    C.printf("map count %d\n", m.count(5) as int);
    C.printf("map erases %d\n", m.erase_all(5) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("multi");
    assert_eq!(
        scratch.run("multi", source),
        "count 3, all 4\nerased 3, length 1\nmap count 2\nmap erases 2\n"
    );
}

#[test]
fn the_order_is_the_comparators_and_a_greater_walks_backwards() {
    // The comparator is a type, so a descending set is the same container with
    // a different order.
    let source = r#"import std;

fn main() int {
    s TSet<int, Greater<int>> = TSet<int, Greater<int>>();
    s.insert(1);
    s.insert(3);
    s.insert(2);
    for (key &const int : s) {
        C.printf("%d ", key);
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("order");
    assert_eq!(scratch.run("order", source), "3 2 1 \n");
}

#[test]
fn a_key_of_a_type_the_language_cannot_compare_needs_its_own_order() {
    // Strings are compared by a comparator of their own, which is what makes a
    // set of them possible at all: the tree asks the *type* how to order, and
    // never compares two values itself.
    let source = r#"import std;

fn main() int {
    s TSet<String, Less<String>> = TSet<String, Less<String>>();
    s.insert(String("pear"));
    s.insert(String("apple"));
    s.insert(String("fig"));
    for (key &const String : s) {
        C.printf("%s ", key.data());
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(scratch.run("strings", source), "apple fig pear \n");
}

#[test]
fn removing_a_key_keeps_the_walk_shorter() {
    // An erase rewrites the links, so the next walk is the tree as it is now.
    let source = r#"import std;

fn main() int {
    s TSet<int> = TSet<int>();
    for (n int = 0; n < 8; n += 1) {
        s.insert(n);
    }
    s.erase(0);
    s.erase(4);
    s.erase(7);
    for (key &const int : s) {
        C.printf("%d ", key);
    }
    C.printf("\nlength %d\n", s.length() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("erase");
    assert_eq!(scratch.run("erase", source), "1 2 3 5 6 \nlength 5\n");
}

#[test]
fn a_map_is_indexed_by_the_key_it_holds() {
    // `[]` takes the key by `&const K`, which is how a key that owns memory is
    // passed without copying it. A value written where the reference belongs is
    // the same rule a call argument follows -- `find` accepts it too -- so the
    // subscript has to accept it as well.
    let source = r#"import std;

fn main() int {
    m TMap<String, int>;
    m.insert(String("apple"), 1);
    m.insert(String("pear"), 2);
    C.printf("apple %d pear %d\n", m[String("apple")], m.at(String("pear")));
    m[String("apple")] = 7;
    C.printf("after %d\n", m[String("apple")]);
    return 0;
}
"#;
    let scratch = Scratch::new("index");
    assert_eq!(scratch.run("index", source), "apple 1 pear 2\nafter 7\n");
}
