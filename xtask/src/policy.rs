//! Tests over the repository's own configuration.
//!
//! Each test reads a committed file and asserts a property the supply chain
//! depends on: what continuous integration runs is pinned, what a Claude Code
//! session may write is fenced, and no hook fetches from a registry. They live
//! here because the files they read have no other test.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::repo_root;

/// Every `uses:` reference in a workflow, as `(action, reference, comment)`.
fn workflow_uses(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let rest = trimmed
                .strip_prefix("- uses:")
                .or_else(|| trimmed.strip_prefix("uses:"))?;
            let (value, comment) = rest.split_once('#').unwrap_or((rest, ""));
            let (action, reference) = value.trim().split_once('@')?;
            Some((
                action.to_string(),
                reference.trim().to_string(),
                comment.trim().to_string(),
            ))
        })
        .collect()
}

/// Every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

/// Collect every dependency name a manifest takes from the workspace table.
///
/// Walks the whole document, so `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]` and the target-specific tables all count.
fn workspace_inherited(value: &toml::Value, found: &mut BTreeSet<String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (key, child) in table {
        if matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            let Some(deps) = child.as_table() else {
                continue;
            };
            for (name, spec) in deps {
                if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    found.insert(name.clone());
                }
            }
        } else {
            workspace_inherited(child, found);
        }
    }
}

/// The events of one `##` section of a Markdown document, from the block after
/// its heading to the block before the next heading at the same level or above.
/// `None` when the document has no such heading.
///
/// A heading is a stabler anchor than the sentence under it, so a section can be
/// reworded without moving what reads it.
fn section<'a>(markdown: &'a str, heading: &str) -> Option<Vec<Event<'a>>> {
    let events: Vec<Event<'a>> = Parser::new_ext(markdown, Options::ENABLE_TABLES).collect();
    let mut index = 0;
    while index < events.len() {
        if let Event::Start(Tag::Heading {
            level: HeadingLevel::H2,
            ..
        }) = &events[index]
        {
            let close = index
                + events[index..]
                    .iter()
                    .position(|event| matches!(event, Event::End(TagEnd::Heading(_))))?;
            let title: String = events[index + 1..close]
                .iter()
                .filter_map(text_of)
                .collect();
            if title == heading {
                let body = &events[close + 1..];
                let end = body
                    .iter()
                    .position(|event| {
                        matches!(event, Event::Start(Tag::Heading { level, .. }) if *level <= HeadingLevel::H2)
                    })
                    .unwrap_or(body.len());
                return Some(body[..end].to_vec());
            }
            index = close;
        }
        index += 1;
    }
    None
}

/// The text an inline event carries, code spans included.
fn text_of<'e>(event: &'e Event<'_>) -> Option<&'e str> {
    match event {
        Event::Text(text) | Event::Code(text) => Some(&**text),
        _ => None,
    }
}

/// Every paragraph in `events` that is a bare list of inline code spans, as the
/// spans it holds.
///
/// Anchoring on the paragraph's shape rather than on the sentence above it means
/// rewording the section around it leaves the check working. A bullet is not a
/// paragraph, so a bulleted list of code spans is not read.
fn code_span_lists(events: &[Event<'_>]) -> Vec<Vec<String>> {
    let mut lists: Vec<Vec<String>> = Vec::new();
    let mut spans: Vec<String> = Vec::new();
    let mut bare = true;
    let mut inside = false;
    for event in events {
        match event {
            Event::Start(Tag::Paragraph) => {
                inside = true;
                bare = true;
                spans.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                inside = false;
                if bare && !spans.is_empty() {
                    lists.push(std::mem::take(&mut spans));
                }
            }
            Event::Code(code) if inside => spans.push(code.to_string()),
            Event::Text(text) if inside => {
                bare &= text
                    .chars()
                    .all(|c| c == ',' || c == '.' || c.is_whitespace());
            }
            Event::SoftBreak | Event::HardBreak => {}
            _ if inside => bare = false,
            _ => {}
        }
    }
    lists
}

/// The first cell of every body row of every table in `events`, as text with the
/// code markers dropped.
fn first_column(events: &[Event<'_>]) -> Vec<String> {
    let mut column: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut first_cell_of_row = false;
    let mut in_head = false;
    for event in events {
        match event {
            Event::Start(Tag::TableHead) => in_head = true,
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) => first_cell_of_row = true,
            Event::Start(Tag::TableCell) => cell.clear(),
            Event::End(TagEnd::TableCell) => {
                if first_cell_of_row && !in_head {
                    column.push(std::mem::take(&mut cell));
                }
                first_cell_of_row = false;
            }
            Event::Text(text) | Event::Code(text) => cell.push_str(text),
            _ => {}
        }
    }
    column
}

/// Number words the prose check reads as a count.
///
/// "One" is left out, because a sentence naming one step names a step rather
/// than counting the gate's. Ordinals are left out, because "the first step" is
/// a position.
const COUNT_WORDS: &[&str] = &[
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "dozen",
];

/// The tens a hyphenated count opens with, as in "twenty-one".
const TENS_WORDS: &[&str] = &[
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// The units a hyphenated count closes with, as in "twenty-one".
const UNIT_WORDS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// The nouns whose count the prose check refuses when the count comes first:
/// the gate's steps under each name prose gives them, and the pinned tools.
const COUNTED_NOUNS: &[&str] = &[
    "step", "steps", "stage", "stages", "check", "checks", "command", "commands", "tool", "tools",
];

/// The nouns the check also reads with the count after them, as in
/// `Steps: 13`. "Checks" and "commands" are left out, because they are verbs
/// as often as nouns, as in `the gate checks two things`.
const COUNTED_BEFORE: &[&str] = &["steps", "stages", "tools"];

/// Whether `word` is a count: a run of up to three digits, a number word from
/// two up, or a hyphenated tens and unit. A longer run of digits is an id or a
/// measurement, never a count of steps or tools.
fn is_count(word: &str) -> bool {
    if (1..=3).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if COUNT_WORDS.contains(&word) {
        return true;
    }
    word.split_once('-')
        .is_some_and(|(tens, unit)| TENS_WORDS.contains(&tens) && UNIT_WORDS.contains(&unit))
}

/// The clauses of `prose`, each as its lowercase words.
///
/// An inline code span reads as the one word `code`, so a command name between
/// a count and its noun is not three words of distance, and a count quoted as
/// code is literal text rather than a claim. A run of three or more backticks
/// is a fence rather than a span, so the text inside a fenced block is read. A
/// sentence ends at `.`, `!` or `?` followed by whitespace, and a clause at
/// `;`, `,` and brackets. A dot inside a word, as in `crates.io`, ends nothing.
/// Colons and table pipes join, so a count after a colon or in the next table
/// cell reads in the same clause. Apostrophes stay inside a word, so `gate's`
/// is one word.
fn clauses(prose: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = prose.chars().collect();
    let mut plain = String::with_capacity(prose.len());
    let mut in_code = false;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`' {
            let run = chars[index..].iter().take_while(|c| **c == '`').count();
            if run < 3 {
                if !in_code {
                    plain.push_str(" code ");
                }
                in_code = !in_code;
            } else {
                plain.push(' ');
            }
            index += run;
            continue;
        }
        if !in_code {
            plain.push(chars[index]);
        }
        index += 1;
    }
    let plain = paths_as_words(&plain);

    let chars: Vec<char> = plain.chars().collect();
    let mut clauses: Vec<Vec<String>> = Vec::new();
    let mut current = String::new();
    for (index, c) in chars.iter().enumerate() {
        let ends_sentence = matches!(c, '.' | '!' | '?')
            && chars.get(index + 1).is_none_or(|next| next.is_whitespace());
        if ends_sentence || ";,()[]".contains(*c) {
            clauses.push(words(&current));
            current.clear();
        } else {
            current.push(*c);
        }
    }
    clauses.push(words(&current));
    clauses
}

