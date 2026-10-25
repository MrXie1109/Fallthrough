//! `std.crc`, `std.crypto`, `std.uuid` and `std.rand_dist`.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-crypto-{label}-{}", std::process::id()));
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
fn crc_checksums_match_their_standard_vectors() {
    let source = r#"import std;
import std.bytes;
from std.bytes import Bytes;

fn main() int except {
    check Bytes = Bytes();
    text String = String("123456789");
    index usize = 0;
    while (index < text.length()) {
        check.push(text[index] as byte);
        index += 1;
    }
    std.print("{:08x} {:04x} {:02x}\n", std.crc32(&check), std.crc16(&check), std.crc8(&check))??;
    return 0;
}
"#;
    let scratch = Scratch::new("crc");
    assert_eq!(scratch.run("crc", source), "cbf43926 29b1 f4\n");
}

#[test]
fn sha256_and_hmac_match_the_standard_vectors() {
    let source = r#"import std;
import std.bytes;
from std.bytes import Bytes;

fn bytes_of(text &const String) Bytes {
    result Bytes = Bytes();
    index usize = 0;
    while (index < text.length()) {
        result.push(text[index] as byte);
        index += 1;
    }
    return result;
}

fn main() int except {
    abc String = String("abc");
    abc_bytes Bytes = bytes_of(&abc);
    std.print("{}\n", std.sha256_hex(&abc_bytes))??;
    empty String = String("");
    empty_bytes Bytes = bytes_of(&empty);
    std.print("{}\n", std.sha256_hex(&empty_bytes))??;
    key Bytes = Bytes();
    counter int = 0;
    while (counter < 20) {
        key.push(0x0b as byte);
        counter += 1;
    }
    data String = String("Hi There");
    data_bytes Bytes = bytes_of(&data);
    std.print("{}\n", std.hmac_sha256_hex(&key, &data_bytes))??;
    return 0;
}
"#;
    let scratch = Scratch::new("sha");
    assert_eq!(
        scratch.run("sha", source),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
         b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7\n"
    );
}

#[test]
fn uuid_version_four_round_trips_through_its_text() {
    let source = r#"import std;

fn main() int except {
    identifier Uuid = std.v4()??;
    text String = identifier.to_string();
    std.print("{} {} {} ", text.length(), identifier.version(), identifier.variant())??;
    parsed Uuid = std.parse_uuid(&text)??;
    std.print("{} {}", (parsed == identifier) as int, (parsed != identifier) as int)??;
    try {
        bad String = String("0123456789abcdef0123456789abcdef0123456789");
        std.parse_uuid(&bad)??;
    } except {
        std.print(" {}", std.exception().name())??;
    }
    return 0;
}
"#;
    let scratch = Scratch::new("uuid");
    assert_eq!(
        scratch.run("uuid", source),
        "36 4 2 1 0 UuidError"
    );
}

#[test]
fn random_distributions_sample_in_their_ranges() {
    let source = r#"import std;
import std.rand_dist;

fn main() int except {
    generator Random = Random(11u32);
    exponential_value f64 = rand_dist.sample_exponential(&generator, 2.0);
    normal_value f64 = rand_dist.sample_normal(&generator, 0.0, 1.0);
    poisson_value u32 = rand_dist.sample_poisson(&generator, 3.0);
    binomial_value usize = rand_dist.sample_binomial(&generator, 10usize, 0.5);
    geometric_value u32 = rand_dist.sample_geometric(&generator, 0.5);
    weights Array<f64> = Array<f64>();
    weights.push(0.0);
    weights.push(1.0);
    values Array<int> = Array<int>();
    values.push(1);
    values.push(2);
    picked int = generator.weighted_choice(&values, &weights) as int;
    std.print("{} {} {} {} {} {}\n",
        exponential_value > 0.0,
        normal_value > -10.0 && normal_value < 10.0,
        poisson_value < 100u32,
        binomial_value <= 10usize,
        geometric_value >= 1u32,
        picked)??;
    return 0;
}
"#;
    let scratch = Scratch::new("dist");
    assert_eq!(scratch.run("dist", source), "true true true true true 2\n");
}
