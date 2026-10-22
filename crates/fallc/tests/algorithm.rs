//! Algorithms over a range: sorting, searching, and reversing.
//!
//! They are free functions in `std/algorithm.ftm`, not members of `Array<T>`:
//! `sort(values)` takes a container, and `sort(first, last)` takes the two
//! positions `begin()` and `end()` hand back, so the same function serves an
//! array, a string, and a block of memory. The order is a comparator *type*,
//! the same one the tree and the heap take, so a descending sort is a different
//! type argument rather than a second algorithm.

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
            std::env::temp_dir().join(format!("fallc-sort-{label}-{}", std::process::id()));
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
fn a_sort_orders_the_elements_and_says_so() {
    // The promise is the order, not the algorithm: the elements are compared
    // with `<`, put in order, and `is_sorted` agrees afterwards.
    let source = r#"import std;

fn main() int {
    values Array<int>;
    values.push(5);
    values.push(1);
    values.push(4);
    values.push(2);
    values.push(3);
    C.printf("before %d\n", std.is_sorted(values.begin(), values.end()) as int);
    std.sort(values.begin(), values.end());
    C.printf("after %d:", std.is_sorted(values.begin(), values.end()) as int);
    for (value int : values) {
        C.printf(" %d", value);
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("ascending");
    assert_eq!(
        scratch.run("ascending", source),
        "before 0\nafter 1: 1 2 3 4 5\n"
    );
}

#[test]
fn a_sort_takes_its_order_from_the_comparator() {
    // The same algorithm twice, with two comparator *types*: `Less` orders
    // ascending and `Greater` descending. A symbol mangled without the
    // comparator made the two share one body, so whichever was emitted first
    // sorted both ways.
    let source = r#"import std;

fn main() int {
    ascending Array<int>;
    ascending.push(2);
    ascending.push(3);
    ascending.push(1);
    std.sort(ascending.begin(), ascending.end());
    C.printf("ascending %d %d %d\n", ascending[0], ascending[1], ascending[2]);

    descending Greater<int>;
    descending_values Array<int>;
    descending_values.push(2);
    descending_values.push(3);
    descending_values.push(1);
    std.sort_by(descending_values.begin(), descending_values.end(), descending);
    C.printf("descending %d %d %d\n", descending_values[0], descending_values[1], descending_values[2]);
    return 0;
}
"#;
    let scratch = Scratch::new("comparators");
    assert_eq!(
        scratch.run("comparators", source),
        "ascending 1 2 3\ndescending 3 2 1\n"
    );
}

#[test]
fn a_sort_of_strings_keeps_each_buffer_its_own() {
    // A swap runs through the element type's own copy and move, so a type that
    // owns memory still owns exactly one buffer after the order changed.
    let source = r#"import std;

fn main() int {
    words Array<String>;
    words.push(String("pear"));
    words.push(String("apple"));
    words.push(String("fig"));
    std.sort(words.begin(), words.end());
    C.printf("%s %s %s\n", words[0].data(), words[1].data(), words[2].data());
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(scratch.run("strings", source), "apple fig pear\n");
}

#[test]
fn reverse_search_count_and_fill_reach_the_elements() {
    // `reverse` is the in-place counterpart of a walk, `index_of` answers where
    // a value is (or `-1`), `count_of` how many there are, and `fill` writes
    // every slot.
    let source = r#"import std;

fn main() int {
    values Array<int>;
    values.push(1);
    values.push(2);
    values.push(3);
    values.push(2);
    std.reverse(values.begin(), values.end());
    C.printf("reversed %d %d %d %d\n", values[0], values[1], values[2], values[3]);
    C.printf("index_of 2 %ld, index_of 9 %ld\n", std.index_of(values.begin(), values.end(), 2), std.index_of(values.begin(), values.end(), 9));
    C.printf("count_of 2 %lu\n", std.count_of(values.begin(), values.end(), 2));
    std.fill(values.begin(), values.end(), 7);
    C.printf("fill %d %d %d %d\n", values[0], values[1], values[2], values[3]);
    return 0;
}
"#;
    let scratch = Scratch::new("reverse-search");
    assert_eq!(
        scratch.run("reverse-search", source),
        "reversed 2 3 2 1\nindex_of 2 0, index_of 9 -1\ncount_of 2 2\nfill 7 7 7 7\n"
    );
}

#[test]
fn the_same_algorithms_take_a_deque_iterator_pair() {
    // A deque's positions are iterators rather than pointers, so the range
    // algorithms have a second spelling: `sort(d.begin(), d.end())`. The
    // elements are reached through the buffer the iterator names.
    let source = r#"import std;

fn main() int except {
    d Deque<int> = Deque<int>();
    d.push_back(5);
    d.push_back(1);
    d.push_back(3);
    std.sort(d.begin(), d.end());
    C.printf("sorted %d %d %d\n", d[0], d[1], d[2]);
    C.printf("is_sorted %d\n", std.is_sorted(d.begin(), d.end()) as int);
    std.reverse(d.begin(), d.end());
    C.printf("reversed %d\n", d[0]);
    C.printf("index_of %ld count_of %lu\n", std.index_of(d.begin(), d.end(), 5), std.count_of(d.begin(), d.end(), 5));
    std.fill(d.begin(), d.end(), 7);
    C.printf("filled %d %d\n", d[0], d[2]);
    return 0;
}
"#;
    let scratch = Scratch::new("deque-iterators");
    assert_eq!(
        scratch.run("deque-iterators", source),
        "sorted 1 3 5\nis_sorted 1\nreversed 5\nindex_of 0 count_of 1\nfilled 7 7\n"
    );
}
