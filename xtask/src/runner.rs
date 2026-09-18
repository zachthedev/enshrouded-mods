//! Process execution behind one trait.
//!
//! Every command this crate runs goes through a `Runner`. A test builds its own
//! and spawns nothing, so no test formats the tree, installs a git hook or
//! reaches the network.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Environment variables that describe this crate rather than the workspace.
///
/// Cargo sets these for a binary it launches, and a child that inherits them
/// reads them as facts about itself. `cargo-machete` reads `CARGO_PKG_NAME` to
/// decide whether its first argument is its own subcommand name, so a child
/// that inherits it parses `machete` as a directory to scan.
const CRATE_ENV: &[&str] = &[
    "CARGO_BIN_NAME",
    "CARGO_CRATE_NAME",
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_NAME",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "CARGO_PRIMARY_PACKAGE",
];

/// How one command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The command exited zero.
    Ok,
    /// The command exited non-zero, or a signal killed it.
    Err,
}

/// Runs commands on behalf of an xtask subcommand.
pub trait Runner {
    /// Report whether the tool behind `command` answers.
    ///
    /// `command[0]` is the program. The command's output is discarded, so a
    /// probe is silent whether or not the tool is there.
    fn probe(&self, command: &[&str]) -> bool;

    /// Run `command`, letting it write straight to this process's terminal.
    ///
    /// `command[0]` is the program.
    ///
    /// # Errors
    ///
    /// Returns the operating system error when the program cannot be started.
    fn run(&self, command: &[&str]) -> io::Result<Exit>;

    /// Report whether a file exists at `relative`, resolved from the current
    /// directory, which is the repository root when the gate runs.
    fn file_exists(&self, relative: &str) -> bool;
}

/// The `Runner` that spawns real child processes.
pub struct Processes;

impl Runner for Processes {
    fn probe(&self, command: &[&str]) -> bool {
        let Some((program, args)) = command.split_first() else {
            return false;
        };
        let mut child = Command::new(program);
        scrub(&mut child);
        child
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn run(&self, command: &[&str]) -> io::Result<Exit> {
        let Some((program, args)) = command.split_first() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command"));
        };
        let mut child = Command::new(program);
        scrub(&mut child);
        let status = child.args(args).status()?;
        Ok(if status.success() {
            Exit::Ok
        } else {
            Exit::Err
        })
    }

    fn file_exists(&self, relative: &str) -> bool {
        Path::new(relative).is_file()
    }
}

/// Drop this crate's own build metadata from a child's environment.
fn scrub(command: &mut Command) {
    for name in CRATE_ENV {
        command.env_remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::CRATE_ENV;

    /// `cargo-machete` reads `CARGO_PKG_NAME`, and inheriting it makes the tool
    /// scan a directory named after its own subcommand.
    #[test]
    fn the_scrubbed_set_covers_the_variable_cargo_machete_reads() {
        assert!(CRATE_ENV.contains(&"CARGO_PKG_NAME"));
    }

    /// Every scrubbed name describes this crate, never the workspace under
    /// check, so nothing here removes a variable a tool needs.
    #[test]
    fn every_scrubbed_name_is_cargo_build_metadata() {
        for name in CRATE_ENV {
            assert!(name.starts_with("CARGO_"), "{name} is not cargo metadata");
        }
    }

    /// A repeated name would hide a missing one in a hand-read of the list.
    #[test]
    fn the_scrubbed_set_has_no_repeats() {
        let mut seen = CRATE_ENV.to_vec();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();

        assert_eq!(seen.len(), count, "two entries share a name");
    }
}
