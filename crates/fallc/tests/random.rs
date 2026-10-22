//! Random numbers: the generator, its distributions, and the global stream.
//!
//! The generator is MT19937, so the reference stream is a fact these tests can
//! pin: seed 5489 is the C++ default seed, and the first three words it produces
//! are the ones `std::mt19937` produces. The distributions are checked by what
//! they promise (a value in the range, a permutation, a spread) rather than by
//! the numbers, because the numbers are the generator's business.

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
            std::env::temp_dir().join(format!("fallc-random-{label}-{}", std::process::id()));
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
fn the_generator_matches_the_reference_stream() {
    // MT19937 with seed 5489: the first three words C++'s `std::mt19937`
    // produces. Pinning them is what says the state, the twist and the
    // tempering are the standard's rather than "some generator".
    let source = r#"import std;
import C;

fn main() int {
    local Random = Random(5489u32);
    C.printf("%u %u %u\n", local(), local(), local());
    local.seed(5489u32);
    C.printf("%u\n", local());
    return 0;
}
"#;
    let scratch = Scratch::new("reference");
    assert_eq!(
        scratch.run("reference", source),
        "3499211612 581869302 3890346734\n3499211612\n"
    );
}

#[test]
fn the_global_generator_is_seeded_and_advances() {
    // It is seeded from the system at setup, so two runs draw different
    // numbers; within one run each draw advances the stream.
    let source = r#"import std;
import C;

fn main() int {
    first u32 = std.random();
    second u32 = std.random();
    C.printf("%d\n", (first != second) as int);
    C.printf("in range %d\n", (std.random.uniform(1, 6) >= 1 && std.random.uniform(1, 6) <= 6) as int);
    C.printf("draw %u\n", first);
    return 0;
}
"#;
    let scratch = Scratch::new("global");
    let first_run = scratch.run("global-one", source);
    assert!(
        first_run.starts_with("1\nin range 1\ndraw "),
        "{first_run}"
    );
    let second_run = scratch.run("global-two", source);
    assert_ne!(
        first_run, second_run,
        "two runs with the system entropy source seed the generator differently"
    );
}

