//! The single gate.
//!
//! `CONTRIBUTING.md`, continuous integration and the pre-push hook all call
//! `cargo xtask check` and nothing else, which is what keeps them from drifting
//! apart. The rows run in order and the run stops at the first failure, naming
//! the row. `cargo xtask check --rows` prints the same table.
//!
//! A tool that is not installed is reported by name with the command that
//! installs it, and the gate stops there. It is never skipped quietly, because
//! a gate that reports a pass for a row it did not run is worse than no gate.
//!
//! Every command goes through a [`Runner`], so a test drives the table with a
//! fake and spawns nothing. The one filesystem side effect outside the runner
//! is the canary workflow, written to a temporary directory of its own.

use std::io::{self, Write};

use owo_colors::{OwoColorize, Stream};

use crate::pins;
use crate::runner::{Exit, Runner};

/// Where a row's program comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    /// A tool mise installs, run by the path `mise which` resolves, so the
    /// binary the lockfile records is the binary that runs.
    Mise(&'static str),
    /// A program the toolchain or the package manager puts on `PATH`.
    Path(&'static str),
}

/// One row of the gate.
pub struct Step {
    /// The name the result row carries.
    pub name: &'static str,
    /// What the row checks, as `check --rows` prints it.
    pub covers: &'static str,
    /// Where the program comes from.
    pub program: Program,
    /// The arguments after the program.
    pub args: &'static [&'static str],
    /// What to run when the program is absent.
    pub install: &'static str,
    /// Variables set for the child, on top of the scrubbed environment.
    pub env: &'static [(&'static str, &'static str)],
}

/// What to run when a tool mise owns is absent. One command covers every one
/// of them, because `mise.toml` names them all and mise reads it.
pub const MISE_INSTALL: &str = "mise install --locked";

/// The rustup command that installs the toolchain a cargo row needs.
const RUSTUP: &str = "rustup toolchain install";

/// The rows that build test artifacts share a target directory of their own
/// on Windows, where a test build cannot replace the running `xtask.exe` under
/// `target/debug`.
const TEST_ENV: &[(&str, &str)] = if cfg!(windows) {
    &[("CARGO_TARGET_DIR", "target/check")]
} else {
    &[]
};

/// Every row, in the order they run.
pub const STEPS: &[Step] = &[
    Step {
        name: "fmt",
        covers: "Rust formatting",
        program: Program::Path("cargo"),
        args: &["fmt", "--check"],
        install: "rustup component add rustfmt",
        env: &[],
    },
    Step {
        name: "taplo",
        covers: "TOML formatting, over the files .taplo.toml names",
        program: Program::Mise("taplo"),
        args: &["fmt", "--check"],
        install: MISE_INSTALL,
        env: &[],
    },
    Step {
        name: "clippy",
        covers: "Lints on every target, warnings denied",
        program: Program::Path("cargo"),
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        install: "rustup component add clippy",
        env: &[],
    },
    Step {
        name: "tests",
        covers: "The test suites, under cargo-nextest",
        program: Program::Mise("cargo-nextest"),
        args: &["nextest", "run", "--workspace", "--locked"],
        install: MISE_INSTALL,
        env: TEST_ENV,
    },
    Step {
        name: "doctests",
        covers: "Every documented example, which nextest runs none of",
        program: Program::Path("cargo"),
        args: &["test", "--workspace", "--doc", "--locked"],
        install: RUSTUP,
        env: TEST_ENV,
    },
    Step {
        name: "doc",
        covers: "rustdoc over every crate, warnings denied",
        program: Program::Path("cargo"),
        args: &["doc", "--workspace", "--no-deps", "--locked"],
        install: RUSTUP,
        env: &[("RUSTDOCFLAGS", "-D warnings")],
    },
    Step {
        name: "deny",
        covers: "Licenses, bans and sources, per deny.toml",
        program: Program::Mise("cargo-deny"),
        args: &["--locked", "check", "licenses", "bans", "sources"],
        install: MISE_INSTALL,
        env: &[],
    },
    Step {
        name: "machete",
        covers: "Dependencies a crate declares and never uses; .ignore keeps vendor/ out",
        program: Program::Mise("cargo-machete"),
        // The one path is the workspace root. With no argument at all, the binary
        // indexes an argument that is not there and panics.
        args: &["."],
        install: MISE_INSTALL,
        env: &[],
    },
    Step {
        name: "prettier",
        covers: "Markup, JavaScript and TypeScript formatting",
        program: Program::Path("bunx"),
        args: &["--no-install", "--bun", "prettier", "--check", "."],
        install: "bun install",
        env: &[],
    },
    Step {
        name: "actionlint",
        covers: "Workflow syntax, runner labels, expressions, and run: blocks through ShellCheck",
        program: Program::Mise("actionlint"),
        args: &["-pyflakes="],
        install: MISE_INSTALL,
        env: &[],
    },
    Step {
        name: "zizmor",
        covers: "Workflow pinning, credentials, permissions and injection, over the named paths",
        program: Program::Mise("zizmor"),
        // The paths are named, so the audit never reaches Ember's checkout
        // under vendor/, whose workflows Ember's own gate covers.
        args: &[
            "--no-progress",
            "--strict-collection",
            "--config",
            ".github/zizmor.yml",
            ".github/workflows",
            ".github/dependabot.yml",
        ],
        install: MISE_INSTALL,
        env: &[],
    },
];

