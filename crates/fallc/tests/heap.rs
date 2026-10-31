//! Heaps and priority queues: the value that comes first is on top.
//!
//! The order is a type -- `Less<T>` unless the program says `Greater<T>` -- and
//! the operations are `push`, `pop` and `top`.

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
            std::env::temp_dir().join(format!("fallc-heap-{label}-{}", std::process::id()));
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
fn a_heap_pops_its_values_in_order() {
    // Whatever the insertion order, a `Less` heap hands the values back from the
    // smallest up.
    let source = r#"import std;

fn main() int {
    h Heap<int> = Heap<int>();
    h.push(5);
    h.push(1);
    h.push(4);
    h.push(2);
    h.push(8);
    h.push(3);
    h.push(7);
    h.push(6);
    C.printf("length %d top %d\n", h.length() as int, h.top());
    for (i int = 0; i < 8; i += 1) {
        C.printf("%d ", h.pop());
    }
    C.printf("\nempty %d\n", h.empty() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("min");
    assert_eq!(
        scratch.run("min", source),
        "length 8 top 1\n1 2 3 4 5 6 7 8 \nempty 1\n"
    );
}

#[test]
fn the_order_type_decides_which_end_is_on_top() {
    // The same values, the other order: `Greater` keeps the largest first.
    let source = r#"import std;

fn main() int {
    h Heap<int, Greater<int>> = Heap<int, Greater<int>>();
    h.push(2);
    h.push(9);
    h.push(4);
    h.push(7);
    C.printf("%d %d %d %d\n", h.pop(), h.pop(), h.pop(), h.pop());
    return 0;
}
"#;
    let scratch = Scratch::new("max");
    assert_eq!(scratch.run("max", source), "9 7 4 2\n");
}

#[test]
fn a_priority_queue_is_a_heap_with_a_smaller_surface() {
    let source = r#"import std;

fn main() int {
    q PriorityQueue<int> = PriorityQueue<int>();
    q.push(30);
    q.push(10);
    q.push(20);
    C.printf("top %d length %d\n", q.top(), q.length() as int);
    C.printf("%d %d %d\n", q.pop(), q.pop(), q.pop());
    return 0;
}
"#;
    let scratch = Scratch::new("priority");
    assert_eq!(
        scratch.run("priority", source),
        "top 10 length 3\n10 20 30\n"
    );
}

#[test]
fn a_heap_of_strings_owns_and_reorders_them() {
    // Swapping two elements carries an owning value through a temporary, so a
    // heap of strings has to release each one exactly once.
    let source = r#"import std;

fn main() int {
    h Heap<String> = Heap<String>();
    h.push(String("pear"));
    h.push(String("apple"));
    h.push(String("fig"));
    while (!h.empty()) {
        C.printf("%s ", h.top().data());
        dropped String = h.pop();
    }
    C.printf("\n");
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(scratch.run("strings", source), "apple fig pear \n");
}

#[test]
fn a_comparator_of_the_programs_own_orders_a_heap() {
    // A type that says how to order itself is a heap's order directly, exactly
    // as it is a tree's.
    let source = r#"import std;

type Point {
    x int;
    y int;
}

fn __call__(this &const Nearer, a &const Point, b &const Point) bool {
    return a.x * a.x + a.y * a.y < b.x * b.x + b.y * b.y;
}

type Nearer {
}

fn main() int {
    h Heap<Point, Nearer> = Heap<Point, Nearer>();
    far Point = .{3, 4};
    near Point = .{1, 1};
    middle Point = .{2, 0};
    h.push(far);
    h.push(near);
    h.push(middle);
    C.printf("%d %d %d\n", h.pop().x, h.pop().x, h.pop().x);
    return 0;
}
"#;
    let scratch = Scratch::new("custom");
    assert_eq!(scratch.run("custom", source), "1 2 3\n");
}
