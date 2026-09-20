//! The gate every change passes, behind one command.
//!
//! `CONTRIBUTING.md`, the `pre-push` hook and continuous integration all call
//! `cargo xtask check`, so none of them can drift from the others. Steps run in
//! the order they are declared and the run stops at the first one that does not
//! pass.

use std::io::{self, Write};

use owo_colors::{OwoColorize, Stream};

use crate::pins::{self, pinned_version};
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
    /// A command whose output has to carry the version [`pins::PINS`] holds for the
    /// program that printed it. The first element is the program, and its name
    /// is the key read from [`pins::PINS`], so the checked release and the pinned one
    /// cannot disagree.
    ///
    /// This is the shape for a tool a second tool reads findings from, where a
    /// different release changes those findings while every step still passes.
    Version(&'static [&'static str]),
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
    /// Whether mise installs `command[0]`, under the name `command[0]` spells.
    ///
    /// The gate resolves such a program once through `mise which` and runs the
    /// path it gives back, probe included, so the binary the probe answered and
    /// the binary the step ran are the same one. A program the toolchain or the
    /// package manager provides is not one of these and runs by name.
    pub mise: bool,
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
            mise: false,
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
            tool: "taplo",
            mise: true,
            probe: Probe::Command(&["taplo", "--version"]),
            // The files and the exclusions are in .taplo.toml, so the same set
            // is formatted whether the gate or an editor runs the tool.
            command: &["taplo", "fmt", "--check"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "clippy",
        primary: Run {
            tool: "clippy",
            mise: false,
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
            mise: true,
            // The binary takes its own subcommand name as its first argument,
            // which is the convention cargo's dispatch supplies when the tool
            // runs as `cargo nextest`. The gate names the path, so it supplies
            // the argument itself.
            probe: Probe::Command(&["cargo-nextest", "nextest", "--version"]),
            command: &["cargo-nextest", "nextest", "run", "--workspace"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: Some(Run {
            tool: "cargo test",
            mise: false,
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
            mise: false,
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
            mise: true,
            // This binary takes its subcommand straight, with no repeat of its
            // own name, unlike cargo-nextest and cargo-audit.
            probe: Probe::Command(&["cargo-deny", "--version"]),
            command: &["cargo-deny", "check"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "machete",
        primary: Run {
            tool: "cargo-machete",
            mise: true,
            // This binary takes paths straight and has no subcommand at all.
            probe: Probe::Command(&["cargo-machete", "--version"]),
            // Named directories, because vendor/ holds Ember's own workspace
            // and its manifests are checked by Ember's own gate.
            command: &["cargo-machete", "crates", "mods", "xtask"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "audit",
        primary: Run {
            tool: "cargo-audit",
            mise: true,
            // This binary takes its own subcommand name first, the way
            // cargo-nextest does.
            probe: Probe::Command(&["cargo-audit", "--version"]),
            command: &["cargo-audit", "audit"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "prettier",
        primary: Run {
            tool: "prettier",
            mise: false,
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
            mise: true,
            // The release, not the presence. A stale install directory answers
            // under the name mise resolves while carrying another release, and
            // a different release changes which findings actionlint reports
            // while every step still passes. This step runs before actionlint,
            // and the gate stops at the first step that does not pass, so
            // actionlint is never reached on a host the pin does not cover.
            probe: Probe::Version(&["shellcheck", "--version"]),
            // The git hooks are the POSIX shell this repository owns outside a
            // workflow. actionlint reaches a `run:` block only where it
            // resolves the shell to sh or bash, which no block in the gate job
            // is, so these two files are the bulk of what the analysis covers.
            // A test holds this list equal to what `.githooks` holds, because a
            // command spawns with no shell to expand a glob.
            command: &["shellcheck", ".githooks/commit-msg", ".githooks/pre-push"],
            resolved: None,
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "actionlint",
        primary: Run {
            tool: "actionlint",
            mise: true,
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
            install: MISE_INSTALL,
        },
        fallback: None,
    },
    Step {
        name: "zizmor",
        primary: Run {
            tool: "zizmor",
            mise: true,
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
            install: MISE_INSTALL,
        },
        fallback: None,
    },
];

/// What to run when a tool mise owns is absent.
///
/// One command covers every one of them, because [`pins::PINS`] names them all and
/// mise reads it.
const MISE_INSTALL: &str = "mise install";

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
        let mut rows = Vec::with_capacity(self.steps.len() + 1);
        let first = self.pin_row(out)?;
        let mut stop = !first.passed();
        rows.push(first);
        if !stop {
            for step in self.steps {
                let row = self.run_step(step, out)?;
                stop = !row.passed();
                rows.push(row);
                if stop {
                    break;
                }
            }
        }
        summarize(out, &rows)?;
        Ok(rows)
    }

    /// Hold the pin files to their rules and print every problem.
    ///
    /// Returns true when a problem was found, which is what the `pins`
    /// subcommand exits on.
    ///
    /// # Errors
    ///
    /// Returns the error from writing to `out`.
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
        // The environment can point mise at a file other than the one these
        // rules read, which would leave them judging a document mise ignores.
        // Reading the variables costs no process, so the row still runs before
        // mise exists on a machine.
        let mut found = pins::environment_problems(|name| std::env::var(name).ok());
        found.extend(
            match (read(pins::PINS), read(pins::LOCK), read(pins::WORKFLOW)) {
                (Ok(pin_text), Ok(lock), Ok(workflow)) => {
                    pins::problems(&pin_text, &lock, &workflow)
                }
                (first, second, third) => [first, second, third]
                    .into_iter()
                    .filter_map(Result::err)
                    .collect(),
            },
        );
        found
    }

    /// The row for the pin rules, which run before any tool does.
    ///
    /// A lockfile entry carrying a url and no checksum installs whatever that
    /// url serves, so a rule that ran after the tools would report a finding
    /// about a binary that had already executed.
    fn pin_row(&self, out: &mut dyn Write) -> io::Result<Row> {
        let problems = self.pin_problems();
        let outcome = if problems.is_empty() {
            Outcome::Passed
        } else {
            for problem in &problems {
                writeln!(out, "  {problem}")?;
            }
            Outcome::Failed
        };
        Ok(Row {
            step: "pins",
            tool: pins::PINS,
            install: pins::RELOCK,
            outcome,
            command: String::new(),
            fell_back: false,
        })
    }

    /// The program `run` executes: the path mise gives for a tool it owns, or
    /// the name itself for a program the toolchain provides.
    ///
    /// `None` when mise owns the tool and resolves nothing, which is how an
    /// uninstalled tool reads.
    fn program(&self, run: &Run) -> Option<String> {
        if !run.mise {
            return Some(run.command[0].to_string());
        }
        self.runner
            .resolve(run.command[0])
            .map(|path| path.display().to_string())
    }

    /// Ask `run`'s probe what it finds, with `program` standing in for the name
    /// its command spells.
    fn found(&self, run: &Run, program: &str) -> Found {
        let at = |command: &[&'static str]| {
            let mut argv: Vec<String> = command.iter().map(|word| (*word).to_string()).collect();
            argv[0] = program.to_string();
            argv
        };
        match run.probe {
            Probe::Command(command) => {
                let argv = at(command);
                let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
                if self.runner.capture(&borrowed).is_some() {
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
            Probe::Version(command) => {
                let argv = at(command);
                let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
                self.at_the_pin(run.tool, command[0], &borrowed)
            }
        }
    }

    /// Hold what `command` prints against the version [`pins::PINS`] holds for
    /// `name`.
    ///
    /// An unreadable pin file is a mismatch rather than an absent tool, because
    /// a gate that cannot say which release it wants has nothing to check.
    fn at_the_pin(&self, tool: &str, name: &str, command: &[&str]) -> Found {
        let Some(text) = self.runner.read_file(pins::PINS) else {
            return Found::Wrong(format!(
                "{} cannot be read, so nothing says which {tool}",
                pins::PINS
            ));
        };
        let Some(wanted) = pinned_version(&text, name) else {
            return Found::Wrong(format!("{} pins no version for {name}", pins::PINS));
        };
        let Some(printed) = self.runner.capture(command) else {
            return Found::No;
        };
        match reported_release(&printed) {
            Some(got) if got == wanted => Found::Yes,
            Some(got) => Found::Wrong(format!("{tool} is {got} and {} pins {wanted}", pins::PINS)),
            None => Found::Wrong(format!(
                "{tool} printed no release to hold against {}",
                pins::PINS
            )),
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
        let ask = |run: &Run| match self.program(run) {
            Some(program) => (self.found(run, &program), program),
            None => (Found::No, String::new()),
        };
        let (chosen, program, fell_back) = match ask(&step.primary) {
            (Found::Yes, program) => (&step.primary, program, false),
            (Found::Wrong(why), _) => return Ok(unrun(Outcome::Mismatched(why))),
            (Found::No, _) => match &step.fallback {
                Some(fallback) => match ask(fallback) {
                    (Found::Yes, program) => (fallback, program, true),
                    _ => return Ok(unrun(Outcome::Missing)),
                },
                None => return Ok(unrun(Outcome::Missing)),
            },
        };

        let mut argv: Vec<String> = chosen.command.iter().map(|a| (*a).to_string()).collect();
        argv[0] = program;
        if let Some(resolved) = chosen.resolved {
            let Some(path) = self.runner.resolve(resolved.tool) else {
                return Ok(unrun(Outcome::Mismatched(format!(
                    "mise resolves no {}, so {} would look it up itself",
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
    use std::fmt::Write as _;
    use std::path::PathBuf;

    use crate::pins;

    use super::{
        Gate, MISE_INSTALL, Outcome, Probe, Resolved, Row, Run, STEPS, Step, is_release,
        pinned_version, reported_release,
    };
    use crate::runner::{Exit, Runner};

    /// The release `FakeRunner` invents for a pinned tool, in both the pin file
    /// it serves and the output it gives that tool's probe. The real release
    /// lives in the real pin file, which no test here reads.
    const FAKE_RELEASE: &str = "9.9.9";

    /// A digest of the shape the pin rules require, for the fixture lockfile.
    const FAKE_DIGEST: &str = "abababababababababababababababababababababababababababababababab";

    /// The directory `FakeRunner` claims every mise tool resolves into.
    const FAKE_BIN: &str = "/fake/bin";

    /// The line the gate builds from `command`, with a mise tool's program
    /// replaced by the path mise resolves for it.
    ///
    /// Every case keys on this rather than on the declared name, so a step that
    /// stopped running the resolved path stops matching.
    fn as_run(run: &Run, command: &[&str]) -> String {
        let mut argv: Vec<String> = command.iter().map(|word| (*word).to_string()).collect();
        if run.mise {
            argv[0] = format!("{FAKE_BIN}/{}", run.command[0]);
        }
        argv.join(" ")
    }

    /// A `Runner` that spawns nothing and records what it was asked to run.
    struct FakeRunner {
        /// Tools mise resolves nowhere.
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
            let mut installed = Vec::new();
            let mut printed = Vec::new();
            let mut present = Vec::new();
            // The pin files the gate reads before any step. Both are built from
            // `pins::TOOLS`, so the rules that run first pass and the cases
            // below exercise the steps rather than the pin round. The entries
            // carry a backend and a url as well as a checksum, because the
            // rules hold a lockfile to the owner and the repository in source
            // and a thinner entry is one no real lockfile would contain.
            let mut pinned = String::from("[tools]\n");
            let mut lock = String::new();
            for step in STEPS {
                for run in std::iter::once(&step.primary).chain(step.fallback.as_ref()) {
                    match run.probe {
                        Probe::Command(command) => installed.push(as_run(run, command)),
                        Probe::File(relative) => {
                            present.push((relative.to_string(), String::new()));
                        }
                        Probe::Version(command) => {
                            let line = as_run(run, command);
                            installed.push(line.clone());
                            printed.push((line, format!("version: {FAKE_RELEASE}")));
                        }
                    }
                }
            }
            for tool in pins::TOOLS {
                writeln!(pinned, "\"{}\" = \"{FAKE_RELEASE}\"", tool.key)
                    .expect("write to a String");
                writeln!(
                    lock,
                    "[[tools.\"{}\"]]\nversion = \"{FAKE_RELEASE}\"\nbackend = \"{}\"",
                    tool.key,
                    tool.coordinate()
                )
                .expect("write to a String");
                for platform in ["linux-x64", "windows-x64"] {
                    writeln!(
                        lock,
                        "[tools.\"{}\".\"platforms.{platform}\"]\nchecksum = \"sha256:{}\"\nurl = \
                         \"https://github.com{}{}/{}\"\nurl_api = \
                         \"https://api.github.com/repos/{}/{}/releases/assets/1\"",
                        tool.key,
                        FAKE_DIGEST,
                        tool.release_prefix(),
                        tool.tag(FAKE_RELEASE),
                        tool.binary,
                        tool.owner,
                        tool.repository
                    )
                    .expect("write to a String");
                    // An attested tool's entry carries the line that makes
                    // verification required for it, the same as a real one.
                    if let Some(provenance) = tool.provenance {
                        writeln!(lock, "provenance = \"{provenance}\"").expect("write to a String");
                    }
                }
            }
            pinned.push_str(
                "\n[tool_config]\nlocked = true\n\n[settings]\nlocked = \
                 true\nlocked_verify_provenance = true\n",
            );
            present.push((pins::PINS.to_string(), pinned));
            present.push((pins::LOCK.to_string(), lock));
            present.push((
                pins::WORKFLOW.to_string(),
                "        os: [windows-latest, ubuntu-latest]\n".to_string(),
            ));
            Self {
                unresolvable: Vec::new(),
                installed,
                printed,
                present,
                failing: Vec::new(),
                ran: RefCell::new(Vec::new()),
            }
        }

        /// Make one tool resolve nowhere under mise.
        fn unresolvable(mut self, tool: &str) -> Self {
            self.unresolvable.push(tool.to_string());
            self
        }

        /// Drop one run's probe, so the gate sees that tool as absent.
        ///
        /// A pinned tool keeps the pin file, because a gate that cannot read
        /// the pin is a different failure from one whose tool is not there.
        fn without(mut self, run: &Run) -> Self {
            match run.probe {
                Probe::Command(command) | Probe::Version(command) => {
                    let gone = as_run(run, command);
                    self.installed.retain(|line| *line != gone);
                }
                Probe::File(relative) => self.present.retain(|(path, _)| path != relative),
            }
            self
        }

        /// Make one pinned tool answer with `release`.
        fn reporting(mut self, run: &Run, release: &str) -> Self {
            let Probe::Version(command) = run.probe else {
                panic!("{} carries no pinned release", run.tool);
            };
            let line = as_run(run, command);
            self.printed.retain(|(probe, _)| *probe != line);
            self.printed.push((line, format!("version: {release}")));
            self
        }

        /// Make the pin file unreadable.
        fn unpinned(mut self) -> Self {
            self.present.retain(|(path, _)| path != pins::PINS);
            self
        }

        /// Make one run's command exit non-zero.
        ///
        /// A step may append a resolved tool path, so the declared command is a
        /// prefix of what runs rather than all of it.
        fn failing(mut self, run: &Run) -> Self {
            self.failing.push(as_run(run, run.command));
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

        fn resolve(&self, tool: &str) -> Option<PathBuf> {
            self.unresolvable
                .iter()
                .all(|name| name != tool)
                .then(|| PathBuf::from(format!("{FAKE_BIN}/{tool}")))
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
                    Probe::Command(command) | Probe::Version(command) => {
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

    /// A mise tool runs by the one path `mise which` gave, probe included, so
    /// nothing looks the name up a second time.
    ///
    /// The probe and the command have to open with the same program for that to
    /// hold: the gate substitutes the resolved path into both, and a probe
    /// naming another program would answer for a binary no step runs.
    #[test]
    fn every_mise_tool_probes_and_runs_the_same_program() {
        for step in STEPS {
            for run in std::iter::once(&step.primary).chain(step.fallback.as_ref()) {
                if !run.mise {
                    assert_ne!(
                        run.install, MISE_INSTALL,
                        "{}: {} installs with mise and does not resolve through it",
                        step.name, run.tool
                    );
                    continue;
                }
                assert_eq!(
                    run.install, MISE_INSTALL,
                    "{}: {} resolves through mise and names another installer",
                    step.name, run.tool
                );
                let probed = match run.probe {
                    Probe::Command(command) | Probe::Version(command) => command[0],
                    Probe::File(relative) => {
                        panic!("{}: a mise tool probes for the file {relative}", step.name)
                    }
                };
                assert_eq!(
                    probed, run.command[0],
                    "{}: the probe runs {probed} and the step runs {}",
                    step.name, run.command[0]
                );
            }
        }
    }

    /// Every step whose tool mise owns runs the resolved path rather than a
    /// bare name. A bare name would be looked up again, by a resolver the gate
    /// does not control, which is the join the resolution exists to remove.
    #[test]
    fn every_mise_step_runs_the_path_mise_resolved() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);
        assert!(rows.iter().all(Row::passed), "a step did not pass");

        let ran = runner.ran();
        for step in STEPS {
            if !step.primary.mise {
                continue;
            }
            let wanted = format!("{FAKE_BIN}/{}", step.primary.command[0]);
            assert!(
                ran.iter().any(|line| line.starts_with(&wanted)),
                "{} ran no command opening with {wanted}, got {ran:?}",
                step.name
            );
            assert!(
                !ran.iter()
                    .any(|line| line.starts_with(&format!("{} ", step.primary.command[0]))),
                "{} ran {} by name, got {ran:?}",
                step.name,
                step.primary.command[0]
            );
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
                "pins",
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
                "/fake/bin/taplo fmt --check",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "/fake/bin/cargo-nextest nextest run --workspace",
                "cargo test --workspace --doc",
                "/fake/bin/cargo-deny check",
                "/fake/bin/cargo-machete crates mods xtask",
                "/fake/bin/cargo-audit audit",
                "bunx --no-install --bun prettier --check **/*.{md,yml,yaml,json,js,mjs,cjs,ts}",
                "/fake/bin/shellcheck .githooks/commit-msg .githooks/pre-push",
                "/fake/bin/actionlint -pyflakes= -shellcheck=/fake/bin/shellcheck",
                "/fake/bin/zizmor --no-progress --offline --strict-collection --config \
                 .github/zizmor.yml .github/workflows .github/dependabot.yml",
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
        assert!(
            matches!(probe, Probe::Version(_)),
            "the shellcheck step probes with {probe:?}, which passes at any release"
        );

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
            .find(|line| line.starts_with(&format!("{FAKE_BIN}/actionlint")))
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

    /// An analyzer mise resolves nowhere stops the gate at the step that owns
    /// it, before actionlint is reached. Running actionlint without the flag
    /// would put the lookup back where it started, and a failed lookup there is
    /// silent.
    #[test]
    fn an_analyzer_that_resolves_nowhere_stops_the_gate_before_actionlint() {
        let runner = FakeRunner::all_installed().unresolvable("shellcheck");
        let (rows, text) = gate(&runner);

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "shellcheck");
        assert!(!last.passed());
        assert!(
            !runner
                .ran()
                .iter()
                .any(|line| line.starts_with(&format!("{FAKE_BIN}/actionlint"))),
            "actionlint ran anyway: {:?}",
            runner.ran()
        );
        assert!(text.contains("shellcheck did not run"), "got {text}");
    }

    /// A step that fills a flag with a second tool's path refuses to run when
    /// that path cannot be resolved, rather than dropping the flag.
    ///
    /// The real table never reaches this, because the step that owns the
    /// analyzer runs first and stops the gate. The branch stays, and this is
    /// what holds it: a one-step table whose tool resolves while the tool it
    /// names does not.
    #[test]
    fn a_step_refuses_to_run_when_its_resolved_tool_is_missing() {
        const LONE: &[Step] = &[Step {
            name: "actionlint",
            primary: Run {
                tool: "actionlint",
                mise: true,
                probe: Probe::Command(&["actionlint", "-version"]),
                command: &["actionlint", "-pyflakes="],
                resolved: Some(Resolved {
                    flag: "-shellcheck=",
                    tool: "shellcheck",
                }),
                install: MISE_INSTALL,
            },
            fallback: None,
        }];

        let runner = FakeRunner::all_installed().unresolvable("shellcheck");
        let mut out = Vec::new();
        let rows = Gate::new(LONE, &runner).run(&mut out).expect("write");

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "actionlint");
        assert!(
            matches!(&last.outcome, Outcome::Mismatched(why) if why.contains("shellcheck")),
            "got {:?}",
            last.outcome
        );
        assert!(
            runner.ran().is_empty(),
            "actionlint ran without its analyzer: {:?}",
            runner.ran()
        );
    }

    /// A tool at the wrong release stops the gate before the step that would
    /// have used it, and the summary says which release is which rather than
    /// calling the tool absent.
    #[test]
    fn a_pinned_tool_at_another_release_stops_the_gate() {
        let shellcheck = step("shellcheck");
        let runner = FakeRunner::all_installed().reporting(&shellcheck.primary, "1.2.3");
        let (rows, text) = gate(&runner);

        let last = rows.last().expect("one row");
        assert_eq!(last.step, "shellcheck");
        assert_eq!(
            last.outcome,
            Outcome::Mismatched(format!(
                "shellcheck is 1.2.3 and {} pins {FAKE_RELEASE}",
                pins::PINS
            ))
        );
        assert!(
            !runner
                .ran()
                .iter()
                .any(|line| line.starts_with(&format!("{FAKE_BIN}/actionlint"))),
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

    /// A pin file nothing can read stops the gate at its first row, before any
    /// tool runs, because nothing then says which release anything should be.
    #[test]
    fn an_unreadable_pin_stops_the_gate() {
        let runner = FakeRunner::all_installed().unpinned();
        let (rows, text) = gate(&runner);

        assert_eq!(rows.len(), 1, "a step ran with no pin file to hold it to");
        let first = &rows[0];
        assert_eq!(first.step, "pins");
        assert!(!first.passed());
        assert!(
            text.contains(&format!("{} cannot be read", pins::PINS)),
            "got {text}"
        );
        assert!(runner.ran().is_empty(), "got {:?}", runner.ran());
    }

    /// The pin file gives a version per tool, under the key that tool's binary
    /// is named, whether the entry is the bare version or a table carrying
    /// backend options. A tool no entry names has no version, which is a
    /// refusal rather than a default.
    #[test]
    fn the_release_readers_take_the_shapes_they_name() {
        let document = "\
[tools]
shellcheck = \"0.11.0\"
taplo = \"0.10.0\"
\"github:nextest-rs/nextest\" = { version = \"0.9.145\", version_prefix = \"cargo-nextest-\" }

[settings]
locked = true
";
        let pins = [
            ("shellcheck", Some("0.11.0")),
            ("taplo", Some("0.10.0")),
            ("github:nextest-rs/nextest", Some("0.9.145")),
            // A coordinate is one key, so the name at the end of it is not an
            // entry of its own.
            ("nextest", None),
            ("cargo-nextest", None),
            // A table in another section is not a tool.
            ("locked", None),
            ("", None),
        ];
        for (name, expected) in pins {
            assert_eq!(
                pinned_version(document, name).as_deref(),
                expected,
                "{name:?}"
            );
        }
        assert_eq!(
            pinned_version("this is not toml = = =", "taplo"),
            None,
            "an unparseable pin file reads as no version rather than panicking"
        );
        assert_eq!(
            pinned_version("[settings]\nlocked = true\n", "taplo"),
            None,
            "a pin file with no tools table names no version"
        );

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

        for drop_nextest in [false, true] {
            let mut runner = FakeRunner::all_installed();
            let mut note = "with cargo-nextest";
            if drop_nextest {
                runner = runner.without(&tests.primary);
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
            &command[..1],
            ["cargo-machete"],
            "the machete step runs {command:?}"
        );
        let mut walked: Vec<&str> = command[1..].to_vec();
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
            let runner = FakeRunner::all_installed().failing(&step.primary);
            let (rows, text) = gate(&runner);

            assert_eq!(
                rows.len(),
                index + 2,
                "{} ran the steps after it, counting the pin row that opens the gate",
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
            let mut runner = FakeRunner::all_installed().without(&step.primary);
            if let Some(fallback) = &step.fallback {
                runner = runner.without(fallback);
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
        let runner = FakeRunner::all_installed().without(&step.primary);
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
        // The pin files come from the sound runner, so the row that opens the
        // gate passes and the case reaches the step it is about.
        struct Broken(FakeRunner);
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
            fn read_file(&self, relative: &str) -> Option<String> {
                self.0
                    .read_file(relative)
                    .or_else(|| Some(FAKE_RELEASE.to_string()))
            }
            fn resolve(&self, program: &str) -> Option<PathBuf> {
                Some(PathBuf::from(format!("{FAKE_BIN}/{program}")))
            }
        }

        let mut out = Vec::new();
        let broken = Broken(FakeRunner::all_installed());
        let rows = Gate::new(STEPS, &broken).run(&mut out).expect("write");

        assert_eq!(
            rows.len(),
            2,
            "the pin row opens the gate and fmt follows it"
        );
        assert!(!rows[1].passed());
        assert!(matches!(rows[1].outcome, Outcome::Unstartable(_)));
    }

    /// An empty step table renders a passing gate with nothing in it.
    #[test]
    fn an_empty_step_table_renders() {
        let runner = FakeRunner::all_installed();
        let mut out = Vec::new();
        let steps: &[Step] = &[];
        let rows = Gate::new(steps, &runner).run(&mut out).expect("write");

        assert_eq!(
            rows.len(),
            1,
            "the pin row runs whatever the step table holds"
        );
        assert_eq!(rows[0].step, "pins");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.contains("1 steps passed"), "got {text}");
    }

    /// The fallback stays unused while the first tool answers.
    #[test]
    fn the_fallback_is_not_used_while_the_first_tool_answers() {
        let runner = FakeRunner::all_installed();
        let (rows, _) = gate(&runner);

        let tests = &rows[4];
        assert!(!tests.fell_back);
        assert_eq!(tests.tool, "cargo-nextest");
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
            mise: false,
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
