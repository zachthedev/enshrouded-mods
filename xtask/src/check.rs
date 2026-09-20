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

/// The file holding the one `shellcheck` release the gate accepts.
pub const SHELLCHECK_PIN: &str = ".github/shellcheck-version";

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
    /// A command whose output has to carry the release a pin file holds. This
    /// is the shape for a tool another tool picks up from `PATH` by itself,
    /// where a different release changes which findings the gate reports while
    /// every step still passes.
    Version {
        /// The command that prints the release. The first element is the
        /// program.
        command: &'static [&'static str],
        /// The file holding the one release that output may carry, relative to
        /// the repository root.
        pin: &'static str,
    },
}

/// A last argument the gate fills in with a tool's resolved path.
///
/// A tool that shells out to a second tool looks the second one up by name
/// unless it is told otherwise, which is a second lookup the gate does not
/// control. Handing over the path the gate resolved makes the checked binary
/// and the used binary the same one by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// The flag the path follows, including its `=`.
    pub flag: &'static str,
    /// The tool whose path on `PATH` fills the flag.
    pub tool: &'static str,
}

/// One command, and the tool that has to be installed to run it.
pub struct Run {
    /// The tool's name, printed when it is missing.
    pub tool: &'static str,
    /// How to tell the tool is installed.
    pub probe: Probe,
    /// The command itself. `command[0]` is the program.
    pub command: &'static [&'static str],
    /// An argument appended to `command` carrying a resolved tool path. The
    /// step refuses to run when the path cannot be resolved, because the
    /// command without it would look the tool up itself.
    pub resolved: Option<Resolved>,
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
            resolved: None,
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
            resolved: None,
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
            resolved: None,
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
            resolved: None,
            install: "cargo install cargo-nextest --locked",
        },
        fallback: Some(Run {
            tool: "cargo test",
            probe: Probe::Command(&["cargo", "--version"]),
            command: &["cargo", "test", "--workspace"],
            resolved: None,
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
            resolved: None,
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
            resolved: None,
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
            resolved: None,
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
            resolved: None,
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
            resolved: None,
            install: "bun install",
        },
        fallback: None,
    },
    Step {
        name: "shellcheck",
        primary: Run {
            tool: "shellcheck",
            // The release, not the presence. actionlint picks shellcheck up
            // from PATH by itself, so a host carrying a different release
            // reports different findings while every step still passes. This
            // step runs before actionlint, and the gate stops at the first step
            // that does not pass, so actionlint is never reached on a host the
            // pin does not cover.
            probe: Probe::Version {
                command: &["shellcheck", "--version"],
                pin: SHELLCHECK_PIN,
            },
            // The git hooks are the POSIX shell this repository owns outside a
            // workflow. actionlint reaches a `run:` block only where it
            // resolves the shell to sh or bash, which no block in the gate job
            // is, so these two files are the bulk of what the analysis covers.
            // A test holds this list equal to what `.githooks` holds, because a
            // command spawns with no shell to expand a glob.
            command: &["shellcheck", ".githooks/commit-msg", ".githooks/pre-push"],
            resolved: None,
            install: "the release .github/shellcheck-version pins, from \
                      https://github.com/koalaman/shellcheck/releases",
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
            // The empty flag turns pyflakes off. actionlint runs an external
            // analyzer when it finds it on PATH and says nothing at all when it
            // does not, so an absent analyzer is a pass for a pass nobody ran.
            // No Windows package manager ships pyflakes, which leaves off as
            // the only setting both matrix legs can agree on.
            //
            // shellcheck stays on, and the step before this one holds it to one
            // release on every host, so both legs run the same analysis.
            command: &["actionlint", "-pyflakes="],
            // shellcheck arrives as a resolved path rather than a name.
            // actionlint is equally silent over an analyzer that is absent and
            // one that will not execute, so naming the binary the step before
            // this one version-checked is what ties the check to the analysis.
            // A name would be looked up a second time, by a resolver the gate
            // does not control.
            resolved: Some(Resolved {
                flag: "-shellcheck=",
                tool: "shellcheck",
            }),
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
            resolved: None,
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
    /// The tool answered and gave a release its pin file does not hold, so the
    /// step never ran. The string says which release is which.
    Mismatched(String),
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
            Outcome::Unstartable(why) | Outcome::Mismatched(why) => {
                format!("{} ({why})", self.step)
            }
        }
    }

    /// The glyph that opens this row's summary line.
    fn glyph(&self) -> &'static str {
        match self.outcome {
            Outcome::Passed => "\u{2713}",
            Outcome::Missing => "!",
            Outcome::Failed | Outcome::Unstartable(_) | Outcome::Mismatched(_) => "\u{2717}",
        }
    }
}