/// The name the row for the pin rules carries.
pub const PINS_STEP: &str = "pins";

/// A workflow with one unquoted expansion, which `ShellCheck` reports as
/// `SC2086`.
const CANARY: &str = "on: push\njobs:\n  canary:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo $GITHUB_REF\n";

/// Whether the canary run reported the finding it was written to trigger.
fn canary_passed(heard: &str) -> bool {
    heard.contains("SC2086")
}

/// How a row ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The row ran and exited zero, with the note its row carries.
    Passed(String),
    /// The row ran and exited non-zero.
    Failed,
    /// The row could not start, with the sentence saying why.
    Unrun(String),
}

/// One row of the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The step name.
    pub step: &'static str,
    /// How the step ended.
    pub outcome: Outcome,
}

impl Row {
    /// Whether the step passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.outcome, Outcome::Passed(_))
    }
}

/// The command a row runs: its argument vector, its environment, and the note
/// its row carries when it passes.
struct Prepared {
    command: Vec<String>,
    env: Vec<(String, String)>,
    note: String,
}

/// The gate's rows and the runner that executes them.
pub struct Gate<'a> {
    steps: &'a [Step],
    runner: &'a dyn Runner,
}

impl<'a> Gate<'a> {
    /// A gate over `steps`, executed by `runner`.
    #[must_use]
    pub fn new(steps: &'a [Step], runner: &'a dyn Runner) -> Self {
        Self { steps, runner }
    }

    /// Print the row table: the pin rules, then every row and what it covers.
    ///
    /// # Errors
    ///
    /// Returns the error `out` raised.
    pub fn rows(&self, out: &mut dyn Write) -> io::Result<()> {
        let width = self
            .steps
            .iter()
            .map(|step| step.name.len())
            .max()
            .unwrap_or(0)
            .max(PINS_STEP.len());
        writeln!(
            out,
            "  {PINS_STEP:width$}  mise.toml and mise.lock against the rules in pins.rs, before any tool runs"
        )?;
        for step in self.steps {
            writeln!(out, "  {:width$}  {}", step.name, step.covers)?;
        }
        Ok(())
    }

    /// Run the pin rules alone and print each problem.
    ///
    /// Returns whether any problem was found.
    ///
    /// # Errors
    ///
    /// Returns the error `out` raised.
    pub fn pins(&self, out: &mut dyn Write) -> io::Result<bool> {
        let problems = self.pin_problems();
        for problem in &problems {
            writeln!(out, "  {problem}")?;
        }
        Ok(!problems.is_empty())
    }

    /// Every way the pin files fall short, with an unreadable file reported as
    /// a problem of its own.
    fn pin_problems(&self) -> Vec<String> {
        let read = |path: &str| {
            self.runner
                .read_file(path)
                .ok_or_else(|| format!("{path} cannot be read"))
        };
        match (read(pins::PINS), read(pins::LOCK)) {
            (Ok(pin_text), Ok(lock)) => pins::problems(&pin_text, &lock),
            (first, second) => [first, second]
                .into_iter()
                .filter_map(Result::err)
                .collect(),
        }
    }

