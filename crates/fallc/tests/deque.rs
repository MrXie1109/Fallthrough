//! Deques, queues and stacks: sequences that are cheap at both ends.
//!
//! A `Deque` is a ring buffer, so `push_front` is as cheap as `push_back`; the
//! queue and the stack are views of it that only expose one end.

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
            std::env::temp_dir().join(format!("fallc-deque-{label}-{}", std::process::id()));
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
fn a_deque_pushes_and_pops_at_both_ends() {
    // The two ends are independent: what went in at the front comes out at the
    // front, and the count is the same from either side.
    let source = r#"import std;

fn main() int {
    d Deque<int> = Deque<int>();
    d.push_back(2);
    d.push_back(3);
    d.push_front(1);
    d.push_front(0);
    C.printf("length %d front %d back %d\n", d.length() as int, d.front(), d.back());
    for (value &int : d) {
        C.printf("%d ", value);
    }
    C.printf("\n");
    C.printf("front %d then %d, back %d then %d\n", d.pop_front(), d.pop_front(), d.pop_back(), d.pop_back());
    C.printf("empty %d\n", d.empty() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("ends");
    assert_eq!(
        scratch.run("ends", source),
        "length 4 front 0 back 3\n0 1 2 3 \nfront 0 then 1, back 3 then 2\nempty 1\n"
    );
}

#[test]
fn a_deque_wraps_around_its_buffer_and_grows_without_losing_order() {
    // Pushing at the front walks the front index backwards, and the buffer
    // wraps; growing copies the elements in order, so the walk is unchanged.
    let source = r#"import std;

fn main() int {
    d Deque<int> = Deque<int>();
    for (i int = 0; i < 20; i += 1) {
        d.push_front(i);
    }
    for (i int = 0; i < 20; i += 1) {
        d.push_back(100 + i);
    }
    C.printf("length %d\n", d.length() as int);
    total int = 0;
    seen int = 0;
    in_order bool = true;
    for (value &int : d) {
        if (seen < 20) {
            if (value != 19 - seen) {
                in_order = false;
            }
        } else {
            if (value != 100 + (seen - 20)) {
                in_order = false;
            }
        }
        total += value;
        seen += 1;
    }
    C.printf("in order %d total %d\n", in_order as int, total);
    C.printf("front %d back %d\n", d.front(), d.back());
    return 0;
}
"#;
    let scratch = Scratch::new("grow");
    // Fronts were 19..0 and backs 100..119; their sum is 190 + 2190.
    assert_eq!(
        scratch.run("grow", source),
        "length 40\nin order 1 total 2380\nfront 19 back 119\n"
    );
}

#[test]
fn a_deque_of_strings_owns_them() {
    // The elements are constructed in place and destroyed where they stood, so
    // an owning type is released exactly once.
    let source = r#"import std;

fn main() int {
    d Deque<String> = Deque<String>();
    d.push_back(String("beta"));
    d.push_front(String("alpha"));
    d.push_back(String("gamma"));
    for (value &String : d) {
        C.printf("%s ", value.data());
    }
    C.printf("\n");
    C.printf("popped %s\n", d.pop_front().data());
    C.printf("left %d\n", d.length() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("strings");
    assert_eq!(
        scratch.run("strings", source),
        "alpha beta gamma \npopped alpha\nleft 2\n"
    );
}

#[test]
fn a_queue_waits_and_a_stack_does_not() {
    // The same deque, used from opposite ends: the queue returns what arrived
    // first and the stack what arrived last.
    let source = r#"import std;

fn main() int {
    q Queue<int> = Queue<int>();
    q.push(1);
    q.push(2);
    q.push(3);
    C.printf("queue %d %d %d\n", q.pop(), q.pop(), q.pop());
    s Stack<int> = Stack<int>();
    s.push(1);
    s.push(2);
    s.push(3);
    C.printf("stack %d %d %d\n", s.pop(), s.pop(), s.pop());
    return 0;
}
"#;
    let scratch = Scratch::new("adapters");
    assert_eq!(
        scratch.run("adapters", source),
        "queue 1 2 3\nstack 3 2 1\n"
    );
}

#[test]
fn a_deque_copies_and_moves_as_a_value() {
    // A copy owns its own buffer, and a move takes the other's: the copy is
    // unchanged after the original is changed.
    let source = r#"import std;

fn main() int {
    first Deque<int> = Deque<int>();
    first.push_back(1);
    first.push_back(2);
    copy Deque<int> = first;
    first.push_back(3);
    C.printf("copy %d first %d\n", copy.length() as int, first.length() as int);
    moved Deque<int> = std.move(copy);
    C.printf("copy %d moved %d moved front %d\n", copy.length() as int, moved.length() as int, moved.front());
    return 0;
}
"#;
    let scratch = Scratch::new("value");
    assert_eq!(
        scratch.run("value", source),
        "copy 2 first 3\ncopy 0 moved 2 moved front 1\n"
    );
}
