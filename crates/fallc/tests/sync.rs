//! Synchronisation: a mutex, an atomic value, and threads.
//!
//! These tests run more than one thread, so what they check is what threads
//! promise: two increments never both read the same old value, and a lock keeps
//! the region between its arms to one thread at a time.

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
            std::env::temp_dir().join(format!("fallc-sync-{label}-{}", std::process::id()));
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

const WORKER: &str = r#"fn worker(argument *byte) u32 {
    counter *Atomic<int> = argument as *Atomic<int>;
    for (i int = 0; i < 20000; i += 1) {
        counter->fetch_add(1);
    }
    return 0u32;
}
"#;

#[test]
fn two_threads_incrementing_an_atomic_value_lose_nothing() {
    let source = format!(
        r#"import std;

{WORKER}
fn main() int except {{
    shared Atomic<int> = Atomic<int>(0);
    first Thread = Thread(&worker, &shared)??;
    second Thread = Thread(&worker, &shared)??;
    first.join()??;
    second.join()??;
    C.printf("%d\n", shared.load());
    return 0;
}}
"#
    );
    let scratch = Scratch::new("atomic");
    assert_eq!(scratch.run("atomic", &source), "40000\n");
}

#[test]
fn a_mutex_makes_an_ordinary_value_safe_to_share() {
    // The value is a plain `int`; only the lock is what keeps the two threads
    // from reading it at once.
    let source = r#"import std;

type Shared {
    mutex Mutex;
    value int;
}

fn worker(argument *byte) u32 {
    shared *Shared = argument as *Shared;
    for (i int = 0; i < 20000; i += 1) {
        shared->mutex.lock();
        shared->value = shared->value + 1;
        shared->mutex.unlock();
    }
    return 0u32;
}

fn main() int except {
    shared Shared;
    (&shared.mutex)->__init__();
    shared.value = 0;
    first Thread = Thread(&worker, &shared)??;
    second Thread = Thread(&worker, &shared)??;
    first.join()??;
    second.join()??;
    C.printf("%d\n", shared.value);
    return 0;
}
"#;
    let scratch = Scratch::new("mutex");
    assert_eq!(scratch.run("mutex", source), "40000\n");
}

#[test]
fn a_lock_guard_releases_the_mutex_when_the_scope_ends() {
    let source = r#"import std;

fn main() int except {
    m Mutex = Mutex();
    {
        guard Lock = Lock(&m);
        C.printf("held\n");
    }
    m.lock();
    C.printf("released\n");
    m.unlock();
    std.sleep(1);
    C.printf("slept\n");
    return 0;
}
"#;
    let scratch = Scratch::new("guard");
    assert_eq!(scratch.run("guard", source), "held\nreleased\nslept\n");
}

#[test]
fn a_thread_local_global_is_one_object_per_thread() {
    // `thread_local` puts the object in thread-local storage: each thread gets
    // its own copy, initialised from the declaration, and the main thread's
    // value is untouched by what the workers wrote.
    let source = r#"import std;

seen int @thread_local = 0;

fn worker(argument *byte) u32 {
    slot *int = argument as *int;
    seen = 0;
    for (i int = 0; i < 100; i += 1) {
        seen += 1;
    }
    *slot = seen;
    return 0u32;
}

fn main() int except {
    seen = 41;
    first_value int = 0;
    second_value int = 0;
    first Thread = Thread(&worker, &first_value as *byte)??;
    second Thread = Thread(&worker, &second_value as *byte)??;
    first.join()??;
    second.join()??;
    C.printf("main %d first %d second %d\n", seen, first_value, second_value);
    return 0;
}
"#;
    let scratch = Scratch::new("thread-local");
    assert_eq!(
        scratch.run("thread-local", source),
        "main 41 first 100 second 100\n"
    );
}

#[test]
fn a_condition_variable_waits_for_a_predicate() {
    // The predicate form asks the question again after every wake, which is the
    // loop a caller would otherwise write by hand.
    let source = r#"import std;
import C;

fn ready() bool {
    return true;
}

fn main() int except {
    m Mutex = Mutex();
    cv CondVar = CondVar();
    cv.wait(&m, ready);
    C.printf("waited\n");
    return 0;
}
"#;
    let scratch = Scratch::new("condvar-predicate");
    assert_eq!(scratch.run("condvar-predicate", source), "waited\n");
}

#[test]
fn atomics_take_an_ordering_with_a_default() {
    // Every operation takes an `AtomicOrder`, defaulting to `SEQ_CST`. The
    // current implementation is a mutex, so it runs at least as strongly as the
    // order asked for.
    let source = r#"import std;

fn main() int {
    a Atomic<int> = Atomic<int>(1);
    C.printf("%d %d\n", a.load(), a.load(AtomicOrder.ACQUIRE));
    a.store(5, AtomicOrder.RELEASE);
    C.printf("%d\n", a.load(AtomicOrder.SEQ_CST));
    C.printf("old %d now %d\n", a.fetch_add(2, AtomicOrder.ACQ_REL), a.load());
    first bool = a.compare_exchange(7, 9, AtomicOrder.SEQ_CST);
    second bool = a.compare_exchange(100, 0);
    C.printf("cas %d %d old %d now %d\n",
        first as int, second as int,
        a.exchange(1, AtomicOrder.RELAXED), a.load());
    return 0;
}
"#;
    let scratch = Scratch::new("atomic-order");
    assert_eq!(
        scratch.run("atomic-order", source),
        "1 1\n5\nold 5 now 7\ncas 1 0 old 9 now 1\n"
    );
}