/// `text` with every whitespace-separated token that holds a `/` read as the
/// one word `path`, so a file path or a URL between two words is not several
/// words, and a directory name in it is not a noun. Punctuation closing the
/// token stays, so a sentence ending in a path still ends.
fn paths_as_words(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            if !token.contains('/') {
                return token.to_string();
            }
            let closing: String = token
                .chars()
                .rev()
                .take_while(|c| ".,;!?)]".contains(*c))
                .collect::<Vec<char>>()
                .into_iter()
                .rev()
                .collect();
            format!("path{closing}")
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The lowercase words of a clause, split at anything that is not a letter, a
/// digit, a hyphen or an apostrophe.
fn words(clause: &str) -> Vec<String> {
    clause
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '\''))
        .map(|word| word.trim_matches(['\'', '-']))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Every phrase in `prose` that states how many gate steps or pinned tools there
/// are.
///
/// Three shapes count, each inside one clause:
///
/// - a count, then one of [`COUNTED_NOUNS`] with at most two words between
/// - one of [`COUNTED_BEFORE`] with a count straight after it, as in
///   `Steps: 13`, or "step count" or "tool count" then a count with at most two
///   words between. A plural noun takes no gap, because "steps" is also a verb.
/// - a count hyphenated to one of [`COUNTED_NOUNS`], as in `eleven-step`
fn stated_counts(prose: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for words in clauses(prose) {
        for (index, word) in words.iter().enumerate() {
            let following = |from: usize| words.iter().skip(from).take(3);
            if let Some((head, tail)) = word.rsplit_once('-')
                && is_count(head)
                && COUNTED_NOUNS.contains(&tail)
            {
                found.push(word.clone());
            } else if is_count(word)
                && let Some(offset) =
                    following(index + 1).position(|next| COUNTED_NOUNS.contains(&next.as_str()))
            {
                found.push(words[index..=index + 1 + offset].join(" "));
            } else {
                let (noun_end, reach) = if COUNTED_BEFORE.contains(&word.as_str()) {
                    (Some(index), 1)
                } else if matches!(word.as_str(), "step" | "tool")
                    && words.get(index + 1).is_some_and(|next| next == "count")
                {
                    (Some(index + 1), 3)
                } else {
                    (None, 0)
                };
                if let Some(end) = noun_end
                    && let Some(offset) = words
                        .iter()
                        .skip(end + 1)
                        .take(reach)
                        .position(|next| is_count(next))
                {
                    found.push(words[index..=end + 1 + offset].join(" "));
                }
            }
        }
    }
    found
}

/// The text of a `//`, `///`, `//!` or block comment line, markers removed.
fn slash_comment(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    (trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*"))
        .then(|| trimmed.trim_start_matches(['/', '*', '!']))
}

/// The text of a `#` comment, whether it opens the line or trails a value.
fn hash_comment(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix('#')
        .or_else(|| line.split_once(" #").map(|(_, rest)| rest))
}

/// The value of a YAML `name:` key, which the Actions page shows as prose.
fn yaml_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    let value = trimmed.strip_prefix("name:")?;
    let value = value.split_once(" #").map_or(value, |(before, _)| before);
    Some(value.trim().trim_matches(['"', '\'']))
}

/// How the prose check reads a file, or `None` when the file carries no prose
/// it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProseKind {
    /// Markdown and the issue forms, read whole, code blocks included.
    Whole,
    /// Rust, TypeScript and JavaScript, read for their comment lines.
    SlashComments,
    /// YAML, read for its `#` comments and its `name:` values.
    Yaml,
    /// TOML and the extensionless hook, pin and ignore files, read for their
    /// `#` comments.
    HashComments,
}

/// The way the prose check reads `path`.
fn prose_kind(path: &Path) -> Option<ProseKind> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let in_forms = path
        .components()
        .any(|part| part.as_os_str() == "ISSUE_TEMPLATE");
    if extension == "md" || in_forms {
        return Some(ProseKind::Whole);
    }
    match extension {
        "rs" | "ts" | "js" | "mjs" | "cjs" => Some(ProseKind::SlashComments),
        "yml" | "yaml" => Some(ProseKind::Yaml),
        "toml" | "" => Some(ProseKind::HashComments),
        _ => None,
    }
}