#[test]
fn the_distributions_stay_inside_their_ranges() {
    // A range's ends are included, a real draw is in `[low, high)`, a normal
    // draw is finite, a shuffle is a permutation, and a coin's two sides both
    // appear over enough throws. The numbers themselves are the generator's.
    let source = r#"import std;
import C;

fn main() int {
    local Random = Random(12345u32);

    lowest int = 100;
    highest int = 0;
    index int = 0;
    while (index < 500) {
        roll int = local.uniform(1, 6);
        if (roll < lowest) {
            lowest = roll;
        }
        if (roll > highest) {
            highest = roll;
        }
        index += 1;
    }
    C.printf("uniform ends %d %d\n", lowest, highest);

    inside bool = true;
    index2 int = 0;
    while (index2 < 200) {
        real f64 = local.uniform_real(-2.0, 3.0);
        if (real < -2.0 || real >= 3.0) {
            inside = false;
        }
        index2 += 1;
    }
    C.printf("real inside %d\n", inside as int);

    spread f64 = local.normal(10.0, 2.0);
    C.printf("normal finite %d\n", (spread > 0.0 && spread < 100.0) as int);

    heads int = 0;
    tails int = 0;
    index3 int = 0;
    while (index3 < 200) {
        if (local.bernoulli(0.5)) {
            heads += 1;
        } else {
            tails += 1;
        }
        index3 += 1;
    }
    C.printf("coin both sides %d\n", (heads > 0 && tails > 0) as int);

    values Array<int>;
    values.push(1);
    values.push(2);
    values.push(3);
    values.push(4);
    values.push(5);
    local.shuffle(&values);
    seen [5]int;
    seen[0] = 0;
    seen[1] = 0;
    seen[2] = 0;
    seen[3] = 0;
    seen[4] = 0;
    index4 usize = 0;
    while (index4 < values.length()) {
        seen[(values[index4] - 1) as usize] = 1;
        index4 += 1;
    }
    C.printf("permutation %d\n", (seen[0] + seen[1] + seen[2] + seen[3] + seen[4] == 5) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("distributions");
    assert_eq!(
        scratch.run("distributions", source),
        "uniform ends 1 6\nreal inside 1\nnormal finite 1\ncoin both sides 1\npermutation 1\n"
    );
}

#[test]
fn a_seed_makes_a_stream_repeatable() {
    // The point of a seed: two generators seeded alike are the same stream.
    let source = r#"import std;
import C;

fn main() int {
    one Random = Random(7u32);
    two Random = Random(7u32);
    C.printf("same %d\n", (one() == two()) as int);
    one.seed(9u32);
    two.seed(10u32);
    C.printf("different %d\n", (one() != two()) as int);
    return 0;
}
"#;
    let scratch = Scratch::new("repeatable");
    assert_eq!(
        scratch.run("repeatable", source),
        "same 1\ndifferent 1\n"
    );
}

#[test]
fn the_argument_type_chooses_the_distribution() {
    // One `uniform` serves both kinds: integer arguments take the integer form,
    // floating arguments the real one, and no caller writes `_real`.
    let source = r#"import std;
import C;

fn main() int {
    inside bool = true;
    i usize = 0;
    while (i < 32) {
        value int = std.random.uniform(1, 6);
        if (value < 1 || value > 6) {
            inside = false;
        }
        i += 1;
    }
    C.printf("int %d\n", inside as int);
    real_ok bool = true;
    j usize = 0;
    while (j < 32) {
        r f64 = std.random.uniform(0.0, 1.0);
        if (r < 0.0 || r >= 1.0) {
            real_ok = false;
        }
        j += 1;
    }
    C.printf("real %d\n", real_ok as int);
    return 0;
}
"#;
    let scratch = Scratch::new("uniform-type");
    assert_eq!(scratch.run("uniform-type", source), "int 1\nreal 1\n");
}

#[test]
fn the_distributions_have_the_means_they_promise() {
    // A statistical draw is checked by its first two facts: the mean, and the
    // support. A fixed seed keeps the sample the same on every run.
    let source = r#"import std;
import C;

fn main() int except {
    r Random = Random(2024u32);
    total f64 = 0.0;
    positive bool = true;
    i usize = 0;
    while (i < 4000) {
        value f64 = r.exponential(2.0);
        total = total + value;
        if (value <= 0.0) {
            positive = false;
        }
        i += 1;
    }
    exp_mean f64 = total / 4000.0;
    C.printf("exponential %d %d\n", (exp_mean > 0.4 && exp_mean < 0.6) as int, positive as int);

    total = 0.0;
    i = 0;
    while (i < 4000) {
        total = total + (r.poisson(2.5) as f64);
        i += 1;
    }
    poisson_mean f64 = total / 4000.0;
    C.printf("poisson %d\n", (poisson_mean > 2.2 && poisson_mean < 2.8) as int);

    total = 0.0;
    i = 0;
    while (i < 2000) {
        total = total + (r.binomial(10, 0.3) as f64);
        i += 1;
    }
    binomial_mean f64 = total / 2000.0;
    C.printf("binomial %d\n", (binomial_mean > 2.6 && binomial_mean < 3.4) as int);

    total = 0.0;
    i = 0;
    while (i < 4000) {
        total = total + (r.geometric(0.5) as f64);
        i += 1;
    }
    geometric_mean f64 = total / 4000.0;
    C.printf("geometric %d\n", (geometric_mean > 1.85 && geometric_mean < 2.15) as int);

    log_total f64 = 0.0;
    i = 0;
    while (i < 1000) {
        log_total = log_total + r.lognormal(0.0, 1.0);
        i += 1;
    }
    // exp of a standard normal has mean exp(0.5), about 1.65.
    log_mean f64 = log_total / 1000.0;
    C.printf("lognormal %d\n", (log_mean > 1.3 && log_mean < 2.0) as int);

    values Array<int> = Array<int>();
    values.push(7);
    values.push(8);
    values.push(9);
    pick int = r.choice(&values);
    C.printf("choice %d\n", ((pick >= 7 && pick <= 9)) as int);
    weights Array<f64> = Array<f64>();
    weights.push(0.0);
    weights.push(1.0);
    weights.push(0.0);
    weighted int = r.weighted_choice(&values, &weights);
    C.printf("weighted %d\n", weighted);
    return 0;
}
"#;
    let scratch = Scratch::new("distribution-means");
    assert_eq!(
        scratch.run("distribution-means", source),
        "exponential 1 1\npoisson 1\nbinomial 1\ngeometric 1\nlognormal 1\nchoice 1\nweighted 8\n"
    );
}
