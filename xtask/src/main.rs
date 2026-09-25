//! Repository automation, run as `cargo xtask <command>`.
//!
//! `check` is the gate, and `pins` is its first step run alone. `setup`
//! installs what the pin files name, once they pass. `package` builds a mod's
//! release bundle. `server`
//! and `schema` forward to Ember's xtask through the submodule, because those
//! drive Keen's binary rather than anything this repository owns.

pub mod check;
pub mod package;
pub mod pins;
mod proof;
mod runner;
mod server;
mod shellcheck;
mod spawn;
mod tree;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use crate::runner::Processes;

/// The commit scope vocabulary: the workspace crate names, the fixtures
/// directory, and the cross-cutting names no crate will ever own.
///
/// `commitlint.config.js` reads the same file, so the scopes this command
/// prints and the scopes the commit hook accepts are one list.
const SCOPES_JSON: &str = include_str!("../../.github/commit-scopes.json");

/// One commit scope and the one sentence saying what it covers.
#[derive(serde::Deserialize)]
struct Scope {
    scope: String,
    covers: String,
}

/// The commit scopes, in the order the scope file lists them.
///
/// # Errors
///
/// Returns an error when the scope file is not a JSON array of
/// `{ scope, covers }` objects.
fn scopes() -> anyhow::Result<Vec<Scope>> {
    serde_json::from_str(SCOPES_JSON).context("reading .github/commit-scopes.json")
}

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
    Check {
        /// Print the rows and what each checks, and run nothing.
        #[arg(long)]
        rows: bool,
    },
    /// Hold both mise pin files, their lockfiles and the configs the gate's
    /// tools read to their rules, which the gate does first.
    Pins,
    /// Install every tool mise.toml pins, from mise.lock, once the pin files
    /// pass their rules.
    Setup,
    /// Build a mod's release bundle: one archive and its digest file.
    Package {
        /// The mod to bundle, by its package name.
        #[arg(long = "mod", value_name = "MOD")]
        subject: String,
        /// The release tag the archive is named for.
        #[arg(long, value_name = "TAG")]
        tag: String,
        /// The directory the archive and its digest file are written into.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// A loader already on disk, with its SHA256SUMS beside it, in place of
        /// the download from Ember's release.
        #[arg(long, value_name = "PATH", conflicts_with = "ember_release")]
        ember_loader: Option<PathBuf>,
        /// A directory an earlier job filled from Ember's release: the loader,
        /// its SHA256SUMS and a TAG file naming the release, which must be the
        /// one the lockfile resolves.
        #[arg(long, value_name = "DIR", requires = "ember_release_digest")]
        ember_release: Option<PathBuf>,
        /// The loader's SHA-256 as the job that filled `--ember-release`
        /// recorded it, which the loader must match.
        #[arg(long, value_name = "SHA256", requires = "ember_release")]
        ember_release_digest: Option<String>,
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
    /// Stand in for `ShellCheck` under actionlint, which the gate's actionlint
    /// row names.
    #[command(name = shellcheck::SUBCOMMAND, hide = true)]
    ShellcheckStandIn {
        /// The `ShellCheck` path, then the arguments actionlint passes.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
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

    // The gate reads the tree and starts every tool from the repository root,
    // whichever directory it was started in.
    if matches!(cli.command, Command::Check { .. } | Command::Pins) {
        std::env::set_current_dir(repo_root()).context("entering the repository root")?;
    }

    match cli.command {
        Command::Scopes => {
            for Scope { scope, covers } in scopes()? {
                println!("{scope}  {covers}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Check { rows: true } => {
            check::Gate::new(check::STEPS, &runner).rows(&mut out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check { rows: false } => {
            let rows = check::Gate::new(check::STEPS, &runner).run(&mut out)?;
            Ok(if rows.iter().all(check::Row::passed) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Pins => {
            let problems = check::Gate::new(check::STEPS, &runner).pins(&mut out)?;
            Ok(if problems {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Setup => {
            spawn::mise_install(&repo_root()).map_err(anyhow::Error::msg)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Package {
            subject,
            tag,
            out: into,
            ember_loader,
            ember_release,
            ember_release_digest,
        } => {
            let request = package::Request {
                subject,
                tag,
                out: into,
                loader: package::LoaderFrom::from_flags(
                    ember_loader,
                    ember_release.zip(ember_release_digest),
                ),
            };
            package::run(&runner, &repo_root(), &request, &mut out)?;
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
        Command::ShellcheckStandIn { args } => Ok(shellcheck::stand_in(&args)),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::{Cli, repo_root, scopes};

    /// Clap rejects an ambiguous or malformed command definition, and a broken
    /// one only shows up at run time otherwise.
    #[test]
    fn the_command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    /// The scope file feeds the commit hook, so a duplicate or an empty entry
    /// would accept a commit nobody meant to allow.
    #[test]
    fn scopes_are_distinct_and_named() {
        let entries = scopes().expect("the scope file is a JSON array of scope objects");
        assert!(!entries.is_empty(), "the scope file names no scope");

        let scopes: Vec<&str> = entries.iter().map(|entry| entry.scope.as_str()).collect();
        let mut seen = scopes.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            scopes.len(),
            "a scope appears twice: {scopes:?}"
        );
        for entry in &entries {
            assert!(!entry.scope.is_empty(), "a scope is empty");
            assert!(
                !entry.covers.is_empty(),
                "{} says nothing about what it covers",
                entry.scope
            );
        }
    }

    /// The root is the workspace root, which is where the gate acts.
    #[test]
    fn the_repository_root_holds_the_workspace_manifest() {
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(repo_root().join("xtask").is_dir());
    }
}