    /// Run every row in order, stopping at the first that does not pass.
    ///
    /// # Errors
    ///
    /// Returns the error `out` raised.
    pub fn run(&self, out: &mut dyn Write) -> io::Result<Vec<Row>> {
        writeln!(out, "check")?;
        let mut rows = Vec::new();

        // The pin rules run before any tool. A lockfile entry carrying a url
        // and no checksum installs whatever that url serves, so a rule that ran
        // later would report a finding about a binary that already executed.
        let problems = self.pin_problems();
        if problems.is_empty() {
            rows.push(Row {
                step: PINS_STEP,
                outcome: Outcome::Passed(String::new()),
            });
        } else {
            for problem in &problems {
                writeln!(out, "  {problem}")?;
            }
            // The relock is the remedy only when a problem is about the lockfile;
            // an unreadable or malformed mise.toml needs an edit, not a relock.
            let remedy = if problems.iter().any(|problem| problem.contains(pins::LOCK)) {
                format!("rewrite the lockfile with: {}", pins::RELOCK)
            } else {
                format!("fix {}", pins::PINS)
            };
            rows.push(Row {
                step: PINS_STEP,
                outcome: Outcome::Unrun(remedy),
            });
            self.summarize(out, &rows)?;
            return Ok(rows);
        }

        for step in self.steps {
            let outcome = match self.prepare(step) {
                Ok(prepared) => self.invoke(out, &prepared)?,
                Err(problem) => Outcome::Unrun(problem),
            };
            let stop = !matches!(outcome, Outcome::Passed(_));
            rows.push(Row {
                step: step.name,
                outcome,
            });
            if stop {
                break;
            }
        }
        self.summarize(out, &rows)?;
        Ok(rows)
    }

    /// The command a row runs, or the sentence its row carries when it cannot.
    fn prepare(&self, step: &Step) -> Result<Prepared, String> {
        let program = match step.program {
            Program::Mise(tool) => self
                .runner
                .resolve(tool)
                .ok_or_else(|| format!("mise resolves no {tool}: {}", step.install))?
                .display()
                .to_string(),
            Program::Path(name) => name.to_string(),
        };
        let mut command = vec![program];
        command.extend(step.args.iter().map(|arg| (*arg).to_string()));
        let mut env: Vec<(String, String)> = step
            .env
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        let mut note = String::new();
        match step.name {
            "actionlint" => {
                // actionlint exits zero and prints nothing when its analyzer is
                // absent or will not execute, and no flag changes that. The path
                // mise resolved goes on the command line, and a workflow with one
                // known finding has to come back with that finding before the
                // real run is trusted.
                let analyzer = self.runner.resolve("shellcheck").ok_or_else(|| {
                    format!(
                        "mise resolves no shellcheck, and actionlint would skip the analysis in silence: {MISE_INSTALL}"
                    )
                })?;
                command.push(format!("-shellcheck={}", analyzer.display()));
                let heard = self.canary(&command)?;
                if !canary_passed(&heard) {
                    return Err(
                        "actionlint ran without ShellCheck: the canary workflow came back with no SC2086"
                            .to_string(),
                    );
                }
            }
            "zizmor" => {
                // Online when the host has a GitHub login, so the audits that
                // read the API run; offline otherwise, so a laptop with no token
                // still gets the rest.
                if let Some(token) = self.gh_token() {
                    env.push(("GH_TOKEN".to_string(), token));
                    note.push_str("online");
                } else {
                    command.push("--offline".to_string());
                    note.push_str("offline");
                }
            }
            _ => {}
        }
        Ok(Prepared { command, env, note })
    }

    /// Run the actionlint command over the canary workflow and return what it
    /// printed. An exit code alone says nothing, because a finding is the
    /// expected result.
    fn canary(&self, command: &[String]) -> Result<String, String> {
        let dir =
            tempfile::tempdir().map_err(|err| format!("creating the canary directory: {err}"))?;
        let workflow = dir.path().join("canary.yml");
        std::fs::write(&workflow, CANARY)
            .map_err(|err| format!("writing the canary workflow: {err}"))?;
        let path = workflow.display().to_string();
        let mut argv: Vec<&str> = command.iter().map(String::as_str).collect();
        argv.push(&path);
        self.runner
            .capture_any(&argv)
            .ok_or_else(|| format!("{} could not start for the canary run", command[0]))
    }

    /// The token `gh` holds for github.com, or `None` when nobody is logged in.
    ///
    /// The first line alone: `capture` merges the streams, and the token is
    /// the one line standard output carries.
    fn gh_token(&self) -> Option<String> {
        let printed = self.runner.capture(&["gh", "auth", "token"])?;
        let token = printed.lines().next()?.trim().to_string();
        (!token.is_empty()).then_some(token)
    }

