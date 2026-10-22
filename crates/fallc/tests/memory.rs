//! Memory: ownership and reference counting.
//!
//! `Box` owns one value and frees it when it goes; `Rc` shares one and frees it
//! with the last owner; `RcWeak` watches it without keeping it alive. These are
//! behavioural tests: what a program can observe is the counts, the value, and
//! whether an observer still finds it.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// A directory of scratch sources, removed when it goes out of scope.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!("fallc-mem-{label}-{}", std::process::id()));
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
fn a_box_owns_one_value_and_moving_it_moves_the_value() {
    // The source is empty after the move, and the value is reached through the
    // box that holds it now.
    let source = r#"import std;

fn main() int {
    a Box<int> = Box<int>(7);
    b Box<int> = std.move(a);
    C.printf("%d %d %d\n", a.empty() as int, b.empty() as int, b.get());
    return 0;
}
"#;
    let scratch = Scratch::new("box");
    assert_eq!(scratch.run("box", source), "1 0 7\n");
}

#[test]
fn an_rc_shares_one_value_between_owners() {
    // A copy of an owner shares the value: three owners, one value, and every
    // owner reads the same thing.
    let source = r#"import std;

fn main() int {
    first Rc<int> = Rc<int>(7);
    second Rc<int> = first.clone();
    third Rc<int> = second.clone();
    C.printf("%d %d %d\n", first.strong_count(), second.strong_count(), third.strong_count());
    C.printf("%d %d %d\n", *first, *second, *third);
    first.get() = 9;
    C.printf("%d\n", *third);
    return 0;
}
"#;
    let scratch = Scratch::new("share");
    assert_eq!(scratch.run("share", source), "3 3 3\n7 7 7\n9\n");
}

#[test]
fn an_owner_may_be_released_and_the_value_is_destroyed_with_the_last_one() {
    // The last owner destroys the value, and the observer's block outlives it:
    // the observer reports that the value is gone and gives an empty owner.
    let source = r#"import std;

fn main() int {
    owner Rc<int> = Rc<int>(7);
    watcher RcWeak<int> = owner.downgrade();
    C.printf("observers %d\n", watcher.weak_count());
    owner = Rc<int>();
    C.printf("expired %d\n", watcher.expired() as int);
    revived Rc<int> = watcher.upgrade();
    C.printf("revived %d %d\n", revived.empty() as int, watcher.expired() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("release");
    assert_eq!(
        scratch.run("release", source),
        "observers 1\nexpired 1\nrevived 1 1\n"
    );
}

#[test]
fn an_owner_keeps_the_block_alive_for_an_observer() {
    // An observer taken before the last owner goes keeps the block: the counts
    // stay readable, and the value is gone once the owners are.
    let source = r#"import std;

fn main() int {
    owner Rc<int> = Rc<int>(7);
    watcher RcWeak<int> = owner.downgrade();
    third Rc<int> = owner.clone();
    C.printf("%d %d\n", owner.strong_count(), watcher.weak_count());
    owner = Rc<int>();
    third = Rc<int>();
    C.printf("expired %d strong %d\n", watcher.expired() as int, watcher.strong_count());
    return 0;
}
"#;
    let scratch = Scratch::new("observe");
    assert_eq!(
        scratch.run("observe", source),
        "2 1\nexpired 1 strong 0\n"
    );
}

#[test]
fn an_allocator_is_a_type_the_container_asks() {
    // The allocator is a *type*, so the container calls its `alloc` and `free`
    // as ordinary methods. This one counts: one block is taken for the control
    // block and given back when the last owner goes.
    let source = r#"import std;
import std.util as util;

type Counting {
}

blocks int = 0;
returned int = 0;

fn alloc(this &Counting, size usize) *byte {
    blocks += 1;
    return util.allocate(size);
}

fn free(this &Counting, block *byte) void {
    returned += 1;
    util.free(block);
}

fn main() int {
    counting Counting = .{};
    one Rc<int, Counting> = Rc<int, Counting>(7, counting);
    two Rc<int, Counting> = one.clone();
    C.printf("while alive %d %d\n", blocks, returned);
    one = Rc<int, Counting>();
    two = Rc<int, Counting>();
    C.printf("after %d %d\n", blocks, returned);
    return 0;
}
"#;
    let scratch = Scratch::new("allocator");
    assert_eq!(
        scratch.run("allocator", source),
        "while alive 1 0\nafter 1 1\n"
    );
}

#[test]
fn an_rc_may_be_empty() {
    // The empty state is a null control block, so an owner with nothing is not
    // an error and asks nothing of the allocator.
    let source = r#"import std;

fn main() int {
    empty Rc<int> = Rc<int>();
    C.printf("%d %d %d\n", empty.empty() as int, empty.strong_count(), empty.weak_count());
    return 0;
}
"#;
    let scratch = Scratch::new("empty");
    assert_eq!(scratch.run("empty", source), "1 0 0\n");
}

#[test]
fn the_allocator_trait_answers_for_the_default_allocator() {
    // `Allocator` is a trait, so a program can ask whether a type is one. The
    // answer for the default allocator is yes, and one with no methods is not.
    let source = r#"import std;

type Nothing {
}

fn main() int {
    if (Allocator(DefaultAllocator)) @constexpr {
        C.printf("default yes\n");
    }
    if (Allocator(Nothing)) @constexpr {
        C.printf("nothing yes\n");
    } else {
        C.printf("nothing no\n");
    }
    return 0;
}
"#;
    let scratch = Scratch::new("trait");
    assert_eq!(scratch.run("trait", source), "default yes\nnothing no\n");
}

#[test]
fn an_arc_shares_one_value_and_watches_it() {
    // The same contract as `Rc`, with counts that two threads may change at
    // once: the value is freed with the last owner, and an observer can tell
    // when that happened.
    let source = r#"import std;

fn main() int {
    first Arc<int> = Arc<int>(7);
    second Arc<int> = first.clone();
    C.printf("strong %d weak %d\n", first.strong_count(), second.weak_count());
    watcher ArcWeak<int> = first.downgrade();
    C.printf("after %d\n", first.weak_count());
    first = Arc<int>();
    C.printf("expired %d\n", watcher.expired() as int);
    second = Arc<int>();
    C.printf("expired %d\n", watcher.expired() as int);
    revived Arc<int> = watcher.upgrade();
    C.printf("upgrade empty %d\n", revived.empty() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("arc");
    assert_eq!(
        scratch.run("arc", source),
        "strong 2 weak 0\nafter 1\nexpired 0\nexpired 1\nupgrade empty 1\n"
    );
}

#[test]
fn a_move_assignment_takes_the_reference_as_it_stands() {
    // `b = std.move(a)` passes an `&&String` to `__assign__`'s rvalue-reference
    // parameter. That value *is* the address of `a`, so taking its address
    // handed the method a pointer to the pointer: `b` then took a length out of
    // the slot and the next read was a wild pointer.
    let source = r#"import std;

fn main() int {
    a String = "hello";
    b String;
    b = std.move(a);
    C.printf("%s %d\n", b.data(), b.length());
    C.printf("moved from %d\n", a.empty() as int);
    return 0;
}
"#;
    let scratch = Scratch::new("move-assign");
    assert_eq!(
        scratch.run("move-assign", source),
        "hello 5\nmoved from 1\n"
    );
}
