//! A whole program of the shape the compiler was blamed for: two matrices,
//! a function that returns one, a reference to one, and a checksum.
//!
//! Every line here once failed. A module-level `seed`, a `[N][N]double` return,
//! `c[i][j]` through a reference, `C.stdin`/`C.stdout` -- each was its own
//! internal error. The test keeps them together and checks the arithmetic
//! against the same computation in Rust, so a code generation mistake that
//! produces a plausible number is still caught.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

/// One xorshift64 step, with the wrapping shifts the language's `^`/`<<`/`>>`
/// on `u64` have.
fn step(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

/// The matrix the Fallthrough program builds: values in [0, 1) from the
/// generator's low three decimal digits.
fn make_matrix(seed: &mut u64, n: usize) -> Vec<Vec<f64>> {
    let mut matrix = vec![vec![0.0; n]; n];
    for row in matrix.iter_mut() {
        for cell in row.iter_mut() {
            *cell = (step(seed) % 1000) as f64 / 1000.0;
        }
    }
    matrix
}

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-matrix-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join("main");
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "the program did not compile"
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
fn a_matrix_program_compiles_and_agrees_with_the_same_computation_in_rust() {
    // Small enough to run quickly at -O0, large enough that the array really is
    // passed, returned and subscripted as storage rather than as a value.
    const N: usize = 16;
    let source = format!(
        r#"import std;
import C;

N const usize = {N};
seed u64 = 88172645463325252;

fn xorshift() u64 {{
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    return seed;
}}

fn make_matrix() [N][N]double {{
    m [N][N]double;
    for (i usize = 0; i < N; i += 1) {{
        for (j usize = 0; j < N; j += 1) {{
            m[i][j] = (xorshift() % 1000) as double / 1000.0;
        }}
    }}
    return m;
}}

fn matmul(a &const [N][N]double, b &const [N][N]double, c &[N][N]double) void {{
    for (i usize = 0; i < N; i += 1) {{
        for (j usize = 0; j < N; j += 1) {{
            c[i][j] = 0.0;
        }}
    }}
    for (i usize = 0; i < N; i += 1) {{
        for (k usize = 0; k < N; k += 1) {{
            aik double = a[i][k];
            for (j usize = 0; j < N; j += 1) {{
                c[i][j] += aik * b[k][j];
            }}
        }}
    }}
}}

fn sum_matrix(c &const [N][N]double) double {{
    s double = 0.0;
    for (i usize = 0; i < N; i += 1) {{
        for (j usize = 0; j < N; j += 1) {{
            s += c[i][j];
        }}
    }}
    return s;
}}

fn main() int {{
    a [N][N]double = make_matrix();
    b [N][N]double = make_matrix();
    c [N][N]double;
    matmul(&a, &b, &c);
    C.printf("%.6f\n", sum_matrix(&c));
    return 0;
}}
"#
    );

    // The same computation in Rust, accumulated in the same order.
    let mut seed = 88_172_645_463_325_252_u64;
    let a = make_matrix(&mut seed, N);
    let b = make_matrix(&mut seed, N);
    let mut c = vec![vec![0.0_f64; N]; N];
    for i in 0..N {
        for k in 0..N {
            let aik = a[i][k];
            for j in 0..N {
                c[i][j] += aik * b[k][j];
            }
        }
    }
    let total: f64 = c.iter().flatten().sum();

    let scratch = Scratch::new("matmul");
    let printed = scratch.run(&source);
    assert_eq!(printed, format!("{total:.6}\n"));
}
