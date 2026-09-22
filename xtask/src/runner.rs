//! Process execution behind one trait.
//!
//! Every command this crate runs goes through a `Runner`. A test builds its own
//! and spawns nothing, so no test formats the tree, installs a git hook or
//! reaches the network.

use std::fs;
use std::io;
use std::path::PathBuf;
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
    /// Run `command` silently and return what it printed, or `None` when the
    /// program cannot be started or exits non-zero.
    ///
    /// `command[0]` is the program. Standard output and standard error come
    /// back as one string, because a tool is free to print its release to
    /// either. A caller that only wants to know whether the tool answers reads
    /// this as `Some`, so one call covers both questions.
    fn capture(&self, command: &[&str]) -> Option<String>;

    /// Run `command` silently and return what it printed whatever its exit
    /// code, or `None` when the program cannot be started.
    ///
    /// This is for a command whose finding is its expected result, so a
    /// non-zero exit carries the answer rather than the failure.
    fn capture_any(&self, command: &[&str]) -> Option<String>;

    /// Run `command` with `env` set on top of the scrubbed environment, letting
    /// it write straight to this process's terminal.
    ///
    /// `command[0]` is the program.
    ///
    /// # Errors
    ///
    /// Returns the operating system error when the program cannot be started.
    fn run(&self, command: &[&str], env: &[(&str, &str)]) -> io::Result<Exit>;

    /// Read the file at `relative`, resolved from the current directory, which
    /// is the repository root when the gate runs, or `None` when it cannot be
    /// read.
    fn read_file(&self, relative: &str) -> Option<String>;

    /// The path mise installs for `tool`, or `None` when mise resolves none.
    ///
    /// Every pinned tool runs by this path rather than by name. The binary the
    /// gate probed, the binary a step runs and the binary handed to a tool that
    /// would otherwise look one up are then the same file by construction,
    /// rather than three lookups agreeing.
    fn resolve(&self, tool: &str) -> Option<PathBuf>;
}

/// The `Runner` that spawns real child processes.
pub struct Processes;

impl Runner for Processes {
    fn capture(&self, command: &[&str]) -> Option<String> {
        let (program, args) = command.split_first()?;
        let mut child = Command::new(program);
        scrub(&mut child);
        let output = child.args(args).stdin(Stdio::null()).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let mut printed = String::from_utf8_lossy(&output.stdout).into_owned();
        printed.push_str(&String::from_utf8_lossy(&output.stderr));
        Some(printed)
    }

    fn capture_any(&self, command: &[&str]) -> Option<String> {
        let (program, args) = command.split_first()?;
        let mut child = Command::new(program);
        scrub(&mut child);
        let output = child.args(args).stdin(Stdio::null()).output().ok()?;
        let mut printed = String::from_utf8_lossy(&output.stdout).into_owned();
        printed.push_str(&String::from_utf8_lossy(&output.stderr));
        Some(printed)
    }

    fn run(&self, command: &[&str], env: &[(&str, &str)]) -> io::Result<Exit> {
        let Some((program, args)) = command.split_first() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command"));
        };
        let mut child = Command::new(program);
        scrub(&mut child);
        child.envs(env.iter().copied());
        let status = child.args(args).status()?;
        Ok(if status.success() {
            Exit::Ok
        } else {
            Exit::Err
        })
    }

    fn read_file(&self, relative: &str) -> Option<String> {
        fs::read_to_string(relative).ok()
    }

    fn resolve(&self, tool: &str) -> Option<PathBuf> {
        // Standard output alone, rather than the merged streams `capture`
        // returns. `mise which` writes the path to standard output and any
        // warning to standard error, and reading only the stream that carries
        // the answer keeps the path independent of how the two interleave.
        let mut child = Command::new("mise");
        scrub(&mut child);
        let output = child
            .args(["which", tool])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let printed = String::from_utf8_lossy(&output.stdout).into_owned();
        let line = printed.lines().next()?.trim();
        (!line.is_empty()).then(|| PathBuf::from(line))
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
