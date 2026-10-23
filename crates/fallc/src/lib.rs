//! The `fallc` compiler driver.
//!
//! The driver is the only crate that knows about the outside world: file
//! system, command line, exit codes, and the order in which the other crates
//! run. Everything else in the workspace is a library with no I/O.
//!
//! # Module map
//!
//! * [`cli`] — argument parsing into an [`cli::Options`].
//! * [`session`] — global state for one compilation: the [`session::Session`]
//!   owns the source map and the diagnostics collected across all stages.
//! * [`pipeline`] — runs the stages in order and stops at the first fatal one.
//! * [`emit`] — renders stage output for the user.
//! * [`lsp`] — the language server `fallc --lsp` starts for editors.
//!
//! # Stage order
//!
//! ```text
//! read files
//!   └── lex      (fallc-lexer)
//!         └── parse  (fallc-parser)
//!               └── resolve + typeck (fallc-sema)
//!                     └── lower + validate (fallc-ir)
//!                           └── emit
//! ```
//!
//! Each stage is skipped if an earlier stage reported a fatal diagnostic,
//! because later stages assume well-formed input from earlier ones.

#![forbid(unsafe_code)]

pub mod cli;
pub mod emit;
pub mod lsp;
pub mod modules;
pub mod pipeline;
pub mod session;

pub use cli::{Command, Options};
pub use pipeline::{Outcome, run};
pub use session::Session;
