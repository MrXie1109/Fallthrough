//! Implicit initialisation and implicit conversion (`__im_init__` / `__im_as__`).

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-implicit-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn options(&self, label: &str, source: &str) -> (PathBuf, Options) {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        (executable, options)
    }

    fn accepted(&self, label: &str, source: &str) -> String {
        let (executable, options) = self.options(label, source);
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

    fn rejected(&self, label: &str, source: &str) {
        let (_executable, options) = self.options(label, source);
        assert_eq!(
            run(&options),
            std::process::ExitCode::FAILURE,
            "`{label}` was accepted but must be rejected"
        );
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn an_implicit_conversion_is_used_in_every_target_position() {
    let source = r#"import std;
import C;

type Src { n int; }
type Dst { n int; }
type Holder { value Dst; }

fn __im_as__(this &const Src, *Dst) Dst {
    return .{this.n + 1};
}

fn take(value Dst) int {
    return value.n;
}

fn produce() Dst {
    source Src = .{10};
    return source;
}

fn main() int except {
    source Src = .{10};
    binding Dst = source;
    assignment Dst = .{0};
    assignment = source;
    holder Holder = .{ source };
    C.printf("%d %d %d %d %d\n",
        binding.n, take(source), assignment.n, produce().n, holder.value.n);
    return 0;
}
"#;
    let scratch = Scratch::new("used");
    assert_eq!(
        scratch.accepted("used", source),
        "11 11 11 11 11\n"
    );
}

#[test]
fn an_explicit_conversion_is_not_implicit() {
    let scratch = Scratch::new("explicit");
    scratch.rejected(
        "as-only",
        r#"type Src { n int; }
type Dst { n int; }

fn __as__(this &const Src, *Dst) Dst {
    return .{this.n};
}

fn main() int {
    source Src = .{1};
    target Dst = source;
    return target.n;
}
"#,
    );
    // Writing the conversion out is fine: `__as__` is the explicit form.
    let source = r#"import std;
import C;

type Src { n int; }
type Dst { n int; }

fn __as__(this &const Src, *Dst) Dst {
    return .{this.n + 2};
}

fn main() int except {
    source Src = .{1};
    target Dst = source as Dst;
    C.printf("%d\n", target.n);
    return 0;
}
"#;
    assert_eq!(scratch.accepted("as-written", source), "3\n");
}

#[test]
fn an_explicit_constructor_is_not_implicit() {
    let scratch = Scratch::new("init");
    scratch.rejected(
        "init-only",
        r#"import std;

type Wrapped { text String; }

fn __init__(this &Wrapped, text *const char) void {
    this.text = String(text);
}

fn main() int except {
    value Wrapped = "not implicit";
    return 0;
}
"#,
    );
    let source = r#"import std;
import C;

type Wrapped { text String; }

fn __im_init__(this &Wrapped, text *const char) void {
    (&this.text)->__init__();
    this.text.push(String(text));
}

fn main() int except {
    implicit Wrapped = "implicit";
    explicit Wrapped = Wrapped("explicit");
    C.printf("%s %s\n", implicit.text.data(), explicit.text.data());
    return 0;
}
"#;
    assert_eq!(
        scratch.accepted("im-init", source),
        "implicit explicit\n"
    );
}