/// The prose `text` carries, as paragraphs, given the file it came from.
///
/// Consecutive comment lines join into one paragraph, so a phrase wrapped
/// across two of them reads whole. A YAML `name:` value is a paragraph of its
/// own. A file [`prose_kind`] does not read yields nothing.
fn prose_paragraphs(path: &Path, text: &str) -> Vec<String> {
    let Some(kind) = prose_kind(path) else {
        return Vec::new();
    };
    if kind == ProseKind::Whole {
        return text.split("\n\n").map(str::to_string).collect();
    }
    let comment: fn(&str) -> Option<&str> = match kind {
        ProseKind::SlashComments => slash_comment,
        _ => hash_comment,
    };
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if kind == ProseKind::Yaml
            && let Some(name) = yaml_name(line)
        {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            paragraphs.push(name.to_string());
            continue;
        }
        match comment(line).map(str::trim) {
            Some(prose) if !prose.is_empty() => {
                current.push(' ');
                current.push_str(prose);
            }
            _ if !current.is_empty() => paragraphs.push(std::mem::take(&mut current)),
            _ => {}
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
}

/// Every file under `dir` the prose check reads, past the directories that hold
/// build output, installed packages, fetched data, vendored checkouts and git's
/// own store.
fn prose_files(dir: &Path) -> Vec<PathBuf> {
    const SKIPPED: &[&str] = &[".git", ".cache", "node_modules", "target", "vendor"];
    let mut found: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIPPED.contains(&name));
            if !skipped {
                found.extend(prose_files(&path));
            }
        } else {
            found.push(path);
        }
    }
    found
}

/// The package an install command names, for a command that is a
/// `cargo install`.
///
/// The flag and the package can come in either order, so the package is the
/// first word past `cargo install` that is not a flag.
fn cargo_install_package(install: &str) -> Option<&str> {
    let mut words = install.split_whitespace();
    if words.next()? != "cargo" || words.next()? != "install" {
        return None;
    }
    words.find(|word| !word.starts_with('-'))
}

