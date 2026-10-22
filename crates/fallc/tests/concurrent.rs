//! `std.concurrent`: promise/future, channel, and the thread pool.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-conc-{label}-{}", std::process::id()));
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
fn a_future_receives_the_value_a_promise_sets() {
    let source = r#"import std;
import std.concurrent;
import std.sync;

type Answer {
    promise *Promise<int>;
}

fn producer(argument *byte) u32 {
    answer Answer = *(argument as *Answer);
    try {
        answer.promise->set(42)?!;
    } except {
    }
    return 0;
}

fn main() int except {
    promise Promise<int> = Promise<int>();
    future Future<int> = promise.future();
    answer Answer = .{ &promise };
    thread Thread = Thread(producer, &answer as *byte)?!;
    value int = future.get()?!;
    thread.join()?!;
    std.print("{} {}\n", value, future.ready() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("future");
    assert_eq!(scratch.run("future", source), "42 1\n");
}

#[test]
fn a_channel_carries_values_in_order() {
    let source = r#"import std;
import std.concurrent;

type Feed {
    channel *Channel<int>;
}

fn producer(argument *byte) u32 {
    feed Feed = *(argument as *Feed);
    try {
        feed.channel->send(1)?!;
        feed.channel->send(2)?!;
        feed.channel->send(3)?!;
        feed.channel->close();
    } except {
    }
    return 0;
}

fn main() int except {
    channel Channel<int> = Channel<int>();
    feed Feed = .{ &channel };
    thread Thread = Thread(producer, &feed as *byte)?!;
    first int = channel.recv()?!;
    second int = channel.recv()?!;
    third int = channel.recv()?!;
    thread.join()?!;
    std.print("{} {} {}\n", first, second, third)?!;
    std.print("{} {}\n", channel.closed() as int, channel.try_recv().present() as int)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("channel");
    assert_eq!(scratch.run("channel", source), "1 2 3\n1 0\n");
}

#[test]
fn a_pool_runs_every_job_on_its_workers() {
    let source = r#"import std;
import std.concurrent;
import std.sync;

fn job(argument *byte) u32 {
    counter *Atomic<int> = argument as *Atomic<int>;
    counter->fetch_add(1);
    return 0;
}

fn main() int except {
    counter Atomic<int> = Atomic<int>(0);
    pool ThreadPool = ThreadPool(2usize);
    pool.start()?!;
    pool.submit(job, &counter as *byte);
    pool.submit(job, &counter as *byte);
    pool.submit(job, &counter as *byte);
    pool.shutdown()?!;
    std.print("{}\n", counter.load())?!;
    return 0;
}
"#;
    let scratch = Scratch::new("pool");
    assert_eq!(scratch.run("pool", source), "3\n");
}
