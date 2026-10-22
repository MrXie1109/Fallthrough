//! The `fallc` entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match fallc::cli::Options::parse(&args) {
        Ok(options) => fallc::run(&options),
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!();
            eprint!("{}", fallc::cli::USAGE);
            ExitCode::from(2)
        }
    }
}