/// What a probe found.
enum Found {
    /// The tool answered, at the pinned release where one is pinned.
    Yes,
    /// The tool did not answer at all.
    No,
    /// The tool answered and gave a release its pin file does not hold.
    Wrong(String),
}

/// The release a pin file holds: its first line that is neither blank nor a
/// comment.
pub fn pinned_release(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

/// The first release `text` carries, as three runs of digits separated by dots.
///
/// The shape locates it rather than a label, because the words a tool prints
/// around its release are its own and change between tools.
fn reported_release(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let candidate = &text[at..end];
        if is_release(candidate) {
            return Some(candidate);
        }
        at = end;
    }
    None
}

/// Report whether `release` is three runs of digits separated by dots.
#[must_use]
pub fn is_release(release: &str) -> bool {
    let parts: Vec<&str> = release.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
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

    /// Ask `run`'s probe what it finds.
    fn found(&self, run: &Run) -> Found {
        match run.probe {
            Probe::Command(command) => {
                if self.runner.capture(command).is_some() {
                    Found::Yes
                } else {
                    Found::No
                }
            }
            Probe::File(relative) => {
                if self.runner.read_file(relative).is_some() {
                    Found::Yes
                } else {
                    Found::No
                }
            }
            Probe::Version { command, pin } => self.at_the_pin(run.tool, command, pin),
        }
    }

    /// Hold what `command` prints against the release `pin` holds.
    ///
    /// An unreadable pin file is a mismatch rather than an absent tool, because
    /// a gate that cannot say which release it wants has nothing to check.
    fn at_the_pin(&self, tool: &str, command: &[&str], pin: &str) -> Found {
        let Some(text) = self.runner.read_file(pin) else {
            return Found::Wrong(format!(
                "{pin} cannot be read, so nothing says which {tool}"
            ));
        };
        let Some(wanted) = pinned_release(&text) else {
            return Found::Wrong(format!("{pin} names no release"));
        };
        let Some(printed) = self.runner.capture(command) else {
            return Found::No;
        };
        match reported_release(&printed) {
            Some(got) if got == wanted => Found::Yes,
            Some(got) => Found::Wrong(format!("{tool} is {got} and {pin} pins {wanted}")),
            None => Found::Wrong(format!("{tool} printed no release to hold against {pin}")),
        }
    }

    /// Run one step, falling back to its second tool when the first is absent.
    fn run_step(&self, step: &Step, out: &mut dyn Write) -> io::Result<Row> {
        let unrun = |outcome| Row {
            step: step.name,
            tool: step.primary.tool,
            install: step.primary.install,
            outcome,
            command: String::new(),
            fell_back: false,
        };
        let (chosen, fell_back) = match self.found(&step.primary) {
            Found::Yes => (&step.primary, false),
            Found::Wrong(why) => return Ok(unrun(Outcome::Mismatched(why))),
            Found::No => match &step.fallback {
                Some(fallback) if matches!(self.found(fallback), Found::Yes) => (fallback, true),
                _ => return Ok(unrun(Outcome::Missing)),
            },
        };

        let mut argv: Vec<String> = chosen.command.iter().map(|a| (*a).to_string()).collect();
        if let Some(resolved) = chosen.resolved {
            let Some(path) = self.runner.resolve(resolved.tool) else {
                return Ok(unrun(Outcome::Mismatched(format!(
                    "{} is not on PATH, so {} would look it up itself",
                    resolved.tool, chosen.tool
                ))));
            };
            argv.push(format!("{}{}", resolved.flag, path.display()));
        }

        let line = argv.join(" ");
        writeln!(
            out,
            "\n{}",
            line.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
        )?;
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let outcome = match self.runner.run(&borrowed) {
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
            Outcome::Failed | Outcome::Unstartable(_) | Outcome::Mismatched(_) => glyph
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
        Some(row) if matches!(row.outcome, Outcome::Missing | Outcome::Mismatched(_)) => {
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
    use std::path::PathBuf;

    use super::{
        Gate, Outcome, Probe, Resolved, Row, Run, SHELLCHECK_PIN, STEPS, Step, is_release,
        pinned_release, reported_release,
    };
    use crate::runner::{Exit, Runner};

    /// The release `FakeRunner` invents for a pinned tool, in both the pin file
    /// it serves and the output it gives that tool's probe. The real release
    /// lives in the real pin file, which no test here reads.
    const FAKE_RELEASE: &str = "9.9.9";

    /// The directory `FakeRunner` claims every tool resolves into.
    const FAKE_BIN: &str = "/fake/bin";

    /// A `Runner` that spawns nothing and records what it was asked to run.
    struct FakeRunner {
        /// Tools that resolve nowhere on `PATH`.
        unresolvable: Vec<String>,
        /// Probe commands that answer, each as one joined string.
        installed: Vec<String>,
        /// What an answering probe prints, as the joined command and its
        /// output.
        printed: Vec<(String, String)>,
        /// Files that can be read, as the relative path and its contents.
        present: Vec<(String, String)>,
        /// Commands that exit non-zero, each as one joined string.
        failing: Vec<String>,
        /// Every command passed to `run`, in order.
        ran: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        /// Build a runner where every tool answers at its pinned release and
        /// every command passes.
        fn all_installed() -> Self {
            let probes = STEPS.iter().flat_map(|step| {
                std::iter::once(step.primary.probe)
                    .chain(step.fallback.as_ref().map(|run| run.probe))
            });
            let mut installed = Vec::new();
            let mut printed = Vec::new();
            let mut present = Vec::new();
            for probe in probes {
                match probe {
                    Probe::Command(command) => installed.push(command.join(" ")),
                    Probe::File(relative) => present.push((relative.to_string(), String::new())),
                    Probe::Version { command, pin } => {
                        installed.push(command.join(" "));
                        printed.push((command.join(" "), format!("version: {FAKE_RELEASE}")));
                        present.push((pin.to_string(), format!("{FAKE_RELEASE}\n")));
                    }
                }
            }
            Self {
                unresolvable: Vec::new(),
                installed,
                printed,
                present,
                failing: Vec::new(),
                ran: RefCell::new(Vec::new()),
            }
        }

        /// Make one tool resolve nowhere on `PATH`.
        fn unresolvable(mut self, program: &str) -> Self {
            self.unresolvable.push(program.to_string());
            self
        }

        /// Drop one tool's probe, so the gate sees that tool as absent.
        ///
        /// A pinned tool keeps its pin file, because a gate that cannot read
        /// the pin is a different failure from one whose tool is not there.
        fn without(mut self, probe: Probe) -> Self {
            match probe {
                Probe::Command(command) | Probe::Version { command, .. } => {
                    let gone = command.join(" ");
                    self.installed.retain(|line| *line != gone);
                }
                Probe::File(relative) => self.present.retain(|(path, _)| path != relative),
            }
            self
        }

        /// Make one pinned tool answer with `release`.
        fn reporting(mut self, command: &[&str], release: &str) -> Self {
            let line = command.join(" ");
            self.printed.retain(|(probe, _)| *probe != line);
            self.printed.push((line, format!("version: {release}")));
            self
        }

        /// Make one pin file unreadable.
        fn unpinned(mut self, pin: &str) -> Self {
            self.present.retain(|(path, _)| path != pin);
            self
        }

        /// Make one command exit non-zero, named by the declared command.
        ///
        /// A step may append a resolved tool path, so the declared command is a
        /// prefix of what runs rather than all of it.
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
        fn capture(&self, command: &[&str]) -> Option<String> {
            let line = command.join(" ");
            if !self.installed.contains(&line) {
                return None;
            }
            Some(
                self.printed
                    .iter()
                    .find(|(probe, _)| *probe == line)
                    .map_or_else(String::new, |(_, output)| output.clone()),
            )
        }

        fn run(&self, command: &[&str]) -> std::io::Result<Exit> {
            let line = command.join(" ");
            self.ran.borrow_mut().push(line.clone());
            let failed = self
                .failing
                .iter()
                .any(|declared| line.starts_with(declared.as_str()));
            Ok(if failed { Exit::Err } else { Exit::Ok })
        }

        fn read_file(&self, relative: &str) -> Option<String> {
            self.present
                .iter()
                .find(|(path, _)| path == relative)
                .map(|(_, contents)| contents.clone())
        }

        fn resolve(&self, program: &str) -> Option<PathBuf> {
            self.unresolvable
                .iter()
                .all(|name| name != program)
                .then(|| PathBuf::from(format!("{FAKE_BIN}/{program}")))
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
                    Probe::Version { command, pin } => {
                        assert!(!command.is_empty(), "{}: probe is empty", step.name);
                        assert!(!pin.is_empty(), "{}: probe names no pin file", step.name);
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
                "shellcheck",
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
                "shellcheck .githooks/commit-msg .githooks/pre-push",
                "actionlint -pyflakes= -shellcheck=/fake/bin/shellcheck",
                "zizmor --no-progress --offline --strict-collection --config .github/zizmor.yml \
                 .github/workflows .github/dependabot.yml",
            ]
        );
        assert!(rows.iter().all(Row::passed));
    }

    /// actionlint runs an external analyzer when it finds it on `PATH` and
    /// skips it in silence when it does not, so a host without one reports a
    /// pass for an analysis nobody ran.
    ///
    /// pyflakes stays off, because no Windows package manager ships it and off
    /// is the only setting both matrix legs can agree on. shellcheck stays on,
    /// and the step before actionlint holds it to one release, so the analysis
    /// actionlint runs is the same on every host.
    #[test]
    fn actionlint_runs_shellcheck_and_the_step_before_it_pins_the_release() {
        let names: Vec<&str> = STEPS.iter().map(|step| step.name).collect();
        let at = |wanted: &str| {
            names
                .iter()
                .position(|name| *name == wanted)
                .unwrap_or_else(|| panic!("no {wanted} step"))
        };
        let shellcheck = at("shellcheck");
        let actionlint = at("actionlint");
        assert!(
            shellcheck < actionlint,
            "actionlint runs before the step that holds shellcheck to its release"
        );

        let command = STEPS[actionlint].primary.command;
        assert!(
            command.contains(&"-pyflakes="),
            "pyflakes is left to whatever the host has: {command:?}"
        );
        assert!(
            !command.iter().any(|flag| flag.starts_with("-shellcheck")),
            "the shellcheck analysis is turned off, so a run: block actionlint \
             resolves to sh or bash goes unread: {command:?}"
        );

        let probe = STEPS[shellcheck].primary.probe;
        let Probe::Version { pin, .. } = probe else {
            panic!("the shellcheck step probes with {probe:?}, which passes at any release");
        };
        assert_eq!(pin, SHELLCHECK_PIN);

        assert_eq!(
            STEPS[actionlint].primary.resolved,
            Some(Resolved {
                flag: "-shellcheck=",
                tool: "shellcheck",
            }),
            "actionlint takes the analyzer by name, so it resolves a binary the gate did not check"
        );
    }

    /// actionlint is equally silent over an absent analyzer and one that will
    /// not execute, so the gate hands it the path it resolved rather than the
    /// name. A name would be looked up again by a resolver the gate does not
    /// control, which is the join this step exists to remove.
    #[test]
    fn actionlint_is_handed_the_analyzer_path_the_gate_resolved() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);

        assert!(rows.iter().all(Row::passed), "a step did not pass");
        let line = runner
            .ran()
            .into_iter()
            .find(|line| line.starts_with("actionlint"))
            .expect("the gate ran actionlint");
        assert!(
            line.contains(&format!("-shellcheck={FAKE_BIN}/shellcheck")),
            "actionlint was not given the resolved analyzer path, got {line:?}"
        );
        assert!(
            !line.contains("-shellcheck= "),
            "the analyzer flag is empty, which turns the analysis off: {line:?}"
        );
    }

    /// An analyzer the gate cannot locate stops the step. Running actionlint
    /// without the flag would put the lookup back where it started, and a
    /// failed lookup there is silent.
    #[test]
    fn an_analyzer_that_resolves_nowhere_stops_the_step() {
        let runner = FakeRunner::all_installed().unresolvable("shellcheck");
        let (rows, text) = gate(&runner);

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "actionlint");
        assert!(!last.passed());
        assert!(
            matches!(&last.outcome, Outcome::Mismatched(why) if why.contains("shellcheck")),
            "got {:?}",
            last.outcome
        );
        assert!(
            !runner
                .ran()
                .iter()
                .any(|line| line.starts_with("actionlint")),
            "actionlint ran anyway: {:?}",
            runner.ran()
        );
        assert!(text.contains("actionlint did not run"), "got {text}");
    }

    /// A tool at the wrong release stops the gate before the step that would
    /// have used it, and the summary says which release is which rather than
    /// calling the tool absent.
    #[test]
    fn a_pinned_tool_at_another_release_stops_the_gate() {
        let shellcheck = step("shellcheck");
        let Probe::Version { command, pin } = shellcheck.primary.probe else {
            panic!("the shellcheck step carries no pinned release");
        };

        let runner = FakeRunner::all_installed().reporting(command, "1.2.3");
        let (rows, text) = gate(&runner);

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "shellcheck");
        assert_eq!(
            last.outcome,
            Outcome::Mismatched(format!("shellcheck is 1.2.3 and {pin} pins {FAKE_RELEASE}"))
        );
        assert!(
            !runner
                .ran()
                .iter()
                .any(|line| line.starts_with("actionlint")),
            "actionlint ran against an analyzer the pin does not cover: {:?}",
            runner.ran()
        );
        assert!(
            text.contains("shellcheck did not run"),
            "the summary does not say the step was skipped, got {text}"
        );
        assert!(
            text.contains(shellcheck.primary.install),
            "the summary does not say how to install it, got {text}"
        );
    }

    /// A pin file nothing can read leaves the gate with no release to hold the
    /// tool to, which is a refusal rather than a pass.
    #[test]
    fn an_unreadable_pin_stops_the_gate() {
        let Probe::Version { pin, .. } = step("shellcheck").primary.probe else {
            panic!("the shellcheck step carries no pinned release");
        };

        let runner = FakeRunner::all_installed().unpinned(pin);
        let (rows, _) = gate(&runner);

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "shellcheck");
        assert!(!last.passed());
        assert!(
            matches!(&last.outcome, Outcome::Mismatched(why) if why.contains(pin)),
            "got {:?}",
            last.outcome
        );
    }

    /// A pin file holds one release past its comments, and a tool prints its
    /// own among words of its own.
    #[test]
    fn the_release_readers_take_the_shapes_they_name() {
        let pins = [
            ("0.11.0\n", Some("0.11.0")),
            ("# a comment\n\n1.4.2\n", Some("1.4.2")),
            ("  1.4.2  \n", Some("1.4.2")),
            ("# only comments\n", None),
            ("", None),
        ];
        for (text, expected) in pins {
            assert_eq!(pinned_release(text), expected, "{text:?}");
        }

        let outputs = [
            (
                "ShellCheck - shell script analysis tool\nversion: 0.11.0\n",
                Some("0.11.0"),
            ),
            ("actionlint 1.7.12\nbuilt with go1.26.1", Some("1.7.12")),
            ("tool 7 of 9, release 2.0.1", Some("2.0.1")),
            ("1.4", None),
            ("no digits here", None),
            ("", None),
        ];
        for (text, expected) in outputs {
            assert_eq!(reported_release(text), expected, "{text:?}");
        }
    }

    /// Three runs of digits and nothing else, so a two-part or four-part number
    /// is never read as a release.
    #[test]
    fn is_release_reads_three_runs_of_digits() {
        let cases = [
            ("0.11.0", true),
            ("1.98.1", true),
            ("01.04.02", true),
            ("1.4", false),
            ("1.4.2.1", false),
            ("1..2", false),
            ("1.4.", false),
            ("", false),
        ];
        for (release, expected) in cases {
            assert_eq!(is_release(release), expected, "{release:?}");
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

    /// The extensions in a `{a,b}` brace list, sorted.
    fn brace_list(pattern: &str) -> Vec<String> {
        let open = pattern
            .find('{')
            .unwrap_or_else(|| panic!("{pattern} holds no brace list"));
        let close = pattern
            .rfind('}')
            .unwrap_or_else(|| panic!("{pattern} holds no brace list"));
        let mut list: Vec<String> = pattern[open + 1..close]
            .split(',')
            .map(str::to_string)
            .collect();
        list.sort();
        list
    }

    /// The step named `name`.
    fn step(name: &str) -> &'static Step {
        STEPS
            .iter()
            .find(|step| step.name == name)
            .unwrap_or_else(|| panic!("no {name} step"))
    }

    /// `.editorconfig` names the extensions prettier owns here, in the section
    /// its comment introduces, and prettier reads that file. An extension it
    /// owns that the gate's glob leaves out is a file whose formatting nothing
    /// enforces, and one the glob adds is formatted to a width no editor
    /// agrees with.
    #[test]
    fn prettier_checks_the_extensions_editorconfig_gives_it() {
        let editorconfig = std::fs::read_to_string(crate::repo_root().join(".editorconfig"))
            .expect(".editorconfig is readable");
        let section = editorconfig
            .lines()
            .skip_while(|line| !(line.starts_with('#') && line.contains("prettier owns")))
            .find(|line| line.starts_with('['))
            .expect(".editorconfig introduces the section prettier owns");

        let glob = step("prettier")
            .primary
            .command
            .last()
            .expect("the prettier step names a glob");
        assert!(
            glob.starts_with("**/*.{"),
            "{glob} is not a brace list of extensions"
        );
        assert_eq!(
            brace_list(glob),
            brace_list(section),
            "the prettier glob and the .editorconfig section it owns disagree"
        );
    }

    /// `cargo machete` walks the directories the step names, which keeps it
    /// off Ember's checkout under `vendor/`. They have to be the ones the
    /// workspace members live under, or a member added under a new directory
    /// is a crate machete never reads.
    #[test]
    fn machete_walks_the_directories_the_members_live_under() {
        let manifest: toml::Value = toml::from_str(
            &std::fs::read_to_string(crate::repo_root().join("Cargo.toml"))
                .expect("the workspace manifest is readable"),
        )
        .expect("the workspace manifest parses");
        let mut members: Vec<&str> = manifest["workspace"]["members"]
            .as_array()
            .expect("the workspace lists members")
            .iter()
            .filter_map(toml::Value::as_str)
            .filter_map(|member| member.split('/').next())
            .collect();
        members.sort_unstable();
        members.dedup();

        let command = step("machete").primary.command;
        assert_eq!(
            &command[..2],
            ["cargo", "machete"],
            "the machete step runs {command:?}"
        );
        let mut walked: Vec<&str> = command[2..].to_vec();
        walked.sort_unstable();
        assert_eq!(
            walked, members,
            "cargo machete walks {walked:?} and the members live under {members:?}"
        );
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
            fn capture(&self, _command: &[&str]) -> Option<String> {
                Some(FAKE_RELEASE.to_string())
            }
            fn run(&self, _command: &[&str]) -> std::io::Result<Exit> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "program not found",
                ))
            }
            fn read_file(&self, _relative: &str) -> Option<String> {
                Some(FAKE_RELEASE.to_string())
            }
            fn resolve(&self, program: &str) -> Option<PathBuf> {
                Some(PathBuf::from(format!("{FAKE_BIN}/{program}")))
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
            (
                Outcome::Mismatched("shellcheck is 1.2.3".to_string()),
                false,
            ),
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
            resolved: None,
            install: "bun install",
        };

        assert_eq!(run.probe, Probe::File("node_modules/prettier/package.json"));
        assert_eq!(run.command[0], "bunx");
        assert_eq!(run.tool, "prettier");
        assert_eq!(run.install, "bun install");
    }
}
