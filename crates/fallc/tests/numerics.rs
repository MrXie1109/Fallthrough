//! `std.math` extensions, `std.complex` and `std.matrix`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-num-{label}-{}", std::process::id()));
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
fn math_extensions_answer_their_definitions() {
    let source = r#"import std;

fn main() int except {
    std.print("{} {} {} {}\n", std.hypot(3.0, 4.0), std.cbrt(-27.0), std.trunc(-2.75), std.fract(2.75))?!;
    std.print("{} {} {}\n", std.gcd(12, 18), std.lcm(4, 6), std.isqrt(17))?!;
    std.print("{} {} {} {}\n", std.is_power_of_two(64), std.is_even(10), std.is_nan(std.nan()), std.is_infinite(std.infinity()))?!;
    std.print("{} {}\n", std.degrees(std.pi()), std.radians(180.0))?!;
    return 0;
}
"#;
    let scratch = Scratch::new("math");
    assert_eq!(
        scratch.run("math", source),
        "5 -3 -2 0.75\n6 12 4\ntrue true true true\n180 3.14159\n"
    );
}

#[test]
fn complex_arithmetic_follows_the_formulas() {
    let source = r#"import std;

fn main() int except {
    first Complex = std.complex(1.0, 2.0);
    second Complex = std.complex(3.0, -4.0);
    product Complex = first.mul(&second);
    quotient Complex = first.div(&second)?!;
    std.print("{} {} {} {}\n", product, first.conjugate(), first.abs(), first.arg())?!;
    std.print("{} {} {} {}\n", quotient, first.norm_squared(), first == second, first != second)?!;
    power Complex = std.complex(0.0, 1.0).pow(2.0)?!;
    std.print("{}", power)?!;
    return 0;
}
"#;
    let scratch = Scratch::new("complex");
    assert_eq!(
        scratch.run("complex", source),
        "11+2i 1-2i 2.23607 1.10715\n-0.2+0.4i 5 false true\n-1+1.22465e-16i"
    );
}

#[test]
fn matrix_arithmetic_checks_its_dimensions() {
    let source = r#"import std;

fn main() int except {
    identity Matrix = std.identity(2usize);
    value Matrix = std.matrix(2usize, 3usize, 2.0);
    product Matrix = identity.mul(&value)?!;
    std.print("{} {} {} {}\n", product.rows(), product.columns(), product.at(1usize, 2usize)?!, value.transpose().rows())?!;
    std.print("{} {} {}\n", value.transpose().columns(), product == value, product != value)?!;
    scaled Matrix = value.scale(0.5);
    std.print("{} {}", scaled.at(0usize, 0usize)?!, std.matrix(2usize, 2usize, 1.0).add(&identity)?!.at(1usize, 1usize)?!)?!;
    try {
        identity.mul(&value)?!;
        value.mul(&identity)?!;
    } except {
        std.print(" {}", std.exception().name())?!;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("matrix");
    assert_eq!(
        scratch.run("matrix", source),
        "2 3 2 3\n2 true false\n1 2 ValueError"
    );
}
