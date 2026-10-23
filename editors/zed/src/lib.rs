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
//! 1. the `fallc` on `PATH`, which is what `make install` puts there;
//! 2. `$FALLC`, for a build of one's own that is not on `PATH`;
//! 3. the workspace's `target/release/fallc` or `target/debug/fallc`, so that
//!    working on the compiler in Zed uses the compiler being worked on.
//!
//! A relative path is expanded against the worktree root, using Zed's shell
//! environment support, so a project can name its own compiler in its settings.

use std::path::PathBuf;

use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

/// The language server id declared in `extension.toml`.
const SERVER_ID: &str = "fallc";

/// The language the server is started for.
const LANGUAGE: &str = "Fallthrough";

struct FallthroughExtension {
    /// The executable found for the server, so a workspace is not searched on
    /// every start.
    cached_binary: Option<String>,
}

impl FallthroughExtension {
    /// Returns the `fallc` executable to run.
    fn binary(&mut self, worktree: &Worktree) -> Result<String> {
        if let Some(binary) = &self.cached_binary {
            return Ok(binary.clone());
        }
        // An explicit setting wins: a project that pins a compiler says so.
        if let Some(configured) = worktree
            .config()
            .settings
            .get(FALLTHROUGH_SETTINGS)
            .and_then(|settings| settings.get("server_path"))
            .and_then(|value| value.as_str())
        {
            let binary = worktree.which(configured).unwrap_or_else(|| configured.to_owned());
            self.cached_binary = Some(binary.clone());
            return Ok(binary);
        }
        if let Some(binary) = worktree.which("fallc") {
            self.cached_binary = Some(binary.clone());
            return Ok(binary);
        }
        // `std::env::var` does not read the user's environment inside an
        // extension: it runs as WebAssembly, and the worktree is what knows the
        // shell's variables.
        if let Some(binary) = worktree
            .shell_env()
            .get("FALLC")
            .filter(|path| !path.is_empty())
            .cloned()
        {
            self.cached_binary = Some(binary.clone());
            return Ok(binary);
        }
        for candidate in ["target/release/fallc", "target/debug/fallc"] {
            let absolute = PathBuf::from(worktree.root_path()).join(candidate);
            if absolute.is_file() {
                let binary = absolute.to_string_lossy().into_owned();
                self.cached_binary = Some(binary.clone());
                return Ok(binary);
            }
        }
        Err(format!(
            "`fallc` was not found: install it with `sudo make install`, put one on `PATH`, \
             set `FALLC`, or set `fallthrough.server_path` to the compiler in this project"
        ))
    }
}

/// The settings section the extension reads.
const FALLTHROUGH_SETTINGS: &str = "fallthrough";

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
            return Err(format!("unknown language server `{}`", language_server_id.as_ref()));
        }
        let binary = self.binary(worktree)?;
        // The server reads and writes the protocol on stdin and stdout, which
        // is the default for a `Command` with no arguments beyond these.
        Ok(Command {
            command: binary,
            args: vec!["--lsp".to_owned()],
            env: Default::default(),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let mut settings = zed::serde_json::Map::new();
        let _ = LANGUAGE;
        // The standard library location, if the project has one of its own: the
        // server's own lookup already honours `FALLTHROUGH_STANDARD`, and this
        // is how a project states the same thing for a compile that keeps the
        // library inside itself.
        if let Some(configured) = worktree
            .config()
            .settings
            .get(FALLTHROUGH_SETTINGS)
            .and_then(|settings| settings.get("standard_library"))
            .and_then(|value| value.as_str())
        {
            settings.insert(
                "standardLibrary".to_owned(),
                zed::serde_json::Value::String(configured.to_owned()),
            );
        }
        if settings.is_empty() {
            Ok(None)
        } else {
            Ok(Some(zed::serde_json::Value::Object(settings)))
        }
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
}

zed::register_extension!(FallthroughExtension);
