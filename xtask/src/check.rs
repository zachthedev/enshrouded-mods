//! The gate every change passes, behind one command.
//!
//! `CONTRIBUTING.md`, the `pre-push` hook and continuous integration all call
//! `cargo xtask check`, so none of them can drift from the others. Steps run in
//! the order they are declared and the run stops at the first one that does not
//! pass.

use std::io::{self, Write};

use owo_colors::{OwoColorize, Stream};

use crate::runner::{Exit, Runner};

// ///////////////////////////////////////////////
// The step table
// ///////////////////////////////////////////////

/// How the gate tells whether a tool is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A command that exits zero when the tool answers. The first element is
    /// the program.
    Command(&'static [&'static str]),
    /// A file that exists when the tool is installed, relative to the
    /// repository root. This is the shape for a tool that would fetch itself
    /// from a registry when asked to run.
    File(&'static str),
}

/// One command, and the tool that has to be installed to run it.
pub struct Run {
    /// The tool's name, printed when it is missing.
    pub tool: &'static str,
    /// How to tell the tool is installed.
    pub probe: Probe,
    /// The command itself. `command[0]` is the program.
    pub command: &'static [&'static str],
    /// The command that installs the tool, printed when it is missing.
    pub install: &'static str,
}

/// One step of the gate.
pub struct Step {
    /// The step's name, printed in the summary and when the step fails.
    pub name: &'static str,
    /// What the step runs when its tool is installed.
    pub primary: Run,
    /// What the step runs in place of `primary` when that tool is absent.
    pub fallback: Option<Run>,
}