/// Report whether `source` uses `identifier` as a whole word outside comments.
fn mentions(source: &str, identifier: &str) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    source.lines().any(|line| {
        let code = line.split("//").next().unwrap_or("");
        code.match_indices(identifier).any(|(start, _)| {
            let before = code[..start].chars().next_back();
            let after = code[start + identifier.len()..].chars().next();
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use yaml_rust2::{Yaml, YamlLoader};

    use super::{
        cargo_install_package, code_span_lists, first_column, mentions, prose_files, prose_kind,
        prose_paragraphs, repo_root, rust_sources, section, stated_counts, workflow_uses,
        workspace_inherited,
    };

    fn read(relative: &str) -> String {
        let path = repo_root().join(relative);
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    /// A YAML file under the repository root, parsed as its one document.
    fn yaml(relative: &str) -> Yaml {
        let mut documents = YamlLoader::load_from_str(&read(relative))
            .unwrap_or_else(|err| panic!("{relative} is not YAML: {err}"));
        assert_eq!(
            documents.len(),
            1,
            "{relative} holds {} documents, and a reader takes the first",
            documents.len()
        );
        documents.remove(0)
    }

    /// A commit pin is a hash, and nothing about a hash says which release it
    /// is. Dependabot reads the version comment to know what it is bumping
    /// from, and writes the new one back, so an action without the comment
    /// stops being bumped at all.
    ///
    /// The pin itself is `zizmor`'s `unpinned-uses`, whose blanket policy is
    /// hash-pin. The comment is what no tool checks.
    #[test]
    fn every_action_carries_the_version_comment_dependabot_bumps() {
        let uses = workflow_uses(&read(".github/workflows/ci.yml"));
        assert!(!uses.is_empty(), "the workflow declares no actions");

        for (action, _reference, comment) in uses {
            assert!(
                comment.starts_with('v') && comment[1..].starts_with(|c: char| c.is_ascii_digit()),
                "{action} carries no version comment, got {comment:?}"
            );
        }
    }

    /// The token stays read-only, so a compromised action on a runner cannot
    /// push, release or write to the repository.
    ///
    /// `zizmor`'s `excessive-permissions` does not stand in for this. At the
    /// regular persona it reports a workflow that declares no `permissions`
    /// block at all and says nothing about one that declares `write-all`, which
    /// is the change this is here to refuse.
    #[test]
    fn ci_declares_read_only_permissions() {
        let text = read(".github/workflows/ci.yml");
        let permissions = text
            .lines()
            .position(|line| line.trim_end() == "permissions:")
            .expect("the workflow declares permissions");
        let scope = text.lines().nth(permissions + 1).unwrap_or("");

        assert_eq!(scope.trim(), "contents: read", "got {scope:?}");
    }

    /// The Bun release is resolved at run time unless it is pinned, and a
    /// resolved release is under no cooldown.
    #[test]
    fn ci_pins_the_bun_release() {
        let text = read(".github/workflows/ci.yml");

        assert!(
            text.contains("bun-version: 1.3.13"),
            "setup-bun does not pin the Bun release"
        );
    }

    /// actionlint is a Go program, and neither runner image puts a Go on `PATH`
    /// that builds it. The gate job installs one with `actions/setup-go` before
    /// the step that reads `.github/go-tools`, at an exact release, because a
    /// range resolves at run time and a resolved release is under no cooldown.
    /// That step has to stop on a failed install, or the gate reports the tool
    /// missing one step later and names the wrong cause.
    #[test]
    fn the_go_tools_install_under_an_exact_go_release() {
        let workflow = yaml(".github/workflows/ci.yml");
        let steps = workflow["jobs"]["gate"]["steps"]
            .as_vec()
            .expect("the gate job lists steps");

        let setup = steps
            .iter()
            .position(|step| {
                step["uses"]
                    .as_str()
                    .is_some_and(|uses| uses.starts_with("actions/setup-go@"))
            })
            .expect("the gate job installs Go with actions/setup-go");
        let install = steps
            .iter()
            .position(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains(".github/go-tools"))
            })
            .expect("a gate step installs from .github/go-tools");
        assert!(
            setup < install,
            "setup-go runs after the step that needs its Go"
        );

        let options = &steps[setup]["with"];
        let version = options["go-version"].as_str().unwrap_or("");
        let exact = version.split('.').count() == 3
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        assert!(
            exact,
            "setup-go takes {:?} as go-version, which is not one exact release",
            options["go-version"]
        );
        assert_eq!(
            options["cache"].as_bool(),
            Some(false),
            "setup-go caches on a go.sum this repository does not have"
        );

        let script = steps[install]["run"].as_str().unwrap_or("");
        assert!(
            script.contains("$LASTEXITCODE -ne 0"),
            "the install step does not stop on a failed go install:\n{script}"
        );
    }

    /// Dependabot's cooldown is the wait between a version being published and
    /// a pull request proposing it, and an ecosystem with no cooldown block
    /// waits for nothing. zizmor's `dependabot-cooldown` refuses a cooldown
    /// shorter than `.github/zizmor.yml` sets, and passes a block removed
    /// entirely, so this reads every entry.
    #[test]
    fn every_dependabot_ecosystem_carries_a_cooldown_zizmor_can_hold() {
        let threshold =
            yaml(".github/zizmor.yml")["rules"]["dependabot-cooldown"]["config"]["days"]
                .as_i64()
                .expect(".github/zizmor.yml sets the dependabot-cooldown threshold in days");
        let config = yaml(".github/dependabot.yml");
        let updates = config["updates"]
            .as_vec()
            .expect(".github/dependabot.yml lists updates");
        assert!(
            !updates.is_empty(),
            ".github/dependabot.yml lists no update"
        );

        let short: Vec<String> = updates
            .iter()
            .filter_map(|update| {
                let ecosystem = update["package-ecosystem"]
                    .as_str()
                    .unwrap_or("an unnamed ecosystem");
                match update["cooldown"]["default-days"].as_i64() {
                    Some(days) if days >= threshold => None,
                    Some(days) => Some(format!("{ecosystem} waits {days} days")),
                    None => Some(format!("{ecosystem} carries no cooldown default-days")),
                }
            })
            .collect();
        assert!(
            short.is_empty(),
            "every ecosystem waits at least the {threshold} days .github/zizmor.yml holds it to: {short:?}"
        );
    }

    /// The inline zizmor ignore comments this repository has decided on, as
    /// `(file, audit)`. None today.
    const ALLOWED_ZIZMOR_IGNORES: &[(&str, &str)] = &[];

    /// The opening of an inline zizmor ignore comment. zizmor matches this exact
    /// spelling, one space and all, and skips an empty entry between commas.
    const MARKER: &str = "zizmor: ignore[";

    /// An inline `zizmor: ignore[...]` comment answers a finding with no review
    /// beyond the diff that adds it, and a rule set to ignore or disable in
    /// `.github/zizmor.yml` answers every finding of that audit. Each inline
    /// answer has to be on the allowlist above, and the configuration may only
    /// set thresholds.
    #[test]
    fn zizmor_answers_only_the_findings_this_repository_allows() {
        let root = repo_root().join(".github");
        let mut found: Vec<(String, String)> = Vec::new();
        for path in prose_files(&root) {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{} cannot be read: {err}", path.display()));
            let file = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            for (at, _) in text.match_indices(MARKER) {
                let rest = &text[at + MARKER.len()..];
                let audits = rest.split(']').next().unwrap_or("");
                for audit in audits.split(',').map(str::trim).filter(|a| !a.is_empty()) {
                    found.push((file.clone(), audit.to_string()));
                }
            }
        }
        found.sort();
        let mut allowed: Vec<(String, String)> = ALLOWED_ZIZMOR_IGNORES
            .iter()
            .map(|(file, audit)| ((*file).to_string(), (*audit).to_string()))
            .collect();
        allowed.sort();
        assert_eq!(
            found, allowed,
            "the inline zizmor ignore comments under .github and the allowlist disagree"
        );

        let config = yaml(".github/zizmor.yml");
        let rules = config["rules"]
            .as_hash()
            .expect(".github/zizmor.yml holds rules");
        let answering: Vec<String> = rules
            .iter()
            .filter(|(_, rule)| !rule["ignore"].is_badvalue() || !rule["disable"].is_badvalue())
            .map(|(name, _)| name.as_str().unwrap_or("?").to_string())
            .collect();
        assert!(
            answering.is_empty(),
            ".github/zizmor.yml ignores or disables {answering:?}, which answers every finding of those audits"
        );
    }

    /// Every tool the gate installs with `cargo install` carries a version, in
    /// one file. A second copy of a version is a copy that drifts.
    ///
    /// The check runs both ways. A step whose tool is unpinned installs
    /// whatever the registry serves today, and a pinned entry no step installs
    /// is a version continuous integration fetches for nothing.
    #[test]
    fn the_pinned_tool_file_and_the_gate_name_the_same_tools() {
        let pinned: Vec<(String, String)> = pinned_tools();
        assert!(!pinned.is_empty(), "the pinned tool file names nothing");

        let mut installed: BTreeSet<String> = BTreeSet::new();
        for step in crate::check::STEPS {
            for run in std::iter::once(&step.primary).chain(step.fallback.as_ref()) {
                let Some(package) = cargo_install_package(run.install) else {
                    continue;
                };
                assert_eq!(
                    package, run.tool,
                    "the {} step names {} and installs {package}",
                    step.name, run.tool
                );
                let found = pinned.iter().filter(|(name, _)| name == package).count();
                assert_eq!(
                    found, 1,
                    "{package} appears {found} times in .github/cargo-tools"
                );
                installed.insert(package.to_string());
            }
        }

        let unused: Vec<&String> = pinned
            .iter()
            .map(|(name, _)| name)
            .filter(|name| !installed.contains(*name))
            .collect();
        assert!(
            unused.is_empty(),
            "no gate step installs {unused:?} from .github/cargo-tools"
        );
    }

    /// The flag and the package name come in either order, and a step that
    /// installs through rustup or bun names no package at all.
    #[test]
    fn cargo_install_package_reads_either_argument_order() {
        let cases = [
            ("cargo install cargo-deny --locked", Some("cargo-deny")),
            ("cargo install --locked cargo-deny", Some("cargo-deny")),
            ("cargo install taplo-cli --locked", Some("taplo-cli")),
            ("rustup component add rustfmt", None),
            ("rustup toolchain install stable", None),
            ("bun install", None),
            ("cargo install", None),
            ("cargo install --locked", None),
            ("", None),
        ];

        for (install, expected) in cases {
            assert_eq!(cargo_install_package(install), expected, "{install:?}");
        }
    }

    /// The workflow reads the pinned files rather than carrying its own copy of
    /// the versions.
    #[test]
    fn ci_reads_the_pinned_tool_files() {
        let text = read(".github/workflows/ci.yml");

        for file in [".github/cargo-tools", ".github/go-tools"] {
            assert!(text.contains(file), "the workflow does not read {file}");
        }
        for (tool, version) in pinned_tools() {
            assert!(
                !text.contains(&format!("{tool}@{version}")),
                "the workflow carries its own copy of {tool}@{version}"
            );
        }
        for entry in pinned_go_tools() {
            assert!(
                !text.contains(&entry),
                "the workflow carries its own copy of {entry}"
            );
        }
    }

    /// The gate names the tool it needs and the workflow installs it, so the
    /// version has to be the same one in both places. A gate asking for one
    /// actionlint while continuous integration installs another is a gate whose
    /// findings nobody can reproduce.
    ///
    /// `cargo_install_package` reads the crates.io half of this. It returns
    /// nothing for a `go install`, which is why this is a check of its own.
    #[test]
    fn the_pinned_go_tool_file_and_the_gate_name_the_same_versions() {
        let pinned = pinned_go_tools();
        assert!(!pinned.is_empty(), "the pinned Go tool file names nothing");

        let installs: Vec<&str> = crate::check::STEPS
            .iter()
            .flat_map(|step| std::iter::once(&step.primary).chain(step.fallback.as_ref()))
            .map(|run| run.install)
            .filter(|install| install.starts_with("go install "))
            .collect();

        for entry in &pinned {
            let wanted = format!("go install {entry}");
            assert!(
                installs.contains(&wanted.as_str()),
                "no gate step installs {entry}, and the steps that use `go install` are {installs:?}"
            );
        }
        assert_eq!(
            installs.len(),
            pinned.len(),
            "the gate has {} `go install` steps and the pinned file names {}",
            installs.len(),
            pinned.len()
        );
    }

    /// A document that restates a version is a copy that drifts, so the docs
    /// point at the file instead.
    #[test]
    fn no_document_restates_a_pinned_tool_version() {
        let pinned = pinned_tools();
        let go = pinned_go_tools();

        for doc in ["CONTRIBUTING.md", "docs/dev.md", "README.md"] {
            let text = read(doc);
            for (tool, version) in &pinned {
                assert!(
                    !text.contains(&format!("{tool}@{version}")),
                    "{doc} restates {tool}@{version}"
                );
            }
            for entry in &go {
                assert!(!text.contains(entry), "{doc} restates {entry}");
            }
        }
    }

    /// `.github/commit-scopes.json` is the scope vocabulary. `cargo xtask
    /// scopes` prints it and `commitlint.config.js` enforces it, both by reading
    /// the file. `CONTRIBUTING.md` restates it for a reader, and that
    /// restatement is the copy this holds to the file.
    #[test]
    fn every_copy_of_the_scope_list_agrees() {
        let declared = crate::scopes().expect(".github/commit-scopes.json parses");
        assert!(
            !declared.is_empty(),
            ".github/commit-scopes.json declares no scopes"
        );

        let contributing = read("CONTRIBUTING.md");
        let commits = section(&contributing, "Commit messages")
            .expect("CONTRIBUTING.md has a Commit messages section");
        let documented = code_span_lists(&commits);
        assert_eq!(
            documented.len(),
            1,
            "the Commit messages section holds {} paragraphs that are a bare list of code spans, so which one restates the scopes is ambiguous",
            documented.len()
        );
        assert_eq!(
            documented[0], declared,
            "CONTRIBUTING.md restates {:?} and .github/commit-scopes.json holds {declared:?}",
            documented[0]
        );
    }

    /// `commitlint.config.js` with its comment lines dropped.
    fn commitlint_code() -> String {
        read("commitlint.config.js")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<&str>>()
            .join("\n")
    }

    /// How many times `code` uses `name` as an identifier. A use is the name
    /// with no identifier character or hyphen before it and no identifier
    /// character after it, so a file path holding the name is not one.
    fn identifier_uses(code: &str, name: &str) -> usize {
        let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        code.match_indices(name)
            .filter(|(at, _)| {
                let before = code[..*at].chars().next_back();
                let after = code[at + name.len()..].chars().next();
                !before.is_some_and(|c| is_ident(c) || c == '-') && !after.is_some_and(is_ident)
            })
            .count()
    }

    /// The commit hook enforces the scope file only while the rule reads it and
    /// nothing else. A literal beside the list, a rule turned down to a
    /// warning or off, or a list extended after it loads each lets commitlint
    /// accept a scope `cargo xtask scopes` never prints.
    ///
    /// The config is JavaScript and nothing in this suite runs JavaScript, so
    /// the rule and the line that loads the list are held to their exact text
    /// with whitespace and trailing commas dropped, which is the part Prettier
    /// rewrites. The list's name has to appear exactly where those two lines
    /// put it, so nothing else in the config can read or change it.
    #[test]
    fn commitlint_enforces_the_scope_file_and_nothing_beside_it() {
        let code = commitlint_code();
        let compact: String = code
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace(",]", "]")
            .replace(",)", ")");

        for (what, expected) in [
            ("the rule", r#""scope-enum":[2,"always",scopes]"#),
            (
                "the line that loads the list",
                r#"constscopes=JSON.parse(readFileSync(newURL(".github/commit-scopes.json",import.meta.url),"utf8"))"#,
            ),
        ] {
            assert_eq!(
                compact.matches(expected).count(),
                1,
                "commitlint.config.js does not hold {what} exactly once, as {expected}"
            );
        }
        assert_eq!(
            compact.matches("scope-enum").count(),
            1,
            "commitlint.config.js names scope-enum more than once"
        );
        let uses = identifier_uses(&code, "scopes");
        assert_eq!(
            uses, 2,
            "commitlint.config.js names the list {uses} times, so something beside the rule reads or changes it"
        );
    }

    /// The identifier count reads a name, and neither a path holding it nor a
    /// longer name.
    #[test]
    fn identifier_uses_reads_a_name_and_nothing_holding_it() {
        let cases = [
            ("const scopes = 1;", 1),
            ("[2, \"always\", scopes]", 1),
            ("[...scopes, \"docs\"]", 1),
            ("scopes.push(\"docs\")", 1),
            ("\".github/commit-scopes.json\"", 0),
            ("const allScopes = scopesList;", 0),
            ("", 0),
        ];

        for (code, expected) in cases {
            assert_eq!(identifier_uses(code, "scopes"), expected, "{code}");
        }
    }

    /// `CONTRIBUTING.md` lists the gate's steps for a reader, and that table is
    /// the one copy outside the step table itself. A step added, renamed,
    /// dropped or moved has to move there too.
    #[test]
    fn the_documented_gate_table_names_every_step_in_order() {
        let contributing = read("CONTRIBUTING.md");
        let gate =
            section(&contributing, "The gate").expect("CONTRIBUTING.md has a The gate section");
        let documented = first_column(&gate);
        let declared: Vec<&str> = crate::check::STEPS.iter().map(|step| step.name).collect();

        assert_eq!(
            documented, declared,
            "the CONTRIBUTING.md gate table and the step table in check.rs disagree"
        );
    }

    /// A count of gate steps or pinned tools in prose goes stale the next time
    /// a step or a tool is added, and nothing else notices. The authoritative
    /// lists are `cargo xtask check`, `.github/cargo-tools` and
    /// `.github/go-tools`, and prose names those rather than counting them.
    ///
    /// Markdown and the issue forms are read whole, code blocks included, and
    /// code, configuration and the workflows contribute their comments, and a
    /// workflow contributes its `name:` values, which the Actions page shows. A
    /// file of a kind the check reads that is not UTF-8 fails it, rather than
    /// being skipped. What passes: a count of anything else, a singular, an
    /// ordinal, a count more than two words from its noun, and code outside a
    /// comment, so a test asserting a rendered summary line is not read as a
    /// claim about the gate.
    /// Ember's checkout under `vendor/` is Ember's to keep, and its own gate
    /// runs the same check.
    #[test]
    fn no_prose_states_a_count_of_gate_steps_or_pinned_tools() {
        let root = repo_root();
        let mut read_files = 0;
        let mut stated: Vec<String> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();
        for path in prose_files(&root) {
            if prose_kind(&path).is_none() {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                unreadable.push(
                    path.strip_prefix(&root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
                continue;
            };
            read_files += 1;
            let shown = path.strip_prefix(&root).unwrap_or(&path).display();
            for paragraph in prose_paragraphs(&path, &text) {
                for phrase in stated_counts(&paragraph) {
                    stated.push(format!("{shown}: {phrase}"));
                }
            }
        }

        assert!(read_files > 0, "no file was read, so nothing was checked");
        assert!(
            unreadable.is_empty(),
            "these files carry prose and are not UTF-8, so nothing here read them: {unreadable:?}"
        );
        assert!(
            stated.is_empty(),
            "prose states a count of gate steps or pinned tools, which goes stale the next \
             time one is added. Name the command or the file that lists them instead: {stated:#?}"
        );
    }

    /// The phrase shapes the prose check refuses, and the ones it lets through.
    #[test]
    fn stated_counts_reads_the_shapes_it_names() {
        let cases: &[(&str, &[&str])] = &[
            ("Eleven steps, in order", &["eleven steps"]),
            ("The gate calls seven\ntools that rustup", &["seven tools"]),
            ("13 steps passed", &["13 steps"]),
            ("Ember's two extra steps", &["two extra steps"]),
            (
                "the three supply chain steps",
                &["three supply chain steps"],
            ),
            ("an eleven-step gate", &["eleven-step"]),
            ("five pinned tools", &["five pinned tools"]),
            ("Steps: 13", &["steps 13"]),
            ("| Steps | 13 |", &["steps 13"]),
            (
                "the gate's step count is thirteen",
                &["step count is thirteen"],
            ),
            ("all 13 `cargo xtask check` steps pass", &["13 code steps"]),
            ("the six crates.io tools", &["six crates io tools"]),
            ("twenty-one steps", &["twenty-one steps"]),
            ("thirty stages", &["thirty stages"]),
            ("a twenty-one-step gate", &["twenty-one-step"]),
            ("The gate runs thirteen checks", &["thirteen checks"]),
            ("nine commands run in order", &["nine commands"]),
            ("Step 1 installs Rust", &[]),
            ("The gate checks two things", &[]),
            ("The scan steps through `.rdata` eight bytes at a time", &[]),
            ("bun run tools/watch-builds.ts 2278520 appinfo.txt", &[]),
            ("See docs/dev.md. Steps run in order", &[]),
            ("one step at a time", &[]),
            ("the first step", &[]),
            ("It has nine. Steps run in order", &[]),
            ("two of the many tools", &[]),
            ("13 libraries and 344 functions", &[]),
            ("the four before it", &[]),
            ("a three-way merge", &[]),
            ("the steps cargo xtask check runs", &[]),
            ("", &[]),
        ];

        for (prose, expected) in cases {
            assert_eq!(stated_counts(prose), *expected, "{prose:?}");
        }
    }

    /// Markdown reads whole, code contributes its comments alone, and a comment
    /// wrapped across lines reads as one paragraph.
    #[test]
    fn prose_paragraphs_reads_each_kind_of_file() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "a.rs",
                "/// runs eleven\n/// steps\nconst NINE: &str = \"nine steps\";\n",
                &["eleven steps"],
            ),
            (
                "a.yml",
                "# seven tools\nrun: echo nine steps\n",
                &["seven tools"],
            ),
            ("a.toml", "key = 1 # five tools\n", &["five tools"]),
            (
                "ci.yml",
                "      - name: Run the eleven gate steps # the gate\n        run: echo nine steps\n",
                &["eleven gate steps"],
            ),
            (
                "a.md",
                "Nine\nsteps.\n\n```text\n13 steps passed\n```\n",
                &["nine steps", "13 steps"],
            ),
            (
                ".github/ISSUE_TEMPLATE/bug.yml",
                "description: the gate's nine steps\n",
                &["nine steps"],
            ),
            ("a.json", "{\"note\": \"nine steps\"}", &[]),
        ];

        for (path, text, expected) in cases {
            let found: Vec<String> = prose_paragraphs(Path::new(path), text)
                .iter()
                .flat_map(|paragraph| stated_counts(paragraph))
                .collect();
            assert_eq!(found, *expected, "{path}");
        }
    }

    /// A paragraph of prose holding code spans is not a list, a bullet is not a
    /// bare paragraph, a list wrapped across lines still reads, a table yields
    /// its first column alone, and a section ends at the next heading of its
    /// level.
    #[test]
    fn the_markdown_readers_take_the_shapes_they_name() {
        let markdown = "# Title\n\n## First\n\nRun `cargo xtask scopes` for the live list.\n\n\
                        `one`, `two`,\n`three`.\n\n- `four`, `five`\n\n### Inside\n\n`six`.\n\n\
                        ## Second\n\n| Crate | Holds |\n| --- | --- |\n| `alpha` | The first |\n\
                        | `beta` | The second |\n\n## Third\n\n`seven`.\n";

        let first = section(markdown, "First").expect("a First section");
        assert_eq!(
            code_span_lists(&first),
            [vec!["one", "two", "three"], vec!["six"]]
        );
        assert!(
            first_column(&first).is_empty(),
            "the First section holds no table"
        );

        let second = section(markdown, "Second").expect("a Second section");
        assert_eq!(first_column(&second), ["alpha", "beta"]);
        assert!(
            code_span_lists(&second).is_empty(),
            "a table is not a paragraph"
        );

        assert!(section(markdown, "Fourth").is_none());
        assert!(
            section(markdown, "Inside").is_none(),
            "only a level-two heading opens a section"
        );
    }

    /// Ember's checkout under `vendor/` is formatted by Ember's own gate, so
    /// the TOML formatter here never rewrites a file this repository does not
    /// own.
    #[test]
    fn the_toml_formatter_never_reaches_the_vendored_checkout() {
        let config: toml::Value = toml::from_str(&read(".taplo.toml")).expect(".taplo.toml parses");
        let exclude: Vec<&str> = config
            .get("exclude")
            .and_then(toml::Value::as_array)
            .expect(".taplo.toml declares an exclude list")
            .iter()
            .filter_map(toml::Value::as_str)
            .collect();

        assert!(
            exclude.iter().any(|pattern| pattern.starts_with("vendor/")),
            "no exclude pattern covers vendor/, got {exclude:?}"
        );
    }

    /// The workspace members, as paths relative to the repository root.
    fn members(root: &toml::Value) -> Vec<String> {
        let members: Vec<String> = root["workspace"]["members"]
            .as_array()
            .expect("the workspace lists members")
            .iter()
            .map(|member| member.as_str().expect("a member path").to_string())
            .collect();
        assert!(!members.is_empty(), "the workspace lists no members");
        members
    }

    /// The pinned tools, as `(name, version)`.
    fn pinned_tools() -> Vec<(String, String)> {
        read(".github/cargo-tools")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let (name, version) = line
                    .split_once('@')
                    .unwrap_or_else(|| panic!("{line:?} is not name@version"));
                assert!(!version.is_empty(), "{line:?} has no version");
                (name.to_string(), version.to_string())
            })
            .collect()
    }

    /// The pinned Go tools, each as the whole `module/path@version` entry.
    ///
    /// The path is kept whole because that is what `go install` takes and what
    /// the gate's install command carries, so comparing the two needs no
    /// reassembly.
    fn pinned_go_tools() -> Vec<String> {
        read(".github/go-tools")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let (path, version) = line
                    .split_once('@')
                    .unwrap_or_else(|| panic!("{line:?} is not module@version"));
                assert!(!version.is_empty(), "{line:?} has no version");
                assert!(!path.is_empty(), "{line:?} has no module path");
                line.to_string()
            })
            .collect()
    }

    /// Reading the Steam library is allowed, and is what a schema extract
    /// against the client does. Writing into it is what never happens.
    #[test]
    fn settings_deny_edits_under_a_steam_library_and_allow_reads() {
        let settings: serde_json::Value =
            serde_json::from_str(&read(".claude/settings.json")).expect("settings.json parses");
        let deny: Vec<&str> = settings["permissions"]["deny"]
            .as_array()
            .expect("a deny list")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();

        for required in [
            "Edit(//**/SteamLibrary/**)",
            "Edit(//**/steamapps/common/**)",
        ] {
            assert!(
                deny.contains(&required),
                "deny lacks {required}, got {deny:?}"
            );
        }
        let read_denies: Vec<&&str> = deny
            .iter()
            .filter(|rule| rule.starts_with("Read(") && rule.to_lowercase().contains("steam"))
            .collect();
        assert!(
            read_denies.is_empty(),
            "reading the library is allowed, got {read_denies:?}"
        );
    }

    /// `bunx` fetches from npm when the package is absent locally, which
    /// bypasses the lockfile. The hook has to refuse rather than fetch.
    #[test]
    fn the_commit_hook_never_installs_from_npm() {
        let hook = read(".githooks/commit-msg");
        let line = hook
            .lines()
            .find(|line| line.contains("bunx"))
            .expect("the hook runs bunx");

        assert!(line.contains("--no-install"), "got {line:?}");
    }

    /// `cargo machete` reads each crate's own dependency table against that
    /// crate's sources, so a `[workspace.dependencies]` entry that no member
    /// ever names is invisible to it and stays in the manifest after the code
    /// that wanted it is gone.
    #[test]
    fn every_workspace_dependency_is_named_by_a_member() {
        let root: toml::Value = toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        let members = members(&root);

        let mut named = BTreeSet::new();
        for member in &members {
            let manifest: toml::Value = toml::from_str(&read(&format!("{member}/Cargo.toml")))
                .unwrap_or_else(|err| panic!("{member}/Cargo.toml: {err}"));
            workspace_inherited(&manifest, &mut named);
        }
        assert!(
            !named.is_empty(),
            "no member inherits anything, so nothing was checked"
        );

        let table = root["workspace"]["dependencies"]
            .as_table()
            .expect("a workspace dependency table");
        // A path entry counts like any other. A crate dropped from every
        // member's dependency list leaves one behind exactly the way a registry
        // entry does, and skipping them would hide that.
        let orphans: Vec<&String> = table
            .iter()
            .filter(|(name, _)| !named.contains(*name))
            .map(|(name, _)| name)
            .collect();

        assert!(
            orphans.is_empty(),
            "no member names {orphans:?} from [workspace.dependencies]"
        );
    }

    /// A member that omits its `[lints]` block inherits none of the workspace
    /// lints, so `missing_docs`, `clippy::pedantic`, `undocumented_unsafe_blocks`
    /// and `unsafe_op_in_unsafe_fn` stop applying to it and the gate stays
    /// green while they do.
    #[test]
    fn every_member_inherits_the_workspace_lints() {
        let root: toml::Value = toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        assert!(
            root["workspace"].get("lints").is_some(),
            "the workspace declares no lints, so inheriting them proves nothing"
        );

        let missing: Vec<String> = members(&root)
            .into_iter()
            .filter(|member| {
                let manifest: toml::Value = toml::from_str(&read(&format!("{member}/Cargo.toml")))
                    .unwrap_or_else(|err| panic!("{member}/Cargo.toml: {err}"));
                manifest
                    .get("lints")
                    .and_then(|lints| lints.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    != Some(true)
            })
            .collect();

        assert!(
            missing.is_empty(),
            "{missing:?} do not carry [lints] workspace = true"
        );
    }

    /// The walk reaches every dependency table a manifest can carry, and counts
    /// only what is actually inherited.
    #[test]
    fn workspace_inherited_reads_every_dependency_table() {
        let manifest: toml::Value = toml::from_str(
            r#"
[package]
name = "example"
version.workspace = true

[dependencies]
taken.workspace = true
owned = "1.0"

[dev-dependencies]
dev-taken = { workspace = true }

[build-dependencies]
build-taken.workspace = true

[target.'cfg(windows)'.dependencies]
windows-taken.workspace = true

[target.'cfg(unix)'.dev-dependencies]
unix-taken.workspace = true
"#,
        )
        .expect("the example manifest parses");

        let mut found = BTreeSet::new();
        workspace_inherited(&manifest, &mut found);

        let expected: BTreeSet<String> = [
            "build-taken",
            "dev-taken",
            "taken",
            "unix-taken",
            "windows-taken",
        ]
        .iter()
        .map(|name| (*name).to_string())
        .collect();
        assert_eq!(found, expected);
    }

    /// A name on a crate's cargo-machete ignore list is a dependency the crate
    /// declares before the module that uses it lands. The moment a source file
    /// uses the crate, the name has to leave the list, or the list hides a
    /// dependency that later falls out of use again.
    #[test]
    fn machete_ignore_lists_hold_only_unused_dependencies() {
        let root = repo_root();
        let mut checked = 0;

        for group in ["crates", "mods", "xtask"] {
            let group_dir = root.join(group);
            let manifests: Vec<_> = if group_dir.join("Cargo.toml").is_file() {
                vec![group_dir.join("Cargo.toml")]
            } else {
                fs::read_dir(&group_dir)
                    .expect("group directory")
                    .flatten()
                    .map(|entry| entry.path().join("Cargo.toml"))
                    .filter(|path| path.is_file())
                    .collect()
            };

            for manifest_path in manifests {
                let manifest: toml::Value = toml::from_str(&read_path(&manifest_path))
                    .unwrap_or_else(|err| panic!("{}: {err}", manifest_path.display()));
                let Some(ignored) = manifest
                    .get("package")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("cargo-machete"))
                    .and_then(|c| c.get("ignored"))
                    .and_then(toml::Value::as_array)
                else {
                    continue;
                };

                let crate_dir = manifest_path.parent().expect("manifest directory");
                let sources: Vec<(std::path::PathBuf, String)> =
                    rust_sources(&crate_dir.join("src"))
                        .into_iter()
                        .map(|path| {
                            let text = read_path(&path);
                            (path, text)
                        })
                        .collect();

                for name in ignored.iter().filter_map(toml::Value::as_str) {
                    let identifier = name.replace('-', "_");
                    let used: Vec<String> = sources
                        .iter()
                        .filter(|(_, text)| mentions(text, &identifier))
                        .map(|(path, _)| path.display().to_string())
                        .collect();
                    assert!(
                        used.is_empty(),
                        "{}: `{name}` is on the cargo-machete ignore list and used by {used:?}",
                        manifest_path.display()
                    );
                    checked += 1;
                }
            }
        }

        assert!(
            checked > 0,
            "no ignore list was found, so nothing was checked"
        );
    }

    fn read_path(path: &std::path::Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    /// The word check separates `serde` from `serde_json` and skips comments.
    #[test]
    fn mentions_matches_whole_identifiers_outside_comments() {
        let cases = [
            ("use serde::Serialize;", "serde", true),
            ("use serde_json::Value;", "serde", false),
            ("use serde_json::Value;", "serde_json", true),
            ("// serde is planned", "serde", false),
            ("//! serde is planned", "serde", false),
            ("let x = ember_sdk::Mod;", "ember_sdk", true),
            ("let x = my_ember_sdk;", "ember_sdk", false),
            ("", "serde", false),
        ];

        for (source, identifier, expected) in cases {
            assert_eq!(
                mentions(source, identifier),
                expected,
                "{source:?} mentions {identifier:?}"
            );
        }
    }

    /// The parser reads each shape a `uses:` line takes.
    #[test]
    fn workflow_uses_reads_action_reference_and_comment() {
        let text = "\
jobs:
  gate:
    steps:
      - uses: actions/checkout@abc123 # v5.1.0
      - uses: oven-sh/setup-bun@v2
        with:
          bun-version: 1.3.13
      - run: cargo xtask check
";
        let uses = workflow_uses(text);

        assert_eq!(
            uses,
            [
                (
                    "actions/checkout".to_string(),
                    "abc123".to_string(),
                    "v5.1.0".to_string()
                ),
                (
                    "oven-sh/setup-bun".to_string(),
                    "v2".to_string(),
                    String::new()
                ),
            ]
        );
    }
}
