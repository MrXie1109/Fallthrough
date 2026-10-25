//! `std.process`, `std.term`, `std.dynlib`, `std.signal`, `std.compress` and
//! `std.regex`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-sys-{label}-{}", std::process::id()));
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
fn a_child_exit_code_comes_back() {
    let source = r#"import std;

fn main() int except {
    program String = String("/bin/sh");
    arguments Array<String> = Array<String>();
    arguments.push(String("-c"));
    arguments.push(String("exit 3"));
    child Process = std.spawn(&program, &arguments)??;
    std.print("{}", child.wait()??)??;
    return 0;
}
"#;
    let scratch = Scratch::new("process");
    assert_eq!(scratch.run("process", source), "3");
}

#[test]
fn term_sequences_are_the_ansi_ones() {
    let source = r#"import std;

fn main() int except {
    std.print("{}{}{}{} {}", std.color_fg(std.Color.RED), std.style_bold(), std.style_reset(), std.color_bg(std.Color.BLUE), std.supports_color() as int)??;
    return 0;
}
"#;
    let scratch = Scratch::new("term");
    assert_eq!(
        scratch.run("term", source),
        "\u{1b}[31m\u{1b}[1m\u{1b}[0m\u{1b}[44m 0"
    );
}

#[test]
fn a_library_symbol_is_found_and_a_missing_one_is_null() {
    let source = r#"import std;
import std.dynlib;

fn main() int except {
    lib_path String = String("libm.so.6");
    lib Library = dynlib.open(&lib_path)??;
    sqrt_name String = String("sqrt");
    address *byte = lib.symbol(&sqrt_name)??;
    missing_name String = String("no_such_symbol_xyz");
    missing *byte = lib.symbol(&missing_name)??;
    std.print("{} {} {}", lib.loaded() as int, (address != nullptr) as int, (missing == nullptr) as int)??;
    lib.close();
    std.print(" {}", lib.loaded() as int)??;
    return 0;
}
"#;
    let scratch = Scratch::new("dynlib");
    assert_eq!(scratch.run("dynlib", source), "1 1 1 0");
}

#[test]
fn a_signal_reaches_its_handler() {
    let source = r#"import std;
import std.os;

received int;

fn remember(sig i32) void {
    received = sig;
}

fn main() int except {
    std.install(std.Signal.USR1, &remember)??;
    std.signal_self(std.Signal.USR1);
    os.sleep_ms(5);
    std.print("{}", received)??;
    std.restore(std.Signal.USR1);
    return 0;
}
"#;
    let scratch = Scratch::new("signal");
    assert_eq!(scratch.run("signal", source), "10");
}

#[test]
fn stored_blocks_round_trip_and_a_bad_checksum_raises() {
    let source = r#"import std;

fn main() int except {
    plain Bytes = Bytes();
    plain.push('h' as byte);
    plain.push('e' as byte);
    plain.push('l' as byte);
    plain.push('l' as byte);
    plain.push('o' as byte);
    packed Bytes = std.deflate_stored(&plain);
    back Bytes = std.inflate_stored(&packed)??;
    back_text String = String(back.data() as *const char);
    std.print("{} {} {}", packed.length(), back.length(), back_text)??;
    packed[packed.length() - 1usize] = 0 as u8;
    try {
        std.inflate_stored(&packed)??;
    } except {
        std.print(" {}", std.exception().name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("compress");
    assert_eq!(
        scratch.run("compress", source),
        "16 5 hello ValueError"
    );
}

#[test]
fn regex_anchors_classes_and_search() {
    let source = r#"import std;

fn main() int except {
    digits_pattern String = String("^[0-9]+$");
    digits Regex = std.compile(&digits_pattern)??;
    good String = String("12345");
    bad String = String("12a45");
    std.print("{} {} ", digits.is_match(&good)?? as int, digits.is_match(&bad)?? as int)??;
    word_pattern String = String("\\w+@\\w+");
    words Regex = std.compile(&word_pattern)??;
    email String = String("say hi@example");
    found Match = words.find(&email).value()??;
    std.print("{} {} {}", found.start(), found.end(), found.matched(&email)??)??;
    try {
        group_pattern String = String("a(b)c");
        std.compile(&group_pattern)??;
    } except {
        std.print(" {}", std.exception().name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("regex");
    assert_eq!(
        scratch.run("regex", source),
        "1 0 4 14 hi@example ValueError"
    );
}

#[test]
fn a_loopback_connection_carries_a_request_and_an_answer() {
    let source = r#"import std;
import std.net;

type Echo {
    listener TcpListener;
}

fn serve(raw *byte) u32 {
    holder *Echo = raw as *Echo;
    received int = 0;
    try {
        stream TcpStream = holder.listener.accept()??;
        question String = stream.read_text(64usize)??;
        answer Bytes = Bytes();
        answer.append_string(String("pong:"));
        answer.append_string(question);
        stream.write_all(&answer)??;
        received = 1;
    } except {
    }
    return received as u32;
}

fn main() int except {
    loopback String = String("127.0.0.1");
    listener TcpListener = net.listen(&loopback, 0u16, 8)??;
    port u16 = listener.local_port()??;
    holder Echo = .{ listener };
    thread Thread = Thread(&serve, (&holder) as *byte)??;
    stream TcpStream = net.connect(&loopback, port)??;
    request Bytes = Bytes();
    request.append_string(String("ping"));
    stream.write_all(&request)??;
    answer String = stream.read_text(64usize)??;
    std.print("{}", answer)??;
    thread.join()??;
    return 0;
}
"#;
    let scratch = Scratch::new("net");
    assert_eq!(scratch.run("net", source), "pong:ping");
}