/// The gate, in order.
pub const STEPS: &[Step] = &[
    Step {
        name: "fmt",
        primary: Run {
            tool: "rustfmt",
            probe: Probe::Command(&["cargo", "fmt", "--version"]),
            command: &["cargo", "fmt", "--check"],
            install: "rustup component add rustfmt",
        },
        fallback: None,
    },
    Step {
        name: "taplo",
        primary: Run {
            tool: "taplo-cli",
            probe: Probe::Command(&["taplo", "--version"]),
            // The files and the exclusions are in .taplo.toml, so the same set
            // is formatted whether the gate or an editor runs the tool.
            command: &["taplo", "fmt", "--check"],
            install: "cargo install taplo-cli --locked",
        },
        fallback: None,
    },
    Step {
        name: "clippy",
        primary: Run {
            tool: "clippy",
            probe: Probe::Command(&["cargo", "clippy", "--version"]),
            command: &[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            install: "rustup component add clippy",
        },
        fallback: None,
    },
    Step {
        name: "tests",
        primary: Run {
            tool: "cargo-nextest",
            probe: Probe::Command(&["cargo", "nextest", "--version"]),
            command: &["cargo", "nextest", "run", "--workspace"],
            install: "cargo install cargo-nextest --locked",
        },
        fallback: Some(Run {
            tool: "cargo test",
            probe: Probe::Command(&["cargo", "--version"]),
            command: &["cargo", "test", "--workspace"],
            install: "rustup toolchain install stable",
        }),
    },
    Step {
        name: "doctests",
        primary: Run {
            tool: "cargo test",
            probe: Probe::Command(&["cargo", "--version"]),
            // cargo-nextest runs no doctests, so the tests step above leaves
            // every documented example unbuilt. This step runs unconditionally,
            // which costs a second run of them on the rare host where the tests
            // step fell back to `cargo test`. What the gate covers then does not
            // depend on which test runner is installed.
            command: &["cargo", "test", "--workspace", "--doc"],
            install: "rustup toolchain install stable",
        },
        fallback: None,
    },
    Step {
        name: "deny",
        primary: Run {
            tool: "cargo-deny",
            probe: Probe::Command(&["cargo", "deny", "--version"]),
            command: &["cargo", "deny", "check"],
            install: "cargo install cargo-deny --locked",
        },
        fallback: None,
    },
    Step {
        name: "machete",
        primary: Run {
            tool: "cargo-machete",
            probe: Probe::Command(&["cargo", "machete", "--version"]),
            // Named directories, because vendor/ holds Ember's own workspace
            // and its manifests are checked by Ember's own gate.
            command: &["cargo", "machete", "crates", "mods", "xtask"],
            install: "cargo install cargo-machete --locked",
        },
        fallback: None,
    },
    Step {
        name: "audit",
        primary: Run {
            tool: "cargo-audit",
            probe: Probe::Command(&["cargo", "audit", "--version"]),
            command: &["cargo", "audit"],
            install: "cargo install cargo-audit --locked",
        },
        fallback: None,
    },
    Step {
        name: "prettier",
        primary: Run {
            tool: "prettier",
            // The installed package, not a binary on PATH: bunx fetches from
            // npm when a package is absent locally, which bypasses the
            // lockfile, so the probe has to see the pinned copy itself.
            probe: Probe::File("node_modules/prettier/package.json"),
            // A PostToolUse hook formats .js on every edit, so the glob covers
            // every extension prettier owns here rather than the markup alone.
            // .ts is in the list before the first one lands.
            command: &[
                "bunx",
                "--no-install",
                "--bun",
                "prettier",
                "--check",
                "**/*.{md,yml,yaml,json,js,mjs,cjs,ts}",
            ],
            install: "bun install",
        },
        fallback: None,
    },
    Step {
        name: "actionlint",
        primary: Run {
            tool: "actionlint",
            probe: Probe::Command(&["actionlint", "-version"]),
            // No path argument. actionlint resolves the enclosing git
            // repository and reads its `.github/workflows`, which leaves
            // Ember's checkout under vendor/ to Ember's own gate. It takes
            // files rather than directories, so naming the directory would be
            // a read error rather than a narrowing.
            //
            // The empty flags turn off the external analyzers. actionlint runs
            // shellcheck and pyflakes when it finds them on PATH and says
            // nothing at all when it does not, and ubuntu-latest carries
            // shellcheck while windows-latest does not. Left on, the matrix legs
            // check different things and the quiet leg reports a pass for an
            // analysis it never ran.
            command: &["actionlint", "-shellcheck=", "-pyflakes="],
            // The version is here and in .github/go-tools, and a test asserts
            // the two agree. actionlint is on neither crates.io nor
            // taiki-e/install-action, so it cannot sit in .github/cargo-tools
            // beside the rest.
            install: "go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12",
        },
        fallback: None,
    },
    Step {
        name: "zizmor",
        primary: Run {
            tool: "zizmor",
            probe: Probe::Command(&["zizmor", "--version"]),
            // Named paths, because Ember's checkout under vendor/ carries
            // workflows of its own and Ember's gate covers them.
            //
            // --strict-collection makes a file zizmor cannot parse a failure.
            // Without it zizmor logs a warning, drops the file, and reports no
            // findings for a workflow it never read, which a byte order mark
            // at the top of the file is enough to cause.
            //
            // --offline keeps the audit from needing a GitHub token, so a
            // runner and a laptop report the same findings. --config names the
            // committed configuration, so ZIZMOR_CONFIG in the environment
            // cannot swap it for another.
            command: &[
                "zizmor",
                "--no-progress",
                "--offline",
                "--strict-collection",
                "--config",
                ".github/zizmor.yml",
                ".github/workflows",
                ".github/dependabot.yml",
            ],
            install: "cargo install zizmor --locked",
        },
        fallback: None,
    },
];

// ///////////////////////////////////////////////
// Results
// ///////////////////////////////////////////////

/// What one step produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command ran and exited zero.
    Passed,
    /// The command ran and exited non-zero.
    Failed,
    /// The tool is absent, so the step never ran.
    Missing,
    /// The tool answered its probe and the command still would not start.
    Unstartable(String),
}

/// One step's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The step's name.
    pub step: &'static str,
    /// The tool that ran, or the one found missing.
    pub tool: &'static str,
    /// The command that installs `tool`.
    pub install: &'static str,
    /// What the step produced.
    pub outcome: Outcome,
    /// The command that ran. Empty when the tool was missing.
    pub command: String,
    /// Set when this step fell back to its second tool.
    pub fell_back: bool,
}