    /// Run one prepared command and judge it.
    fn invoke(&self, out: &mut dyn Write, prepared: &Prepared) -> io::Result<Outcome> {
        let line = prepared.command.join(" ");
        writeln!(
            out,
            "\n{}",
            line.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
        )?;
        let argv: Vec<&str> = prepared.command.iter().map(String::as_str).collect();
        let pairs: Vec<(&str, &str)> = prepared
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        Ok(match self.runner.run(&argv, &pairs) {
            Ok(Exit::Ok) => Outcome::Passed(prepared.note.clone()),
            Ok(Exit::Err) => Outcome::Failed,
            Err(err) => Outcome::Unrun(format!("{} could not start: {err}", prepared.command[0])),
        })
    }

    /// Print the summary table.
    #[expect(
        clippy::unused_self,
        reason = "the summary belongs to the gate that produced the rows"
    )]
    fn summarize(&self, out: &mut dyn Write, rows: &[Row]) -> io::Result<()> {
        let width = rows.iter().map(|row| row.step.len()).max().unwrap_or(0);
        let ok = "\u{2713}"
            .if_supports_color(Stream::Stdout, OwoColorize::green)
            .to_string();
        let bad = "\u{2717}"
            .if_supports_color(Stream::Stdout, OwoColorize::red)
            .to_string();
        writeln!(out)?;
        for row in rows {
            let (mark, text) = match &row.outcome {
                Outcome::Passed(note) => (&ok, note.clone()),
                Outcome::Failed => (&bad, "failed".to_string()),
                Outcome::Unrun(why) => (&bad, format!("did not run: {why}")),
            };
            writeln!(out, "  {mark} {:width$}  {text}", row.step)?;
        }
        let rule = "\u{2500}".repeat(width + 12);
        writeln!(
            out,
            "  {}",
            rule.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
        )?;
        match rows.iter().find(|row| !row.passed()) {
            Some(row) => {
                let text = format!("the gate failed at {}", row.step);
                writeln!(
                    out,
                    "  {}",
                    text.if_supports_color(Stream::Stdout, OwoColorize::red)
                )
            }
            None => writeln!(out, "  {} steps passed", rows.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fmt::Write as _;
    use std::io;
    use std::path::PathBuf;

    use super::{Gate, MISE_INSTALL, Outcome, Program, Row, STEPS, canary_passed};
    use crate::pins;
    use crate::runner::{Exit, Runner};

    /// A runner that spawns nothing: every mise tool resolves under one fake
    /// prefix, every command passes, and the canary answers with its finding.
    struct FakeRunner {
        /// Tools `resolve` answers `None` for.
        unresolvable: Vec<&'static str>,
        /// Programs whose command exits non-zero, by basename.
        failing: Vec<&'static str>,
        /// Whether `gh auth token` answers.
        logged_in: bool,
        /// Whether the pin files can be read.
        pins_readable: bool,
        /// Every command passed to `run`, in order.
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn all_installed() -> Self {
            Self {
                unresolvable: Vec::new(),
                failing: Vec::new(),
                logged_in: true,
                pins_readable: true,
                ran: RefCell::new(Vec::new()),
            }
        }

        fn unresolvable(mut self, tool: &'static str) -> Self {
            self.unresolvable.push(tool);
            self
        }

        fn failing(mut self, program: &'static str) -> Self {
            self.failing.push(program);
            self
        }

        fn logged_out(mut self) -> Self {
            self.logged_in = false;
            self
        }

        fn unpinned(mut self) -> Self {
            self.pins_readable = false;
            self
        }

        fn ran(&self) -> Vec<Vec<String>> {
            self.ran.borrow().clone()
        }
    }

    /// The pin files the fake reads, built from `pins::TOOLS` so the pin rules
    /// pass and the cases exercise the rows rather than the pin round.
    fn sound_pins() -> (String, String) {
        let digest = "a".repeat(64);
        let mut pinned = String::from("[tools]\n");
        let mut lock = String::new();
        for tool in pins::TOOLS {
            writeln!(pinned, "\"{}\" = \"1.2.3\"", tool.key).expect("write to a String");
            writeln!(
                lock,
                "[[tools.\"{}\"]]\nversion = \"1.2.3\"\nbackend = \"{}\"",
                tool.key,
                tool.coordinate()
            )
            .expect("write to a String");
            for platform in ["linux-x64", "windows-x64"] {
                writeln!(
                    lock,
                    "[tools.\"{}\".\"platforms.{platform}\"]\nchecksum = \"sha256:{digest}\"\nurl = \"https://github.com{}{}/{}\"\nurl_api = \"https://api.github.com/repos/{}/{}/releases/assets/1\"",
                    tool.key,
                    tool.release_prefix(),
                    tool.tag("1.2.3"),
                    tool.binary,
                    tool.owner,
                    tool.repository
                )
                .expect("write to a String");
                if let Some(provenance) = tool.provenance {
                    writeln!(lock, "provenance = \"{provenance}\"").expect("write to a String");
                }
            }
        }
        pinned.push_str("\n[settings]\nlockfile_platforms = [\"linux-x64\", \"windows-x64\"]\n");
        (pinned, lock)
    }

    impl Runner for FakeRunner {
        fn capture(&self, command: &[&str]) -> Option<String> {
            (command == ["gh", "auth", "token"] && self.logged_in).then(|| "ghp_fake\n".to_string())
        }

        fn capture_any(&self, command: &[&str]) -> Option<String> {
            command[0].ends_with("actionlint").then(|| {
                "canary.yml:6:9: shellcheck reported issue in this script: SC2086:info:1:6"
                    .to_string()
            })
        }

        fn run(&self, command: &[&str], _env: &[(&str, &str)]) -> io::Result<Exit> {
            self.ran
                .borrow_mut()
                .push(command.iter().map(|arg| (*arg).to_string()).collect());
            let program = command[0].rsplit(['/', '\\']).next().unwrap_or(command[0]);
            Ok(if self.failing.contains(&program) {
                Exit::Err
            } else {
                Exit::Ok
            })
        }

        fn read_file(&self, relative: &str) -> Option<String> {
            if !self.pins_readable {
                return None;
            }
            let (pinned, lock) = sound_pins();
            match relative {
                pins::PINS => Some(pinned),
                pins::LOCK => Some(lock),
                _ => None,
            }
        }

        fn resolve(&self, tool: &str) -> Option<PathBuf> {
            (!self.unresolvable.contains(&tool)).then(|| PathBuf::from(format!("/fake/bin/{tool}")))
        }
    }

    fn gate(runner: &FakeRunner) -> (Vec<Row>, String) {
        let mut out = Vec::new();
        let rows = Gate::new(STEPS, runner)
            .run(&mut out)
            .expect("write to a Vec");
        (rows, String::from_utf8(out).expect("utf-8"))
    }

    /// The order is the contract `--rows` prints and CONTRIBUTING.md names, so
    /// a row added out of place or twice is caught here.
    #[test]
    fn the_gate_runs_its_rows_in_the_documented_order() {
        let names: Vec<&str> = STEPS.iter().map(|step| step.name).collect();
        assert_eq!(
            names,
            [
                "fmt",
                "taplo",
                "clippy",
                "tests",
                "doctests",
                "doc",
                "deny",
                "machete",
                "prettier",
                "actionlint",
                "zizmor"
            ]
        );
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);
        let ran: Vec<&str> = rows.iter().map(|row| row.step).collect();
        assert_eq!(ran[0], "pins");
        assert_eq!(&ran[1..], names.as_slice());
        assert!(rows.iter().all(Row::passed), "{rows:?}");
        for step in STEPS {
            assert!(
                !step.covers.is_empty(),
                "{} says nothing about what it covers",
                step.name
            );
        }
    }

    /// A cargo command that resolves dependencies runs `--locked`, so an edit to
    /// a manifest with no relock stops the gate rather than rewriting Cargo.lock.
    #[test]
    fn every_cargo_row_that_resolves_dependencies_is_locked() {
        for step in STEPS {
            let resolves = match step.program {
                Program::Path("cargo") => step.args[0] != "fmt",
                Program::Mise("cargo-nextest" | "cargo-deny") => true,
                _ => false,
            };
            if resolves {
                assert!(
                    step.args.contains(&"--locked"),
                    "{} runs without --locked",
                    step.name
                );
            }
        }
    }

    /// Every mise tool runs by the path the runner resolved, never by name.
    #[test]
    fn every_mise_row_runs_the_path_mise_resolved() {
        let runner = FakeRunner::all_installed();
        gate(&runner);
        for (step, command) in STEPS.iter().zip(runner.ran()) {
            match step.program {
                Program::Mise(tool) => {
                    assert_eq!(command[0], format!("/fake/bin/{tool}"), "{}", step.name);
                }
                Program::Path(name) => assert_eq!(command[0], name, "{}", step.name),
            }
        }
    }

    /// A tool mise resolves nowhere stops the gate at its row, names the install
    /// command, and runs nothing after it.
    #[test]
    fn an_unresolvable_tool_stops_the_gate_and_names_the_install() {
        let runner = FakeRunner::all_installed().unresolvable("cargo-deny");
        let (rows, text) = gate(&runner);
        let last = rows.last().expect("one row");
        assert_eq!(last.step, "deny");
        assert!(
            matches!(&last.outcome, Outcome::Unrun(why) if why.contains(MISE_INSTALL)),
            "{last:?}"
        );
        assert_eq!(
            runner.ran().len(),
            6,
            "rows after deny ran: {:?}",
            runner.ran()
        );
        assert!(text.contains("the gate failed at deny"), "{text}");
    }

    /// A row that exits non-zero stops the gate there.
    #[test]
    fn a_failing_row_stops_the_gate() {
        let runner = FakeRunner::all_installed().failing("cargo-nextest");
        let (rows, _) = gate(&runner);
        let last = rows.last().expect("one row");
        assert_eq!(last.step, "tests");
        assert_eq!(last.outcome, Outcome::Failed);
    }

    /// actionlint takes the analyzer by resolved path and refuses to run when
    /// mise resolves none, because it would skip the analysis in silence.
    #[test]
    fn actionlint_takes_shellcheck_by_resolved_path_or_refuses() {
        let runner = FakeRunner::all_installed();
        gate(&runner);
        let actionlint = runner
            .ran()
            .into_iter()
            .find(|command| command[0].ends_with("actionlint"))
            .expect("actionlint ran");
        assert!(
            actionlint.contains(&"-shellcheck=/fake/bin/shellcheck".to_string()),
            "{actionlint:?}"
        );
        assert!(
            actionlint.contains(&"-pyflakes=".to_string()),
            "{actionlint:?}"
        );

        let runner = FakeRunner::all_installed().unresolvable("shellcheck");
        let (rows, _) = gate(&runner);
        let last = rows.last().expect("one row");
        assert_eq!(last.step, "actionlint");
        assert!(
            matches!(&last.outcome, Outcome::Unrun(why) if why.contains("shellcheck")),
            "{last:?}"
        );
    }

    /// The canary is judged on the finding it was written to produce, so an
    /// analyzer that prints anything else, or nothing, fails it.
    #[test]
    fn the_canary_passes_only_on_its_own_finding() {
        assert!(canary_passed(
            "canary.yml:6:9: shellcheck reported issue in this script: SC2086:info:1:6"
        ));
        assert!(!canary_passed(""));
        assert!(!canary_passed(
            "canary.yml:6:9: shellcheck reported issue in this script: SC2046:warning"
        ));
    }

    /// zizmor runs online with a token and offline without one, and the row
    /// says which.
    #[test]
    fn zizmor_runs_online_with_a_login_and_offline_without() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);
        let zizmor = rows
            .iter()
            .find(|row| row.step == "zizmor")
            .expect("zizmor ran");
        assert_eq!(zizmor.outcome, Outcome::Passed("online".to_string()));
        let last = runner.ran().pop().expect("a command");
        assert!(!last.contains(&"--offline".to_string()), "{last:?}");

        let runner = FakeRunner::all_installed().logged_out();
        let (rows, _) = gate(&runner);
        let zizmor = rows
            .iter()
            .find(|row| row.step == "zizmor")
            .expect("zizmor ran");
        assert_eq!(zizmor.outcome, Outcome::Passed("offline".to_string()));
        let last = runner.ran().pop().expect("a command");
        assert!(last.contains(&"--offline".to_string()), "{last:?}");
    }

    /// A pin file nothing can read stops the gate at its first row, before any
    /// tool runs.
    #[test]
    fn an_unreadable_pin_stops_the_gate() {
        let runner = FakeRunner::all_installed().unpinned();
        let (rows, text) = gate(&runner);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].step, "pins");
        assert!(!rows[0].passed());
        assert!(runner.ran().is_empty(), "a tool ran before the pin rules");
        assert!(
            text.contains(&format!("{} cannot be read", pins::PINS)),
            "{text}"
        );
    }

    /// `--rows` prints the pin rules and every row, one line each, and runs
    /// nothing.
    #[test]
    fn the_row_table_prints_one_line_per_row() {
        let runner = FakeRunner::all_installed();
        let mut out = Vec::new();
        Gate::new(STEPS, &runner)
            .rows(&mut out)
            .expect("write to a Vec");
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(text.lines().count(), STEPS.len() + 1);
        assert!(text.lines().next().expect("a line").contains("pins"));
        assert!(runner.ran().is_empty(), "--rows ran a tool");
    }
}
