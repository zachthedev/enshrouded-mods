//! Reaching Ember's xtask through the submodule.
//!
//! The development server and the schema extractor drive Keen's binary, which
//! Ember owns. This repository forwards those commands and passes `--root`, so
//! the fetched build, the fixtures and the logs stay under its own `.cache`.
//! Arguments pass through untouched, so Ember's flags are the only ones.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::runner::{Exit, Runner};

/// Ember's xtask manifest, relative to this repository's root.
const EMBER_XTASK: &str = "vendor/enshrouded-ember/xtask/Cargo.toml";

/// Ember's xtask, reached through the submodule.
pub struct Delegate<'a> {
    runner: &'a dyn Runner,
    root: PathBuf,
}

impl<'a> Delegate<'a> {
    /// Build a delegate rooted at `root` that executes through `runner`.
    pub fn new(runner: &'a dyn Runner, root: PathBuf) -> Self {
        Self { runner, root }
    }

    /// Where Ember's xtask manifest sits for this repository.
    pub fn manifest(&self) -> PathBuf {
        self.root.join(EMBER_XTASK)
    }

    /// The command that runs `group` with `args` through Ember's xtask.
    pub fn command(&self, group: &str, args: &[String]) -> Vec<String> {
        let mut command = vec![
            "cargo".to_string(),
            "run".to_string(),
            "--quiet".to_string(),
            "--manifest-path".to_string(),
            display(&self.manifest()),
            "--".to_string(),
            group.to_string(),
        ];
        command.extend(args.iter().cloned());
        command.push("--root".to_string());
        command.push(display(&self.root));
        command
    }

    /// Forward `group` and `args` to Ember's xtask.
    ///
    /// # Errors
    ///
    /// Returns an error when the submodule is not checked out, when cargo
    /// cannot be started, or when Ember's xtask exits non-zero.
    pub fn run(&self, group: &str, args: &[String], out: &mut dyn Write) -> anyhow::Result<()> {
        let manifest = self.manifest();
        if !manifest.is_file() {
            bail!(
                "Ember is not checked out at {}, run: git submodule update --init",
                display(&manifest)
            );
        }

        let command = self.command(group, args);
        let borrowed: Vec<&str> = command.iter().map(String::as_str).collect();
        let line = command.join(" ");
        writeln!(out, "{line}").context("failed to write to the terminal")?;

        match self
            .runner
            .run(&borrowed, &[])
            .with_context(|| format!("failed to start: {line}"))?
        {
            Exit::Ok => Ok(()),
            Exit::Err => bail!("ember xtask {group} failed"),
        }
    }
}

/// A path as one command argument.
///
/// A path that is not valid Unicode cannot reach a child process through this
/// crate's runner, so it is rendered lossily and the child reports it.
fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;

    use super::{Delegate, EMBER_XTASK};
    use crate::runner::{Captured, Exit, Runner};

    /// A `Runner` that records commands and never spawns one.
    struct FakeRunner {
        ran: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new() -> Self {
            Self {
                ran: RefCell::new(Vec::new()),
            }
        }
    }

    impl Runner for FakeRunner {
        fn capture_within(
            &self,
            _command: &[&str],
            _env: &[(&str, &str)],
            _deadline: std::time::Duration,
        ) -> Option<String> {
            None
        }

        fn capture_any(&self, _command: &[&str]) -> Option<String> {
            None
        }

        fn run(&self, command: &[&str], _env: &[(&str, &str)]) -> io::Result<Exit> {
            self.ran.borrow_mut().push(command.join(" "));
            Ok(Exit::Ok)
        }

        fn output(
            &self,
            _command: &[&str],
            _env: &[(&str, &str)],
            _input: Option<&str>,
        ) -> io::Result<Captured> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no row runs here",
            ))
        }

        fn read_file(&self, _relative: &str) -> Option<String> {
            None
        }

        fn resolve(&self, program: &str) -> Result<PathBuf, String> {
            Err(format!("mise resolves no {program}"))
        }
    }

    /// Build a delegate over a root that holds no files.
    fn delegate(runner: &FakeRunner) -> Delegate<'_> {
        Delegate::new(runner, PathBuf::from("Z:/checkout/enshrouded-mods"))
    }

    /// Ember's xtask is reached by its manifest path, never by a directory
    /// change, because a directory change would move every later command.
    #[test]
    fn the_command_names_embers_manifest() {
        let runner = FakeRunner::new();
        let command = delegate(&runner).command("server", &[]);

        let manifest = command
            .windows(2)
            .find(|pair| pair[0] == "--manifest-path")
            .map(|pair| pair[1].clone())
            .expect("the command names a manifest");

        assert!(manifest.ends_with(EMBER_XTASK), "got {manifest}");
        assert_eq!(command[0], "cargo");
        assert!(!command.contains(&"cd".to_string()));
    }

    /// Every delegated command carries the root, so Ember writes under this
    /// repository and nowhere else.
    #[test]
    fn every_delegated_command_passes_the_root() {
        let runner = FakeRunner::new();
        let delegate = delegate(&runner);

        let groups = ["server", "schema"];
        let argument_sets: [&[String]; 3] = [
            &[],
            &["fetch".to_string()],
            &[
                "run".to_string(),
                "--fixture".to_string(),
                "empty".to_string(),
            ],
        ];

        for group in groups {
            for args in argument_sets {
                let command = delegate.command(group, args);
                let root = command
                    .windows(2)
                    .find(|pair| pair[0] == "--root")
                    .map(|pair| pair[1].clone());
                assert_eq!(
                    root,
                    Some("Z:/checkout/enshrouded-mods".to_string()),
                    "{group} {args:?}"
                );
            }
        }
    }

    /// The group leads the forwarded arguments and the caller's arguments
    /// follow it in order.
    #[test]
    fn arguments_pass_through_in_order() {
        let runner = FakeRunner::new();
        let args = vec![
            "run".to_string(),
            "--fixture".to_string(),
            "empty".to_string(),
        ];
        let command = delegate(&runner).command("server", &args);

        let separator = command
            .iter()
            .position(|arg| arg == "--")
            .expect("cargo arguments end at the separator");
        assert_eq!(command[separator + 1], "server");
        assert_eq!(&command[separator + 2..separator + 5], args.as_slice());
    }

    /// A clone without the submodule says what to run, rather than failing
    /// inside cargo.
    #[test]
    fn a_missing_submodule_names_the_command_that_fixes_it() {
        let runner = FakeRunner::new();
        let delegate = Delegate::new(&runner, PathBuf::from("Z:/nothing/here"));
        let mut out = Vec::new();

        let err = delegate
            .run("server", &["fetch".to_string()], &mut out)
            .expect_err("a missing submodule is an error");

        assert!(err.to_string().contains("git submodule update --init"));
        assert!(runner.ran.borrow().is_empty(), "it ran something anyway");
    }
}