impl Row {
    /// Report whether this step passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Passed
    }

    /// The text that follows the glyph on this row's summary line.
    fn label(&self) -> String {
        match &self.outcome {
            Outcome::Passed if self.fell_back => format!("{} ({})", self.step, self.command),
            Outcome::Passed | Outcome::Failed => self.step.to_string(),
            Outcome::Missing => format!("{} ({} is not installed)", self.step, self.tool),
            Outcome::Unstartable(why) => format!("{} ({why})", self.step),
        }
    }

    /// The glyph that opens this row's summary line.
    fn glyph(&self) -> &'static str {
        match self.outcome {
            Outcome::Passed => "\u{2713}",
            Outcome::Missing => "!",
            Outcome::Failed | Outcome::Unstartable(_) => "\u{2717}",
        }
    }
}

// ///////////////////////////////////////////////
// The gate
// ///////////////////////////////////////////////

/// The gate's steps and the runner that executes them.
pub struct Gate<'a> {
    steps: &'a [Step],
    runner: &'a dyn Runner,
}

impl<'a> Gate<'a> {
    /// Build a gate over `steps` that executes through `runner`.
    #[must_use]
    pub fn new(steps: &'a [Step], runner: &'a dyn Runner) -> Self {
        Self { steps, runner }
    }

    /// Run every step in order, stopping at the first that does not pass.
    ///
    /// Each command goes to `out` before it runs, so the output that follows is
    /// attributable to it. The summary goes to `out` at the end.
    ///
    /// # Errors
    ///
    /// Returns the error from writing to `out`.
    pub fn run(&self, out: &mut dyn Write) -> io::Result<Vec<Row>> {
        writeln!(out, "check")?;
        let mut rows = Vec::with_capacity(self.steps.len());
        for step in self.steps {
            let row = self.run_step(step, out)?;
            let stop = !row.passed();
            rows.push(row);
            if stop {
                break;
            }
        }
        summarize(out, &rows)?;
        Ok(rows)
    }

    /// Report whether the tool behind `probe` is installed.
    fn installed(&self, probe: Probe) -> bool {
        match probe {
            Probe::Command(command) => self.runner.probe(command),
            Probe::File(relative) => self.runner.file_exists(relative),
        }
    }

    /// Run one step, falling back to its second tool when the first is absent.
    fn run_step(&self, step: &Step, out: &mut dyn Write) -> io::Result<Row> {
        let (chosen, fell_back) = if self.installed(step.primary.probe) {
            (&step.primary, false)
        } else if let Some(fallback) = &step.fallback
            && self.installed(fallback.probe)
        {
            (fallback, true)
        } else {
            return Ok(Row {
                step: step.name,
                tool: step.primary.tool,
                install: step.primary.install,
                outcome: Outcome::Missing,
                command: String::new(),
                fell_back: false,
            });
        };

        let line = chosen.command.join(" ");
        writeln!(
            out,
            "\n{}",
            line.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
        )?;
        let outcome = match self.runner.run(chosen.command) {
            Ok(Exit::Ok) => Outcome::Passed,
            Ok(Exit::Err) => Outcome::Failed,
            Err(err) => Outcome::Unstartable(err.to_string()),
        };
        Ok(Row {
            step: step.name,
            tool: chosen.tool,
            install: chosen.install,
            outcome,
            command: line,
            fell_back,
        })
    }
}

/// Write the block of result rows and the one line that closes the run.
fn summarize(out: &mut dyn Write, rows: &[Row]) -> io::Result<()> {
    let labels: Vec<String> = rows.iter().map(Row::label).collect();
    let width = labels
        .iter()
        .map(|label| label.chars().count() + 2)
        .max()
        .unwrap_or(0);

    writeln!(out)?;
    for (row, label) in rows.iter().zip(&labels) {
        let glyph = row.glyph();
        let painted = match row.outcome {
            Outcome::Passed => glyph
                .if_supports_color(Stream::Stdout, OwoColorize::green)
                .to_string(),
            Outcome::Missing => glyph
                .if_supports_color(Stream::Stdout, OwoColorize::yellow)
                .to_string(),
            Outcome::Failed | Outcome::Unstartable(_) => glyph
                .if_supports_color(Stream::Stdout, OwoColorize::red)
                .to_string(),
        };
        writeln!(out, "  {painted} {label}")?;
    }

    let rule = "\u{2500}".repeat(width);
    writeln!(
        out,
        "  {}",
        rule.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
    )?;

    match rows.iter().find(|row| !row.passed()) {
        None => writeln!(out, "  {} steps passed", rows.len()),
        Some(row) if row.outcome == Outcome::Missing => {
            writeln!(out, "  {} did not run", row.step)?;
            writeln!(out, "  install {} with: {}", row.tool, row.install)
        }
        Some(row) => writeln!(out, "  {} failed", row.step),
    }
}

