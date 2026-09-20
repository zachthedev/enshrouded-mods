//! Pointing git at the repository's own hooks.
//!
//! The `prepare` script in `package.json` runs the same `git config` line, so a
//! contributor who runs `bun install` needs nothing from here. This command is
//! the route for a contributor who has no Bun.

use std::io::Write;

use anyhow::{Context, bail};

use crate::runner::{Exit, Runner};

/// The command that points git at the repository's hooks.
const INSTALL: &[&str] = &["git", "config", "core.hooksPath", ".githooks"];

/// The git hooks in `.githooks`, and the runner that installs them.
pub struct Hooks<'a> {
    runner: &'a dyn Runner,
}

impl<'a> Hooks<'a> {
    /// Build a hook installer that executes through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Self {
        Self { runner }
    }

    /// Point `core.hooksPath` at `.githooks` for this clone.
    ///
    /// # Errors
    ///
    /// Returns an error when git cannot be started, when it exits non-zero, or
    /// when `out` cannot be written.
    pub fn install(&self, out: &mut dyn Write) -> anyhow::Result<()> {
        let line = INSTALL.join(" ");
        writeln!(out, "{line}").context("failed to write to the terminal")?;

        match self
            .runner
            .run(INSTALL)
            .with_context(|| format!("failed to start git for: {line}"))?
        {
            Exit::Ok => {
                writeln!(out, "hooks installed, commit-msg and pre-push are live")
                    .context("failed to write to the terminal")?;
                Ok(())
            }
            Exit::Err => bail!("git refused to set core.hooksPath"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;

    use super::{Hooks, INSTALL};
    use crate::runner::{Exit, Runner};

    /// A `Runner` that records commands and never spawns one.
    struct FakeRunner {
        /// What git exits with.
        exit: Exit,
        /// Every command passed to `run`, in order.
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        /// Build a runner whose git exits with `exit`.
        fn new(exit: Exit) -> Self {
            Self {
                exit,
                ran: RefCell::new(Vec::new()),
            }
        }
    }

    impl Runner for FakeRunner {
        fn capture(&self, _command: &[&str]) -> Option<String> {
            Some(String::new())
        }

        fn run(&self, command: &[&str]) -> io::Result<Exit> {
            self.ran
                .borrow_mut()
                .push(command.iter().map(|arg| (*arg).to_string()).collect());
            Ok(self.exit)
        }

        fn read_file(&self, _relative: &str) -> Option<String> {
            None
        }

        fn resolve(&self, _program: &str) -> Option<PathBuf> {
            None
        }
    }

    /// The installer issues one git command and nothing else, so no test can
    /// reach a real repository through it.
    #[test]
    fn install_issues_one_git_config_command() {
        let runner = FakeRunner::new(Exit::Ok);
        let mut out = Vec::new();

        Hooks::new(&runner).install(&mut out).expect("install");

        assert_eq!(
            runner.ran.borrow().as_slice(),
            [["git", "config", "core.hooksPath", ".githooks"]]
        );
    }

    /// A git that refuses is an error, not a silent success.
    #[test]
    fn install_fails_when_git_exits_non_zero() {
        let runner = FakeRunner::new(Exit::Err);
        let mut out = Vec::new();

        let err = Hooks::new(&runner)
            .install(&mut out)
            .expect_err("a non-zero git is an error");

        assert!(err.to_string().contains("core.hooksPath"), "got {err}");
    }

    /// A git that will not start names the command that failed.
    #[test]
    fn install_fails_when_git_will_not_start() {
        struct Broken;
        impl Runner for Broken {
            fn capture(&self, _command: &[&str]) -> Option<String> {
                None
            }
            fn run(&self, _command: &[&str]) -> io::Result<Exit> {
                Err(io::Error::new(io::ErrorKind::NotFound, "no git"))
            }
            fn read_file(&self, _relative: &str) -> Option<String> {
                None
            }
            fn resolve(&self, _program: &str) -> Option<PathBuf> {
                None
            }
        }

        let mut out = Vec::new();
        let err = Hooks::new(&Broken)
            .install(&mut out)
            .expect_err("a missing git is an error");

        assert!(err.to_string().contains("git config"), "got {err}");
    }

    /// The path git is pointed at is the directory the hooks live in.
    #[test]
    fn the_configured_path_is_the_hooks_directory() {
        assert_eq!(INSTALL.last(), Some(&".githooks"));
        assert_eq!(INSTALL.first(), Some(&"git"));
    }
}
