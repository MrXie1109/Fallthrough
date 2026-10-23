//! Zed support for the Fallthrough language.
//!
//! The extension does two things: it gives Zed the language's file suffixes and
//! comment syntax, and it starts the language server, which is the compiler
//! itself (`fallc --lsp`). Everything the server knows -- types, modules,
//! templates, exceptions -- is therefore the compiler's own answer, and an
//! editor and the command line cannot disagree about what a program means.
//!
//! # Finding `fallc`
//!
//! In order:
//!
//! 1. the `fallc` on the shell's `PATH`, which is what `make install` puts
//!    there;
//! 2. `$FALLC`, for a build of one's own that is not on `PATH`;
//! 3. the worktree's `target/release/fallc` or `target/debug/fallc`, so that
//!    working on the compiler in Zed uses the compiler being worked on.
//!
//! A user who wants to pin a compiler does it in Zed's own settings, which Zed
//! applies before asking the extension for a command:
//!
//! ```json
//! { "lsp": { "fallc": { "binary": { "path": "/path/to/fallc", "arguments": ["--lsp"] } } } }
//! ```
//!
//! # Why the environment is read through the worktree
//!
//! An extension is WebAssembly, and `std::env::var` there does not see the
//! user's shell: the worktree is what carries the environment Zed was started
//! with, so `$FALLC` is looked up there.

use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

/// The language server id declared in `extension.toml`.
const SERVER_ID: &str = "fallc";

struct FallthroughExtension {
    /// The executable found for the server, so a workspace is not searched on
    /// every start. The language server id is part of the key because Zed may
    /// start more than one.
    cached_binary: Option<(LanguageServerId, String)>,
}

impl FallthroughExtension {
    /// Returns the `fallc` executable to run.
    fn binary(&mut self, language_server_id: &LanguageServerId, worktree: &Worktree) -> Result<String> {
        if let Some((cached_id, binary)) = &self.cached_binary
            && cached_id == language_server_id
        {
            return Ok(binary.clone());
        }
        let binary = Self::find(worktree)?;
        self.cached_binary = Some((language_server_id.clone(), binary.clone()));
        Ok(binary)
    }

    fn find(worktree: &Worktree) -> Result<String> {
        if let Some(binary) = worktree.which("fallc") {
            return Ok(binary);
        }
        // `shell_env` is a list of pairs rather than a map: the WIT interface
        // models the environment as an ordered list, which is what a process
        // receives.
        if let Some((_, binary)) = worktree
            .shell_env()
            .iter()
            .find(|(name, value)| name == "FALLC" && !value.is_empty())
        {
            return Ok(binary.clone());
        }
        for candidate in ["target/release/fallc", "target/debug/fallc"] {
            let absolute = format!("{}/{}", worktree.root_path(), candidate);
            if std::fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(absolute);
            }
        }
        Err("`fallc` was not found: install it with `sudo make install`, put one on `PATH`, \
             set `FALLC`, or point `lsp.fallc.binary.path` at the compiler in Zed's settings"
            .to_owned())
    }
}

impl zed::Extension for FallthroughExtension {
    fn new() -> Self {
        Self {
            cached_binary: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        if language_server_id.as_ref() != SERVER_ID {
            return Err(format!(
                "unknown language server `{}`",
                language_server_id.as_ref()
            ));
        }
        let binary = self.binary(language_server_id, worktree)?;
        // The server reads and writes the protocol on stdin and stdout.
        Ok(Command {
            command: binary,
            args: vec!["--lsp".to_owned()],
            env: Default::default(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // The server takes its configuration from the environment and from the
        // documents it is sent; it has no initialization options of its own.
        Ok(None)
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Nothing to configure yet: the standard library is found through
        // `FALLTHROUGH_STANDARD` in the shell environment, which the server
        // inherits from Zed.
        Ok(None)
    }
}

zed::register_extension!(FallthroughExtension);