// ///////////////////////////////////////////////
// Tests
// ///////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{Gate, Outcome, Probe, Row, Run, STEPS, Step};
    use crate::runner::{Exit, Runner};

    /// A `Runner` that spawns nothing and records what it was asked to run.
    struct FakeRunner {
        /// Probe commands that answer, each as one joined string.
        installed: Vec<String>,
        /// Files that exist, as the relative paths the step table names.
        present: Vec<String>,
        /// Commands that exit non-zero, each as one joined string.
        failing: Vec<String>,
        /// Every command passed to `run`, in order.
        ran: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        /// Build a runner where every tool answers and every command passes.
        fn all_installed() -> Self {
            let probes = STEPS.iter().flat_map(|step| {
                std::iter::once(step.primary.probe)
                    .chain(step.fallback.as_ref().map(|run| run.probe))
            });
            let mut installed = Vec::new();
            let mut present = Vec::new();
            for probe in probes {
                match probe {
                    Probe::Command(command) => installed.push(command.join(" ")),
                    Probe::File(relative) => present.push(relative.to_string()),
                }
            }
            Self {
                installed,
                present,
                failing: Vec::new(),
                ran: RefCell::new(Vec::new()),
            }
        }

        /// Drop one tool's probe, so the gate sees that tool as absent.
        fn without(mut self, probe: Probe) -> Self {
            match probe {
                Probe::Command(command) => {
                    let gone = command.join(" ");
                    self.installed.retain(|line| *line != gone);
                }
                Probe::File(relative) => self.present.retain(|path| path != relative),
            }
            self
        }

        /// Make one command exit non-zero.
        fn failing(mut self, command: &[&str]) -> Self {
            self.failing.push(command.join(" "));
            self
        }

        /// The commands this runner was asked to run, in order.
        fn ran(&self) -> Vec<String> {
            self.ran.borrow().clone()
        }
    }

    impl Runner for FakeRunner {
        fn probe(&self, command: &[&str]) -> bool {
            self.installed.contains(&command.join(" "))
        }

        fn run(&self, command: &[&str]) -> std::io::Result<Exit> {
            let line = command.join(" ");
            self.ran.borrow_mut().push(line.clone());
            Ok(if self.failing.contains(&line) {
                Exit::Err
            } else {
                Exit::Ok
            })
        }

        fn file_exists(&self, relative: &str) -> bool {
            self.present.iter().any(|path| path == relative)
        }
    }

    /// Run the real step table against `runner` and return its rows and output.
    fn gate(runner: &FakeRunner) -> (Vec<Row>, String) {
        let mut out = Vec::new();
        let rows = Gate::new(STEPS, runner).run(&mut out).expect("write");
        (rows, String::from_utf8(out).expect("utf-8"))
    }

    /// Every field the summary reads has to be filled, or a row renders blank.
    #[test]
    fn every_step_declares_a_runnable_command() {
        for step in STEPS {
            assert!(!step.name.is_empty(), "a step has no name");
            for run in std::iter::once(&step.primary).chain(step.fallback.as_ref()) {
                assert!(!run.tool.is_empty(), "{}: tool is empty", step.name);
                match run.probe {
                    Probe::Command(command) => {
                        assert!(!command.is_empty(), "{}: probe is empty", step.name);
                    }
                    Probe::File(relative) => {
                        assert!(!relative.is_empty(), "{}: probe names no file", step.name);
                    }
                }
                assert!(!run.command.is_empty(), "{}: command is empty", step.name);
                assert!(!run.install.is_empty(), "{}: install is empty", step.name);
            }
        }
    }

    /// The gate is the source formatters, lint, the test runs, the supply chain
    /// checks, the markup formatter, then the workflow checks, and each step
    /// runs the command its row names.
    #[test]
    fn steps_run_in_the_declared_order() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);

        let names: Vec<&str> = rows.iter().map(|row| row.step).collect();
        assert_eq!(
            names,
            [
                "fmt",
                "taplo",
                "clippy",
                "tests",
                "doctests",
                "deny",
                "machete",
                "audit",
                "prettier",
                "actionlint",
                "zizmor"
            ]
        );
        assert_eq!(
            runner.ran(),
            [
                "cargo fmt --check",
                "taplo fmt --check",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo nextest run --workspace",
                "cargo test --workspace --doc",
                "cargo deny check",
                "cargo machete crates mods xtask",
                "cargo audit",
                "bunx --no-install --bun prettier --check **/*.{md,yml,yaml,json,js,mjs,cjs,ts}",
                "actionlint -shellcheck= -pyflakes=",
                "zizmor --no-progress --offline --strict-collection --config .github/zizmor.yml \
                 .github/workflows .github/dependabot.yml",
            ]
        );
        assert!(rows.iter().all(Row::passed));
    }

    /// actionlint runs shellcheck and pyflakes when it finds them on `PATH` and
    /// skips them in silence when it does not. `ubuntu-latest` carries
    /// shellcheck and `windows-latest` does not, so without the flags the matrix
    /// legs check different things and the quiet leg reports a pass for an
    /// analysis it never ran.
    #[test]
    fn actionlint_takes_no_analysis_that_depends_on_what_the_host_has() {
        let step = STEPS
            .iter()
            .find(|step| step.name == "actionlint")
            .expect("an actionlint step");

        for flag in ["-shellcheck=", "-pyflakes="] {
            assert!(
                step.primary.command.contains(&flag),
                "{flag} is absent, so that pass is left to whatever the host has: {:?}",
                step.primary.command
            );
        }
    }

    /// Ember's checkout under `vendor/` carries workflows of its own, and
    /// Ember's gate covers them. `zizmor` walks whatever path it is given, so a
    /// bare `.` audits them here too and fails this gate on files this
    /// repository does not own.
    ///
    /// actionlint needs no such argument. It resolves the enclosing git
    /// repository and reads only that one's workflows.
    #[test]
    fn zizmor_names_its_paths_rather_than_walking_the_tree() {
        let step = STEPS
            .iter()
            .find(|step| step.name == "zizmor")
            .expect("a zizmor step");

        let paths: Vec<&&str> = step
            .primary
            .command
            .iter()
            .filter(|argument| argument.starts_with(".github/"))
            .collect();
        assert!(
            !paths.is_empty(),
            "zizmor names no path under .github, so it walks the whole tree: {:?}",
            step.primary.command
        );
        assert!(
            !step.primary.command.contains(&"."),
            "zizmor walks the whole tree, which reaches vendor/: {:?}",
            step.primary.command
        );
    }

    /// zizmor drops a workflow it cannot parse, logs a warning, and reports no
    /// findings, so a byte order mark at the top of a workflow hides every
    /// finding in it. `--strict-collection` turns that into a failure.
    ///
    /// zizmor also reads its configuration from `ZIZMOR_CONFIG`. Naming the
    /// committed file keeps an environment variable from changing what the
    /// gate reports.
    #[test]
    fn zizmor_fails_on_an_unread_file_and_reads_only_the_committed_config() {
        let step = STEPS
            .iter()
            .find(|step| step.name == "zizmor")
            .expect("a zizmor step");
        let command = step.primary.command;

        assert!(
            command.contains(&"--strict-collection"),
            "zizmor skips a file it cannot parse and still passes: {command:?}"
        );
        let config = command
            .iter()
            .position(|argument| *argument == "--config")
            .and_then(|at| command.get(at + 1));
        assert_eq!(
            config,
            Some(&".github/zizmor.yml"),
            "zizmor takes its configuration from wherever the environment says: {command:?}"
        );
    }

    /// `cargo nextest run` runs no doctests, so a doctest that stops compiling
    /// passes a gate whose only test step is nextest.
    #[test]
    fn the_gate_runs_doctests_whichever_runner_ran_the_suite() {
        let tests = &STEPS[3];
        assert_eq!(tests.name, "tests");

        for probe in [None, Some(tests.primary.probe)] {
            let mut runner = FakeRunner::all_installed();
            let mut note = "with cargo-nextest";
            if let Some(probe) = probe {
                runner = runner.without(probe);
                note = "without cargo-nextest";
            }
            let (rows, _) = gate(&runner);

            assert!(rows.iter().all(Row::passed), "{note}: a step did not pass");
            assert!(
                runner
                    .ran()
                    .contains(&"cargo test --workspace --doc".to_string()),
                "{note}: the gate ran no doctests, got {:?}",
                runner.ran()
            );
        }
    }

    /// A `PostToolUse` hook formats JavaScript on every edit. An extension
    /// prettier owns and the gate does not check is a file whose formatting
    /// nothing enforces.
    #[test]
    fn prettier_checks_every_extension_it_owns_here() {
        let step = STEPS
            .iter()
            .find(|step| step.name == "prettier")
            .expect("a prettier step");
        let glob = step
            .primary
            .command
            .last()
            .expect("the prettier step names a glob");
        let list = glob
            .strip_prefix("**/*.{")
            .and_then(|rest| rest.strip_suffix('}'))
            .unwrap_or_else(|| panic!("{glob} is not a brace list of extensions"));
        let covered: Vec<&str> = list.split(',').collect();

        for extension in ["md", "yml", "yaml", "json", "js", "mjs", "cjs", "ts"] {
            assert!(
                covered.contains(&extension),
                "the prettier glob skips .{extension}, got {glob}"
            );
        }
    }

    /// A failing step is the last one to run, whichever step it is.
    #[test]
    fn a_failing_step_stops_the_gate() {
        for (index, step) in STEPS.iter().enumerate() {
            let runner = FakeRunner::all_installed().failing(step.primary.command);
            let (rows, text) = gate(&runner);

            assert_eq!(
                rows.len(),
                index + 1,
                "{} ran the steps after it",
                step.name
            );
            assert_eq!(runner.ran().len(), index + 1, "{}: ran too much", step.name);
            let last = rows.last().expect("one row");
            assert_eq!(last.outcome, Outcome::Failed, "{}", step.name);
            assert!(
                text.contains(&format!("{} failed", step.name)),
                "{}: the summary does not name the failed step, got {text}",
                step.name
            );
        }
    }

    /// A missing tool is named, never skipped, and its step never runs.
    #[test]
    fn a_missing_tool_is_reported_by_name() {
        for (index, step) in STEPS.iter().enumerate() {
            let mut runner = FakeRunner::all_installed().without(step.primary.probe);
            if let Some(fallback) = &step.fallback {
                runner = runner.without(fallback.probe);
            }
            let (rows, text) = gate(&runner);

            let last = rows.last().expect("one row");
            assert_eq!(last.outcome, Outcome::Missing, "{}", step.name);
            assert_eq!(last.tool, step.primary.tool, "{}", step.name);
            assert_eq!(
                runner.ran().len(),
                index,
                "{}: a missing tool still ran",
                step.name
            );
            assert!(
                text.contains(step.primary.tool),
                "{}: the summary does not name the tool, got {text}",
                step.name
            );
            assert!(
                text.contains(step.primary.install),
                "{}: the summary does not say how to install it, got {text}",
                step.name
            );
        }
    }

    /// Without `cargo-nextest` the tests still run, and the summary says which
    /// runner ran.
    #[test]
    fn tests_fall_back_to_cargo_test_when_nextest_is_absent() {
        let step = &STEPS[3];
        assert_eq!(step.name, "tests");
        let runner = FakeRunner::all_installed().without(step.primary.probe);
        let (rows, text) = gate(&runner);

        assert!(rows.iter().all(Row::passed), "the fallback did not pass");
        assert!(runner.ran().contains(&"cargo test --workspace".to_string()));
        assert!(
            !runner
                .ran()
                .contains(&"cargo nextest run --workspace".to_string())
        );
        assert!(
            text.contains("tests (cargo test --workspace)"),
            "the summary does not say which runner ran, got {text}"
        );
    }

    /// A tool that answers its probe and then will not start is a failure, not
    /// a pass.
    #[test]
    fn a_command_that_will_not_start_fails_the_gate() {
        struct Broken;
        impl Runner for Broken {
            fn probe(&self, _command: &[&str]) -> bool {
                true
            }
            fn run(&self, _command: &[&str]) -> std::io::Result<Exit> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "program not found",
                ))
            }
            fn file_exists(&self, _relative: &str) -> bool {
                true
            }
        }

        let mut out = Vec::new();
        let rows = Gate::new(STEPS, &Broken).run(&mut out).expect("write");

        assert_eq!(rows.len(), 1);
        assert!(!rows[0].passed());
        assert!(matches!(rows[0].outcome, Outcome::Unstartable(_)));
    }

    /// An empty step table renders a passing gate with nothing in it.
    #[test]
    fn an_empty_step_table_renders() {
        let runner = FakeRunner::all_installed();
        let mut out = Vec::new();
        let steps: &[Step] = &[];
        let rows = Gate::new(steps, &runner).run(&mut out).expect("write");

        assert!(rows.is_empty());
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("0 steps passed"), "got {text}");
    }

    /// The fallback stays unused while the first tool answers.
    #[test]
    fn the_fallback_is_not_used_while_the_first_tool_answers() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);

        assert!(!rows[3].fell_back);
        assert_eq!(rows[3].tool, "cargo-nextest");
    }

    /// A row's glyph separates a failure from a tool that never ran.
    #[test]
    fn a_missing_tool_and_a_failure_render_differently() {
        let missing = Row {
            step: "audit",
            tool: "cargo-audit",
            install: "cargo install cargo-audit --locked",
            outcome: Outcome::Missing,
            command: String::new(),
            fell_back: false,
        };
        let failed = Row {
            outcome: Outcome::Failed,
            ..missing.clone()
        };

        assert_ne!(missing.glyph(), failed.glyph());
        assert!(missing.label().contains("is not installed"));
        assert_eq!(failed.label(), "audit");
    }

    /// A fallback that repeats its primary command cannot stand in for it.
    #[test]
    fn a_fallback_differs_from_the_tool_it_stands_in_for() {
        for step in STEPS {
            let Some(fallback) = &step.fallback else {
                continue;
            };
            assert_ne!(
                fallback.command, step.primary.command,
                "{}: the fallback repeats the primary command",
                step.name
            );
        }
    }

    /// Every step's name is distinct, so a summary line names one step.
    #[test]
    fn step_names_are_distinct() {
        let mut seen: Vec<&str> = STEPS.iter().map(|step| step.name).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two steps share a name");
    }

    /// The exit code reads the rows, so a missing tool exits non-zero exactly
    /// like a failing one.
    #[test]
    fn the_gate_passes_only_when_every_row_passed() {
        let base = Row {
            step: "fmt",
            tool: "rustfmt",
            install: "rustup component add rustfmt",
            outcome: Outcome::Passed,
            command: "cargo fmt --check".to_string(),
            fell_back: false,
        };
        let cases = [
            (Outcome::Passed, true),
            (Outcome::Failed, false),
            (Outcome::Missing, false),
            (Outcome::Unstartable("no such file".to_string()), false),
        ];

        for (outcome, expected) in cases {
            let row = Row {
                outcome: outcome.clone(),
                ..base.clone()
            };
            assert_eq!(row.passed(), expected, "{outcome:?}");
        }
    }

    /// `bunx` fetches a package from npm when it is absent locally, which
    /// bypasses the lockfile and its integrity hashes. The prettier step has
    /// to refuse rather than fetch, tell the truth about whether the pinned
    /// copy is installed, and name `bun install` as the fix.
    #[test]
    fn prettier_never_installs_from_npm() {
        let step = STEPS
            .iter()
            .find(|step| step.name == "prettier")
            .expect("a prettier step");

        assert!(
            step.primary.command.contains(&"--no-install"),
            "the command can fetch from npm: {:?}",
            step.primary.command
        );
        assert_eq!(
            step.primary.probe,
            Probe::File("node_modules/prettier/package.json"),
            "the probe does not check the locally installed package"
        );
        assert_eq!(step.primary.install, "bun install");
        assert!(
            step.fallback.is_none(),
            "a fallback would be a second fetch path"
        );
    }

    /// A `Run` built by hand carries the same shape the table declares.
    #[test]
    fn a_run_is_addressable_by_field() {
        let run = Run {
            tool: "prettier",
            probe: Probe::File("node_modules/prettier/package.json"),
            command: &["bunx", "--no-install", "--bun", "prettier", "--check", "."],
            install: "bun install",
        };

        assert_eq!(run.probe, Probe::File("node_modules/prettier/package.json"));
        assert_eq!(run.command[0], "bunx");
        assert_eq!(run.tool, "prettier");
        assert_eq!(run.install, "bun install");
    }
}
