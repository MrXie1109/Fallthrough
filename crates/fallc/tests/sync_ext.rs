//! The synchronisation primitives added beside `Mutex`/`CondVar`/`Atomic`:
//! `RwLock`, `Semaphore`, `Once` and `Barrier`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-syncext-{label}-{}", std::process::id()));
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
fn a_read_write_lock_takes_readers_and_one_writer() {
    let source = r#"import std;
import std.sync;

fn main() int except {
    lock RwLock = RwLock();
    lock.lock_shared();
    lock.lock_shared();
    lock.unlock_shared();
    lock.unlock_shared();
    lock.lock();
    lock.unlock();
    std.print("rw ok\n")??;
    return 0;
}
"#;
    let scratch = Scratch::new("rwlock");
    assert_eq!(scratch.run("rwlock", source), "rw ok\n");
}

#[test]
fn a_semaphore_counts_permits() {
    let source = r#"import std;
import std.sync;

fn main() int except {
    sem Semaphore = Semaphore(2u32);
    std.print("{} ", sem.available())??;
    sem.acquire();
    sem.acquire();
    std.print("{} ", sem.available())??;
    sem.release();
    std.print("{}\n", sem.available())??;
    return 0;
}
"#;
    let scratch = Scratch::new("semaphore");
    assert_eq!(scratch.run("semaphore", source), "2 0 1\n");
}

#[test]
fn once_runs_its_action_once() {
    let source = r#"import std;
import std.sync;

var calls int;

fn action() void {
    calls += 1;
}

fn main() int except {
    once Once = Once();
    once.call(&action);
    once.call(&action);
    once.call(&action);
    std.print("{}\n", calls)??;
    return 0;
}
"#;
    // Module-level variables use the declaration form the language uses.
    let source = source.replace("var calls int;", "calls int;").replace(
        "fn main() int except {",
        "fn main() int except {\n    calls = 0;",
    );
    let scratch = Scratch::new("once");
    assert_eq!(scratch.run("once", &source), "1\n");
}

#[test]
fn a_barrier_waits_for_every_party() {
    let source = r#"import std;
import std.sync;

type Context {
    hits *Atomic<int>;
    bar *Barrier;
}

fn worker(argument *byte) u32 {
    context Context = *(argument as *Context);
    context.hits->fetch_add(1);
    context.bar->wait();
    return 0;
}

fn main() int except {
    counter Atomic<int> = Atomic<int>(0);
    bar Barrier = Barrier(2u32);
    context Context = .{ &counter, &bar };
    thread Thread = Thread(worker, &context as *byte)??;
    bar.wait();
    thread.join()??;
    std.print("{}\n", counter.load())??;
    return 0;
}
"#;
    let scratch = Scratch::new("barrier");
    assert_eq!(scratch.run("barrier", source), "1\n");
}
