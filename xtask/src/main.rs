//! Repository automation, run as `cargo xtask <command>`.
//!
//! `check` is the gate. `hooks install` points git at `.githooks`. `server` and
//! `schema` forward to Ember's xtask through the submodule, because those drive
//! Keen's binary rather than anything this repository owns.

pub mod check;
mod hooks;
#[cfg(test)]
mod policy;
mod runner;
mod server;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::runner::Processes;

/// Commit scopes: the workspace crate names, the fixtures directory, and the
/// three cross-cutting names no crate will ever own.
const SCOPES: &[&str] = &[
    "private-chests",
    "common",
    "xtask",
    "fixtures",
    "deps",
    "ci",
    "release",
];

#[derive(Parser)]
#[command(name = "xtask", about = "Repository automation for the mods workspace")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the commit scopes this repository accepts.
    Scopes,
    /// Run the gate: every check a change has to pass.
    Check,
    /// Manage this clone's git hooks.
    Hooks {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Drive the development server through Ember's xtask.
    Server {
        /// Arguments for Ember's `server` command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Extract or diff the server schema through Ember's xtask.
    Schema {
        /// Arguments for Ember's `schema` command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Point `core.hooksPath` at `.githooks` for this clone.
    Install,
}

/// The repository root, which is the directory above this crate's manifest.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the xtask manifest sits one level below the repository root")
        .to_path_buf()
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// Run the parsed command and report the code the process exits with.
fn dispatch() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let runner = Processes;
    let mut out = io::stdout();

    match cli.command {
        Command::Scopes => {
            for scope in SCOPES {
                println!("{scope}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Check => {
            let rows = check::Gate::new(check::STEPS, &runner).run(&mut out)?;
            Ok(if rows.iter().all(check::Row::passed) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Hooks {
            action: HookAction::Install,
        } => {
            hooks::Hooks::new(&runner).install(&mut out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Server { args } => {
            server::Delegate::new(&runner, repo_root()).run("server", &args, &mut out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Schema { args } => {
            server::Delegate::new(&runner, repo_root()).run("schema", &args, &mut out)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::{Cli, SCOPES, repo_root};

    /// Clap rejects an ambiguous or malformed command definition, and a broken
    /// one only shows up at run time otherwise.
    #[test]
    fn the_command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    /// The scope list feeds `commitlint.config.js`, so a duplicate or an empty
    /// entry would accept a commit nobody meant to allow.
    #[test]
    fn scopes_are_distinct_and_named() {
        let mut seen = SCOPES.to_vec();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();

        assert_eq!(seen.len(), count, "two scopes share a name");
        assert!(SCOPES.iter().all(|scope| !scope.is_empty()));
    }

    /// The root is the workspace root, which is where the gate and the hooks
    /// both act.
    #[test]
    fn the_repository_root_holds_the_workspace_manifest() {
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(repo_root().join("xtask").is_dir());
    }
}
