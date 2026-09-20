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

/// The link target of every first cell of every body row of every table in
/// `events`.
fn first_column_links(events: &[Event<'_>]) -> Vec<String> {
    let mut links: Vec<String> = Vec::new();
    let mut first_cell_of_row = false;
    let mut in_first_cell = false;
    let mut in_head = false;
    for event in events {
        match event {
            Event::Start(Tag::TableHead) => in_head = true,
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) => first_cell_of_row = true,
            Event::Start(Tag::TableCell) => in_first_cell = first_cell_of_row && !in_head,
            Event::End(TagEnd::TableCell) => {
                in_first_cell = false;
                first_cell_of_row = false;
            }
            Event::Start(Tag::Link { dest_url, .. }) if in_first_cell => {
                links.push(dest_url.to_string());
            }
            _ => {}
        }
    }
    links
}

// ///////////////////////////////////////////////
// Versions restated outside their pin
// ///////////////////////////////////////////////

/// Whether `release` is three runs of digits separated by dots, and nothing
/// else.
///
/// The digits are read as characters rather than parsed, because an integer
/// parse takes a leading `+` and this is a shape rather than a number. Ember's
/// `tools/workflows.test.ts` holds the Bun pin to the same rule.
fn is_exact_release(release: &str) -> bool {
    let parts: Vec<&str> = release.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `text` holds `version` whole: no digit, and no dot followed by a
/// digit, continues it on either side. A version is then not read inside a
/// longer one on either side, and a leading `v` does not hide it.
fn holds_version(text: &str, version: &str) -> bool {
    text.match_indices(version).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let mut after = text[at + version.len()..].chars();
        let next = after.next();
        let continued_before = before.is_some_and(|c| c.is_ascii_digit() || c == '.');
        let continued_after = next.is_some_and(|c| c.is_ascii_digit())
            || (next == Some('.') && after.next().is_some_and(|c| c.is_ascii_digit()));
        !continued_before && !continued_after
    })
}

// ///////////////////////////////////////////////
// `cargo xtask` references against the command tree
// ///////////////////////////////////////////////

/// The command every reference opens with, split so this file's own cases
/// never spell it.
const XTASK: &str = concat!("cargo", " xtask");

/// Characters that end a command outside a quoted word: a shell separator, a
/// closing bracket or a code span's closing backtick.
const COMMAND_ENDS: &str = ";|&)`";

/// Every `cargo xtask` command `text` spells out, as the line it starts on and
/// the words after `cargo xtask`.
///
/// A reference opening a code span runs to the closing backtick, across line
/// breaks, with each continuation line's indentation and comment marker
/// dropped, so a span wrapped inside a doc comment or a YAML block reads
/// whole. Any other reference runs to the end of its line, which is the shape
/// of a shell line, a workflow `run:` and a fenced block. Either way it stops
/// at a shell comment or a separator.
fn xtask_references(text: &str) -> Vec<(usize, Vec<String>)> {
    let mut found: Vec<(usize, Vec<String>)> = Vec::new();
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    for (at, _) in text.match_indices(XTASK) {
        let before = text[..at].chars().next_back();
        let rest = &text[at + XTASK.len()..];
        if before.is_some_and(is_word) || rest.chars().next().is_some_and(is_word) {
            continue;
        }
        let first_line = rest.split('\n').next().unwrap_or("");
        let command = match rest.find('`') {
            Some(end) if before == Some('`') && !rest[..end].contains("\n\n") => {
                continued_lines(&rest[..end])
            }
            _ => first_line.to_string(),
        };
        let line = text[..at].matches('\n').count() + 1;
        found.push((line, command_words(&command)));
    }
    found
}

/// `text` as one line, with each continuation line's indentation and comment
/// marker dropped.
fn continued_lines(text: &str) -> String {
    text.split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                return line;
            }
            let trimmed = line.trim_start();
            ["///", "//!", "//", "#", "*"]
                .iter()
                .find_map(|marker| trimmed.strip_prefix(marker))
                .unwrap_or(trimmed)
        })
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The words of one command line, the way a shell splits them: a quoted word
/// is one word, a `#` opening a word starts a comment, and a separator ends
/// the command. A word closed by `:*`, the permission wildcard, ends it too.
/// Sentence punctuation closing a word is dropped, so a reference at the end
/// of a sentence reads as the command.
fn command_words(command: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut chars = command.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else {
            break;
        };
        if first == '#' || COMMAND_ENDS.contains(first) {
            break;
        }
        if first == '"' || first == '\'' {
            chars.next();
            let quoted: String = chars.by_ref().take_while(|c| *c != first).collect();
            words.push(quoted);
            continue;
        }
        let mut word = String::new();
        while let Some(c) = chars.next_if(|c| {
            !c.is_whitespace() && !COMMAND_ENDS.contains(*c) && *c != '"' && *c != '\''
        }) {
            word.push(c);
        }
        if let Some(stem) = word.strip_suffix(":*") {
            words.push(stem.to_string());
            break;
        }
        if word != "..." {
            word.truncate(word.trim_end_matches(['.', ',', ':']).len());
        }
        words.push(word);
    }
    words
}

/// Every inline code span in `prose` that opens with a command group and one of
/// its subcommands, as the words it holds. `hooks install` is one; `hooks`
/// alone, or a span opening with `cargo`, is not.
fn bare_references(cli: &clap::Command, prose: &str) -> Vec<Vec<String>> {
    let mut spans: Vec<Vec<String>> = Vec::new();
    let mut rest = prose;
    while let Some(open) = rest.find('`') {
        let run = rest[open..].chars().take_while(|c| *c == '`').count();
        let after = &rest[open + run..];
        if run >= 3 {
            rest = after;
            continue;
        }
        let fence = "`".repeat(run);
        let Some(close) = after.find(&fence) else {
            break;
        };
        let words: Vec<String> = command_words(&after[..close]);
        let group = words.first().and_then(|word| cli.find_subcommand(word));
        let opens_with_a_subcommand = group
            .zip(words.get(1))
            .is_some_and(|(group, word)| group.find_subcommand(word).is_some());
        if opens_with_a_subcommand {
            spans.push(words);
        }
        rest = &after[close + run..];
    }
    spans
}

/// Why `words` do not parse against `cli`, or `None` when they do.
///
/// Every subcommand and every flag has to exist where it is written, and a
/// value-taking flag takes the word after it. A required argument may be
/// missing, because a list of commands leaves them out. `...` stands for any
/// arguments, `<name>` for any one value or subcommand, and an argument that
/// forwards everything after it to another program ends the check. `cli` has
/// to be built, so every global flag reaches each subcommand.
fn reference_problem(cli: &clap::Command, words: &[String]) -> Option<String> {
    let mut command = cli;
    let mut path: Vec<&str> = Vec::new();
    let mut taken = 0;
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let word = word.as_str();
        index += 1;
        let shown = || path.join(" ");
        if word == "..." {
            return None;
        }
        if word == "--" {
            let remaining = words.len() - index;
            return match positional_slot(command, taken) {
                Some(slot) if forwards(slot) => None,
                _ if remaining == 0 => None,
                _ => positional_slot(command, taken + remaining - 1)
                    .is_none()
                    .then(|| format!("`{}` takes no argument after --", shown())),
            };
        }
        let flag = if let Some(long) = word.strip_prefix("--") {
            let (name, inline) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            let found = command.get_arguments().find(|arg| {
                arg.get_long() == Some(name)
                    || arg
                        .get_all_aliases()
                        .is_some_and(|aliases| aliases.contains(&name))
            });
            match found {
                Some(arg) => Some((arg, inline)),
                None => return Some(format!("`{}` takes no --{name}", shown())),
            }
        } else if let Some(short) = word.strip_prefix('-')
            && let Some(letter) = short.chars().next()
            && short.len() == letter.len_utf8()
        {
            let found = command.get_arguments().find(|arg| {
                arg.get_short() == Some(letter)
                    || arg
                        .get_all_short_aliases()
                        .is_some_and(|aliases| aliases.contains(&letter))
            });
            match found {
                Some(arg) => Some((arg, false)),
                None => return Some(format!("`{}` takes no -{letter}", shown())),
            }
        } else if word.starts_with('-') && word.len() > 1 {
            return Some(format!("`{}` cannot read {word}", shown()));
        } else {
            None
        };
        if let Some((arg, inline)) = flag {
            let takes_value = arg.get_action().takes_values();
            if takes_value && !inline && words.get(index).is_some_and(|next| !next.starts_with('-'))
            {
                index += 1;
            }
            continue;
        }
        let placeholder = word.starts_with('<') && word.ends_with('>');
        if taken == 0
            && !placeholder
            && let Some(sub) = command.find_subcommand(word)
        {
            command = sub;
            path.push(sub.get_name());
            continue;
        }
        if let Some(slot) = positional_slot(command, taken) {
            if forwards(slot) {
                return None;
            }
            taken += 1;
            continue;
        }
        if placeholder && command.has_subcommands() {
            return None;
        }
        return Some(if command.has_subcommands() {
            format!("`{}` has no subcommand `{word}`", shown())
        } else {
            format!("`{}` takes no argument `{word}`", shown())
        });
    }
    None
}

/// The positional argument the value at `taken` fills, if the command has room
/// for it.
fn positional_slot(command: &clap::Command, taken: usize) -> Option<&clap::Arg> {
    let mut capacity: usize = 0;
    for arg in command.get_positionals() {
        let most = arg.get_num_args().map_or(1, |range| range.max_values());
        capacity = capacity.saturating_add(most);
        if taken < capacity {
            return Some(arg);
        }
    }
    None
}

/// Whether `arg` hands everything after it to another program unread.
fn forwards(arg: &clap::Arg) -> bool {
    arg.is_trailing_var_arg_set() && arg.is_allow_hyphen_values_set()
}

// ///////////////////////////////////////////////
// Environment variables and build directories
// ///////////////////////////////////////////////

/// Every string literal in a Rust source's code that is exactly an `EMBER_`
/// name, which is how every variable the code reads is spelled. Comment lines
/// are skipped, and a longer string that merely mentions a name is not one.
fn ember_variable_literals(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| {
            line.match_indices("\"EMBER_").filter_map(|(at, _)| {
                let name: String = line[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                line[at + 1 + name.len()..].starts_with('"').then_some(name)
            })
        })
        .collect()
}

/// Every `EMBER_` name `text` mentions.
fn ember_names(text: &str) -> Vec<String> {
    text.match_indices("EMBER_")
        .filter(|(at, _)| {
            !text[..*at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .map(|(at, _)| {
            text[at..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect()
        })
        .collect()
}

/// The directories under `.cache` a build lands in, each named by an id.
const BUILD_DIRECTORIES: &[&str] = &["server", "schema", "run", "loca", "archive", "build"];

/// An issue form with its `placeholder:` values removed, block scalars
/// included. A placeholder is an example by construction and is never
/// submitted.
fn without_placeholders(form: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut skipping_below: Option<usize> = None;
    for line in form.lines() {
        let indent = line.len() - line.trim_start().len();
        if let Some(depth) = skipping_below {
            if line.trim().is_empty() || indent > depth {
                continue;
            }
            skipping_below = None;
        }
        if let Some(value) = line.trim_start().strip_prefix("placeholder:") {
            if matches!(value.trim(), "|" | ">" | "|-" | ">-") {
                skipping_below = Some(indent);
            }
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

/// Every path in `text` that names a build directory by a concrete id, as the
/// directory and the id, such as `server/<id>`. A placeholder is not an id.
fn build_directory_ids(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == '`')
        .flat_map(|token| {
            let segments: Vec<&str> = token.split(['/', '\\']).collect();
            segments
                .windows(2)
                .filter(|pair| {
                    BUILD_DIRECTORIES.contains(&pair[0])
                        && !pair[1].is_empty()
                        && pair[1].bytes().all(|b| b.is_ascii_digit())
                })
                .map(|pair| format!("{}/{}", pair[0], pair[1]))
                .collect::<Vec<String>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use clap::CommandFactory;
    use yaml_rust2::{Yaml, YamlLoader};

    use super::{
        XTASK, bare_references, build_directory_ids, code_span_lists, ember_names,
        ember_variable_literals, first_column, first_column_links, holds_version, is_exact_release,
        mentions, prose_files, prose_kind, prose_paragraphs, reference_problem, repo_root,
        rust_sources, section, stated_counts, without_placeholders, workflow_uses,
        workspace_inherited, xtask_references,
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

    /// Every key in a YAML document at any depth, as the path of keys that
    /// leads to it and the value it holds.
    fn keyed_values(value: &Yaml, path: &[String], found: &mut Vec<(Vec<String>, Yaml)>) {
        match value {
            Yaml::Hash(table) => {
                for (key, child) in table {
                    let mut child_path = path.to_vec();
                    child_path.push(
                        key.as_str()
                            .map_or_else(|| format!("{key:?}"), str::to_string),
                    );
                    found.push((child_path.clone(), child.clone()));
                    keyed_values(child, &child_path, found);
                }
            }
            Yaml::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(index.to_string());
                    keyed_values(child, &child_path, found);
                }
            }
            _ => {}
        }
    }

    /// Every workflow under `.github/workflows`, as its file name and text.
    fn workflows() -> Vec<(String, String)> {
        let dir = repo_root().join(".github").join("workflows");
        let mut found: Vec<(String, String)> = fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
            })
            .map(|path| {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                (name, read_path(&path))
            })
            .collect();
        found.sort();
        found
    }

    /// The Bun release lives in `.bun-version` and nowhere else, and setup-bun
    /// reads it through `bun-version-file`. A workflow that writes a release
    /// down itself, in any key at any depth or anywhere in its text, holds a
    /// second copy the next bump leaves behind, and a release resolved at run
    /// time is under no cooldown.
    ///
    /// setup-bun falls back to `package.json`, and then to the newest release,
    /// when the file names nothing it can read, so the file has to hold one
    /// exact release.
    #[test]
    fn every_bun_release_a_workflow_uses_is_read_from_the_pin_file() {
        let pin = read(".bun-version").trim().to_string();
        assert!(
            is_exact_release(&pin),
            ".bun-version holds {pin:?}, which is not one exact release"
        );

        let mut setups = 0;
        for (name, text) in workflows() {
            assert!(
                !holds_version(&text, &pin),
                "{name} writes the release .bun-version pins"
            );
            let document = YamlLoader::load_from_str(&text)
                .unwrap_or_else(|err| panic!("{name} is not YAML: {err}"))
                .remove(0);
            let mut values: Vec<(Vec<String>, Yaml)> = Vec::new();
            keyed_values(&document, &[], &mut values);
            for (path, value) in &values {
                let key = path.last().map_or("", String::as_str).to_lowercase();
                if key.contains("bun") && key.contains("version") {
                    assert!(
                        key == "bun-version-file" && value.as_str() == Some(".bun-version"),
                        "{name} sets {} to {value:?} itself",
                        path.join(".")
                    );
                }
                if path.last().is_some_and(|last| last == "uses")
                    && value
                        .as_str()
                        .is_some_and(|uses| uses.starts_with("oven-sh/setup-bun@"))
                {
                    setups += 1;
                    let step = &path[..path.len() - 1];
                    let named = values.iter().any(|(other, value)| {
                        other.len() == step.len() + 2
                            && other.starts_with(step)
                            && other[step.len()] == "with"
                            && other[step.len() + 1] == "bun-version-file"
                            && value.as_str() == Some(".bun-version")
                    });
                    assert!(
                        named,
                        "{name} runs setup-bun at {} without the pin file",
                        step.join(".")
                    );
                }
            }
        }
        assert!(
            setups > 0,
            "no workflow runs setup-bun, so nothing was checked"
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

    /// The shellcheck step names every hook in `.githooks`.
    ///
    /// A command spawns with no shell to expand a glob, so the step names its
    /// files one by one. A hook added and left off that list is shell the gate
    /// reports a pass over without reading.
    #[test]
    fn the_shellcheck_step_names_every_hook() {
        let mut hooks: Vec<String> = fs::read_dir(repo_root().join(".githooks"))
            .expect(".githooks is readable")
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| format!(".githooks/{}", entry.file_name().to_string_lossy()))
            .collect();
        hooks.sort();
        assert!(!hooks.is_empty(), ".githooks holds no hook");

        let command = crate::check::STEPS
            .iter()
            .find(|step| step.name == "shellcheck")
            .expect("a shellcheck step")
            .primary
            .command;
        let mut checked: Vec<String> = command[1..].iter().map(|arg| (*arg).to_string()).collect();
        checked.sort();
        assert_eq!(
            checked, hooks,
            "the shellcheck step reads {checked:?} and .githooks holds {hooks:?}"
        );
    }

    /// Every hook declares a shell in a shebang, which is what tells shellcheck
    /// which dialect to read it as. A hook with none is skipped as a file
    /// shellcheck cannot classify.
    #[test]
    fn every_hook_names_its_shell_in_a_shebang() {
        for entry in fs::read_dir(repo_root().join(".githooks"))
            .expect(".githooks is readable")
            .flatten()
            .filter(|entry| entry.path().is_file())
        {
            let path = entry.path();
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("{} cannot be read: {err}", path.display()));
            assert!(
                text.starts_with("#!/"),
                "{} opens with no shebang, so shellcheck cannot tell its dialect",
                path.display()
            );
        }
    }

    /// Every tool the pin file names, as the key and its version.
    fn pinned_tools() -> Vec<(String, String)> {
        crate::pins::pinned_tools(&read(crate::pins::PINS))
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    /// The pin files, as the three texts the rules read.
    fn pin_texts() -> (String, String, String) {
        (
            read(crate::pins::PINS),
            read(crate::pins::LOCK),
            read(crate::pins::WORKFLOW),
        )
    }

    /// The pin files meet every rule the gate holds them to.
    ///
    /// The gate runs these same rules as its first step, before any tool, so
    /// this is the same code rather than a second copy of it. What it covers:
    /// locked mode is on, every version is one exact release, every coordinate
    /// names a binary, every tool carries a checksum on every platform the
    /// matrix installs on, the two files agree on each version, and no lockfile
    /// entry survives a tool the pin file dropped.
    #[test]
    fn the_pin_files_meet_every_rule() {
        let (pins, lock, workflow) = pin_texts();
        let problems = crate::pins::problems(&pins, &lock, &workflow);
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// The pin file turns mise's locked mode on, in both of its spellings.
    ///
    /// This has a case of its own because it is the whole guarantee and it is
    /// two lines. Without them `mise install` accepts a tool the lockfile does
    /// not name, and `mise which` answers for a tool this repository pins
    /// nowhere out of a developer's global configuration, which the gate would
    /// then run.
    ///
    /// Both spellings are asserted because they are not the same setting.
    /// `MISE_LOCKED=false` and a `locked_scopes` that drops `project` each turn
    /// `settings.locked` off while `mise settings get locked` still prints what
    /// the file holds. `tool_config.locked` answers to neither, and mise reads
    /// it from the file alone.
    #[test]
    fn the_pin_file_turns_locked_mode_on() {
        let text = read(crate::pins::PINS);
        assert!(
            crate::pins::locked(&text),
            "{} does not set locked = true under [settings]",
            crate::pins::PINS
        );
        assert!(
            crate::pins::tool_config_locked(&text),
            "{} does not set locked = true under [tool_config]",
            crate::pins::PINS
        );
    }

    /// Every tool the table names is one the pin file still pins.
    ///
    /// The table is what lets a lockfile entry be held to an owner and a
    /// repository at all, so an entry left behind by a removed tool is a
    /// mapping nothing exercises.
    #[test]
    fn the_binary_map_names_only_coordinates_the_pin_file_pins() {
        let pinned = pinned_tools();
        for tool in crate::pins::TOOLS {
            assert!(
                pinned.iter().any(|(name, _)| name == tool.key),
                "the tool table names {} as {} and {} does not pin it",
                tool.key,
                tool.binary,
                crate::pins::PINS
            );
        }
    }
    /// Every binary the gate resolves is one the pin file names, and every tool
    /// the pin file names is one the gate resolves.
    ///
    /// Set equality rather than a count. A rename on one side and a rename on
    /// the other leave the count intact while the gate resolves a name nothing
    /// pins. A pin key is a bare registry name or a backend coordinate, and a
    /// coordinate names an owner and a repository rather than the binary
    /// inside, so `pins::TOOLS` carries that mapping and a rename
    /// has to touch it.
    #[test]
    fn the_pin_file_and_the_gate_name_the_same_binaries() {
        let mut resolved: BTreeSet<String> = BTreeSet::new();
        for step in crate::check::STEPS {
            for step_run in std::iter::once(&step.primary).chain(step.fallback.as_ref()) {
                if step_run.mise {
                    resolved.insert(step_run.command[0].to_string());
                }
                // A tool filled into a flag is resolved through mise like any
                // other, and no step's own command names it.
                if let Some(target) = step_run.resolved {
                    resolved.insert(target.tool.to_string());
                }
            }
        }
        assert!(!resolved.is_empty(), "no gate step resolves through mise");

        let pinned = crate::pins::pinned_binaries(&read(crate::pins::PINS))
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert_eq!(
            resolved,
            pinned,
            "the gate resolves {resolved:?} and {} names {pinned:?}",
            crate::pins::PINS
        );
    }

    /// Whether `run` invokes mise to do anything but read.
    ///
    /// Every line is read, because a multi-line block runs every one of them.
    /// A word naming the program starts an invocation, and the words after it
    /// are matched against `READING_MISE_COMMANDS` as a prefix. A prefix rather
    /// than one word, because `settings` and `config` each have a `set` that
    /// rewrites the pin file. An invocation with nothing after it counts as an
    /// install.
    fn run_installs(run: &str) -> bool {
        if crate::pins::MISE_BOOTSTRAP_URLS
            .iter()
            .any(|url| run.contains(url))
        {
            return true;
        }
        run.lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.iter().enumerate().any(|(at, word)| {
                let program = word.rsplit(['/', '\\']).next().unwrap_or(word);
                if program != "mise" && program != "mise.exe" {
                    return false;
                }
                let rest = &words[at + 1..];
                !crate::pins::READING_MISE_COMMANDS
                    .iter()
                    .any(|reading| rest.starts_with(reading))
            })
        })
    }

    /// Whether `step` installs anything, which is anything not known to be
    /// harmless.
    ///
    /// The question is not whether a step looks like an installer. It is
    /// whether the step is one of the few known to install nothing. A `uses`
    /// naming anything outside `PRELUDE_ACTIONS`, a composite action included,
    /// is an install, because a name says nothing about what an action does.
    /// A `run` invoking mise for anything but a read is an install for the same
    /// reason. Owner names are compared without case, the way the forge
    /// resolves them.
    fn step_installs(step: &Yaml) -> bool {
        if let Some(uses) = step["uses"].as_str() {
            let action = uses.split('@').next().unwrap_or(uses).trim();
            return !crate::pins::PRELUDE_ACTIONS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(action));
        }
        step["run"].as_str().is_some_and(run_installs)
    }

    /// Whether `step` runs the pin rules and nothing else.
    ///
    /// The command is matched whole. A line that appends to it, such as one
    /// piping the exit code into `true`, runs the rules without letting them
    /// stop the job, so it is not this step.
    fn step_runs_pin_rules(step: &Yaml) -> bool {
        step["run"].as_str().is_some_and(|run| {
            let words: Vec<&str> = run.split_whitespace().collect();
            words == ["cargo", "xtask", "pins"]
        })
    }

    /// The steps a job declares, which is empty for a job that declares none.
    fn job_steps(job: &Yaml) -> Vec<Yaml> {
        job["steps"].as_vec().cloned().unwrap_or_default()
    }

    /// No job installs before the pin rules run, and the gate job runs them.
    ///
    /// `mise install` fetches and unpacks every artifact `mise.lock` records, so
    /// a rule that ran after it would report a finding about bytes already on
    /// disk.
    ///
    /// Every job is walked, not only the gate job. Naming one job is what makes
    /// its absence a failure rather than a silent skip, and holding the rest is
    /// what keeps a job added later from installing with nothing watching its
    /// order.
    ///
    /// Conditions are read as well as order, on the job and on the step. A step
    /// or job carrying `if` may not run at all, and a step carrying
    /// `continue-on-error` runs without stopping the job. Both keys are refused
    /// by presence rather than by value, because a value is written in more
    /// spellings than a reader can enumerate and an unrecognized one would read
    /// as absent.
    #[test]
    fn the_pin_rules_run_before_anything_installs() {
        let workflow = yaml(crate::pins::WORKFLOW);
        let jobs = workflow["jobs"]
            .as_hash()
            .expect("the workflow declares jobs");

        // Every job that installs, gate or not, runs the rules first.
        for (name, job) in jobs {
            let name = name.as_str().unwrap_or("a job");
            let steps = job_steps(job);
            let Some(installs) = steps.iter().position(step_installs) else {
                continue;
            };
            let rules = steps
                .iter()
                .position(step_runs_pin_rules)
                .unwrap_or_else(|| {
                    panic!(
                        "the {name} job installs at step {installs} and never runs the pin rules"
                    )
                });
            assert!(
                rules < installs,
                "the {name} job installs at step {installs} and runs the pin rules at step \
                 {rules}, so a poisoned url is fetched and unpacked before anything reads it"
            );
            let step = &steps[rules];
            assert!(
                step["if"].is_badvalue(),
                "the {name} job guards its pin step with an if, so it can install without running \
                 the rules"
            );
            assert!(
                step["continue-on-error"].is_badvalue(),
                "the {name} job lets its pin step continue on error, so a refusal need not stop \
                 the install"
            );
        }

        // The gate job specifically has to be there and has to install, so its
        // absence is a failure rather than a job with nothing to check.
        let gate = &workflow["jobs"][crate::pins::GATE_JOB];
        assert!(
            !gate.is_badvalue(),
            "{} declares no {} job, so nothing holds the order of its install",
            crate::pins::WORKFLOW,
            crate::pins::GATE_JOB
        );
        assert!(
            gate["if"].is_badvalue(),
            "the {} job carries an if, so it can be skipped whole",
            crate::pins::GATE_JOB
        );
        let steps = job_steps(gate);
        assert!(
            !steps.is_empty(),
            "the {} job declares no steps, so its work happens somewhere this cannot read",
            crate::pins::GATE_JOB
        );
        assert!(
            steps.iter().any(step_installs),
            "the {} job installs nothing, so the gate runs tools from somewhere this cannot read",
            crate::pins::GATE_JOB
        );
    }

    /// No step invokes mise for anything but a read.
    ///
    /// Ordering is not enough on its own. A step rewriting `mise.toml` sits
    /// after the pin step quite legitimately, so the order stays compliant
    /// while the document the rules certified is replaced underneath them.
    /// `mise config set` reaches `tool_config.locked`, the setting no
    /// environment variable reaches, and `mise use` rewrites a pinned version,
    /// so the install would then run unlocked or at a release nothing checked.
    ///
    /// The rule is the same allow list the ordering rule uses, applied at every
    /// position rather than before the install. Nothing in the gate needs to
    /// run mise by hand: the action installs, and the gate resolves tools
    /// through mise from inside the binary rather than from a `run` line.
    #[test]
    fn no_step_invokes_mise_outside_a_read() {
        let workflow = yaml(crate::pins::WORKFLOW);
        let jobs = workflow["jobs"]
            .as_hash()
            .expect("the workflow declares jobs");
        for (name, job) in jobs {
            let name = name.as_str().unwrap_or("a job");
            for (at, step) in job_steps(job).iter().enumerate() {
                let Some(run) = step["run"].as_str() else {
                    continue;
                };
                assert!(
                    !run_installs(run),
                    "the {name} job runs {run:?} at step {at}, which invokes mise to install or \
                     write; a write after the pin rules leaves the install reading a document \
                     nothing checked"
                );
            }
        }
    }

    /// The mise reader treats a read as a read and everything else as an
    /// install.
    ///
    /// The accepted half is the control. A rule refusing every mention of mise
    /// would refuse each line below it and still pass every case above it, so
    /// the two halves together are what say the allow list is an allow list.
    ///
    /// `settings` and `config` appear in both halves, which is the point of
    /// matching two words: the same first word reads and writes depending on
    /// the second.
    #[test]
    fn the_mise_reader_separates_a_read_from_a_write() {
        for line in [
            // Fetching, including the aliases.
            "mise install",
            "mise i",
            "mise x -- taplo --version",
            "mise use taplo@0.10.0",
            "mise up",
            // Writing the pin file, which no ordering rule can catch.
            "mise settings set locked false",
            "mise settings set --local locked false",
            "mise config set tool_config.locked false",
            "mise config set tools.taplo 9.9.9",
            // Both documented ways to fetch mise itself.
            "curl https://mise.run | sh",
            "curl https://mise.jdx.dev/install.sh | sh",
            // A prefix of a reading word is not that word.
            "mise versions",
            "mise configure",
            "mise settings-set locked false",
            // An invocation with nothing after it, and one by path.
            "mise",
            "./mise install",
            "~/.local/bin/mise use taplo@1",
            // A second command on the same line, and on a later one.
            "mise which taplo && mise install",
            "echo one\nmise config set tools.taplo 9.9.9",
        ] {
            assert!(run_installs(line), "{line:?} was read as harmless");
        }

        for line in [
            "mise which taplo",
            "mise doctor",
            "mise version",
            "mise --version",
            "mise -v",
            "mise settings get locked",
            "mise settings ls",
            "mise config get tools.taplo",
            "mise config ls",
            // Nothing to do with mise at all.
            "cargo build --workspace",
            "bun install --frozen-lockfile",
            "echo promise install",
        ] {
            assert!(!run_installs(line), "{line:?} was read as an install");
        }
    }

    /// The gate job installs through mise, which reads the pin file itself, and
    /// no workflow writes a version of its own.
    ///
    /// Four shapes, each a way a workflow installs something other than what
    /// the pin file holds. A release written into a workflow is a second copy
    /// the next bump leaves behind. A `mise_toml` or `tool_versions` input
    /// makes the action write its own pin file over the committed one, so the
    /// rules above would check a file no install reads. A `sha256` input
    /// returns before the action compares its mise download against the
    /// minisign-signed `SHASUMS256.txt`, which trades a signature for a hash
    /// somebody typed. A release that is not exact resolves at run time, and a
    /// release resolved at run time is under no cooldown.
    ///
    /// Every job is walked rather than the gate job alone, so a second install
    /// step added elsewhere is held to the same rules.
    #[test]
    fn every_mise_install_reads_the_pin_file_and_writes_no_version() {
        let workflow = yaml(crate::pins::WORKFLOW);
        let jobs = workflow["jobs"]
            .as_hash()
            .expect("the workflow declares jobs");

        let mut installs = 0;
        for (_, job) in jobs {
            for step in job["steps"].as_vec().unwrap_or(&Vec::new()) {
                if !step["uses"]
                    .as_str()
                    .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
                {
                    continue;
                }
                installs += 1;
                for input in ["mise_toml", "tool_versions"] {
                    assert!(
                        step["with"][input].is_badvalue(),
                        "mise-action takes {input}, which writes over the committed {}",
                        crate::pins::PINS
                    );
                }
                assert!(
                    step["with"]["sha256"].is_badvalue(),
                    "mise-action takes a sha256, which skips the signed checksum file it would \
                     otherwise verify mise against"
                );
                assert_ne!(
                    step["with"]["install"].as_bool(),
                    Some(false),
                    "mise-action installs nothing, so the gate resolves tools that are not there"
                );
                let release = step["with"]["version"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| step["with"]["version"].as_f64().map(|n| n.to_string()))
                    .expect("mise-action takes an exact mise release");
                assert!(
                    is_exact_release(&release),
                    "mise-action takes {release:?} as version, which is not one exact release"
                );
            }
        }
        assert!(installs > 0, "no job installs through mise-action");

        let text = read(crate::pins::WORKFLOW);
        for (tool, version) in pinned_tools() {
            assert!(
                !holds_version(&text, &version),
                "the workflow carries its own copy of the release {} pins for {tool}",
                crate::pins::PINS
            );
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
    /// with whitespace, trailing commas and quote style normalized away, which
    /// is what Prettier owns. The list's name has to appear exactly where those
    /// two lines put it, so nothing else in the config can read or change it.
    #[test]
    fn commitlint_enforces_the_scope_file_and_nothing_beside_it() {
        let code = commitlint_code();
        let compact: String = code
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace(",]", "]")
            .replace(",)", ")")
            .replace('"', "'");

        for (what, expected) in [
            ("the rule", r"'scope-enum':[2,'always',scopes]"),
            (
                "the line that loads the list",
                r"constscopes=JSON.parse(readFileSync(newURL('.github/commit-scopes.json',import.meta.url),'utf8'))",
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
    /// lists are `cargo xtask check` and `mise.toml`, and prose names those
    /// rather than counting them.
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
            ("the steps `cargo xtask check` runs", &[]),
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
        let workspace: toml::Value =
            toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        let mut checked = 0;

        for member in members(&workspace) {
            let manifest_path = root.join(member).join("Cargo.toml");
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
            let sources: Vec<(std::path::PathBuf, String)> = rust_sources(&crate_dir.join("src"))
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

    // ///////////////////////////////////////////////
    // Facts bound to their source
    // ///////////////////////////////////////////////

    /// `path` relative to the repository root, with forward slashes on every
    /// host.
    fn shown(path: &Path) -> String {
        path.strip_prefix(repo_root())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Every file under the repository the prose check reads, with its text.
    ///
    /// A file of such a kind that is not UTF-8 is left out here, and
    /// `no_prose_states_a_count_of_gate_steps_or_pinned_tools` fails on it.
    fn prose_texts() -> Vec<(PathBuf, String)> {
        prose_files(&repo_root())
            .into_iter()
            .filter(|path| prose_kind(path).is_some())
            .filter_map(|path| {
                let text = fs::read_to_string(&path).ok()?;
                Some((path, text))
            })
            .collect()
    }

    /// Every version a pin file holds, as the file and the version.
    ///
    /// The toolchain contributes its channel and that channel's minor release,
    /// because prose names a Rust release either way.
    fn pinned_versions() -> Vec<(&'static str, String)> {
        let mut pins: Vec<(&'static str, String)> = Vec::new();

        let toolchain: toml::Value =
            toml::from_str(&read("rust-toolchain.toml")).expect("rust-toolchain.toml parses");
        let channel = toolchain["toolchain"]["channel"]
            .as_str()
            .expect("rust-toolchain.toml names a channel")
            .to_string();
        let minor: String = channel.split('.').take(2).collect::<Vec<&str>>().join(".");
        pins.push(("rust-toolchain.toml", channel));
        pins.push(("rust-toolchain.toml", minor));

        for (_, version) in pinned_tools() {
            pins.push((crate::pins::PINS, version));
        }
        pins.push((".bun-version", read(".bun-version").trim().to_string()));

        let workflow = yaml(".github/workflows/ci.yml");
        let mut values: Vec<(Vec<String>, Yaml)> = Vec::new();
        keyed_values(&workflow, &[], &mut values);
        for (path, value) in values {
            // mise's own release is pinned where the action that installs it
            // is named, because mise cannot install itself.
            if path.last().is_some_and(|key| key == "version") {
                let version = match value {
                    Yaml::Real(text) | Yaml::String(text) => text,
                    other => panic!("a workflow version input holds {other:?}"),
                };
                pins.push((".github/workflows/ci.yml", version));
            }
        }
        pins
    }

    /// A version restated outside the file that pins it is a copy the next
    /// bump leaves behind. Every pin is read from its file: the toolchain
    /// channel and its minor release, each `mise.toml` entry, `.bun-version`,
    /// and the mise release the gate job's install step takes. None of them may
    /// appear in prose, which is what the count check reads: every Markdown
    /// file and issue form whole, and the comments of code, configuration and
    /// workflows.
    ///
    /// What passes: a version in a value, such as the pins themselves,
    /// `rust-version` in `Cargo.toml` and every lockfile; an action's version
    /// comment, which Dependabot moves with its hash; a version of anything no
    /// file here pins; and a pin's previous value, which no longer names a
    /// pin.
    #[test]
    fn no_prose_restates_a_pinned_version() {
        let pins = pinned_versions();
        for source in [
            "rust-toolchain.toml",
            crate::pins::PINS,
            ".bun-version",
            ".github/workflows/ci.yml",
        ] {
            assert!(
                pins.iter().any(|(from, _)| *from == source),
                "no version was read from {source}, so its pin goes unchecked"
            );
        }
        assert!(
            pins.iter().all(|(_, version)| !version.is_empty()),
            "a pin file holds an empty version: {pins:?}"
        );

        let mut restated: Vec<String> = Vec::new();
        for (path, text) in prose_texts() {
            for paragraph in prose_paragraphs(&path, &text) {
                for (source, version) in &pins {
                    if holds_version(&paragraph, version) {
                        restated.push(format!("{}: {version}, pinned in {source}", shown(&path)));
                    }
                }
            }
        }
        restated.sort();
        restated.dedup();
        assert!(
            restated.is_empty(),
            "prose restates a pinned version, which the next bump leaves behind. Name the file \
             that pins it instead: {restated:#?}"
        );
    }

    /// A pinned release is three runs of digits and nothing else, on both
    /// sides of the project: this rule and the one Ember's
    /// `tools/workflows.test.ts` holds the Bun pin to answer alike.
    ///
    /// What passes: a leading zero and a number no release will ever reach.
    /// Both fail closed at the download, and refusing them here would say
    /// something about releases rather than about shape.
    #[test]
    fn is_exact_release_reads_three_runs_of_digits() {
        let cases = [
            ("1.4.2", true),
            ("1.98.1", true),
            ("01.04.02", true),
            ("99999999999.0.0", true),
            ("1.+4.2", false),
            ("v1.4.2", false),
            ("^1.4.2", false),
            ("1.4", false),
            ("1.4.2.1", false),
            ("1..2", false),
            ("1.4.2-canary.1", false),
            ("latest", false),
            ("", false),
        ];
        for (release, expected) in cases {
            assert_eq!(is_exact_release(release), expected, "{release:?}");
        }
    }

    /// A version reads whole, whatever sits beside it.
    #[test]
    fn holds_version_reads_a_version_whole() {
        let cases = [
            ("Rust 1.98.1, pinned", "1.98.1", true),
            ("Bun v1.4.2.", "1.4.2", true),
            ("(1.4.2)", "1.4.2", true),
            ("11.4.2", "1.4.2", false),
            ("1.4.25", "1.4.2", false),
            ("1.1.4.2", "1.4.2", false),
            ("1.4.2.1", "1.4.2", false),
            ("Rust 1.98 or later", "1.98", true),
            ("Rust 1.98.1", "1.98", false),
            ("", "1.4.2", false),
        ];
        for (text, version, expected) in cases {
            assert_eq!(
                holds_version(text, version),
                expected,
                "{text:?} holds {version:?}"
            );
        }
    }

    /// `rust-version` claims the oldest toolchain that builds the workspace,
    /// and the only one anything builds on is the one `rust-toolchain.toml`
    /// pins. Holding the claim to that channel's minor release keeps it proven
    /// by every build, and a toolchain bump that leaves it behind goes red here.
    #[test]
    fn rust_version_is_the_pinned_toolchains_minor_release() {
        let toolchain: toml::Value =
            toml::from_str(&read("rust-toolchain.toml")).expect("rust-toolchain.toml parses");
        let channel = toolchain["toolchain"]["channel"]
            .as_str()
            .expect("rust-toolchain.toml names a channel");
        assert!(
            is_exact_release(channel),
            "rust-toolchain.toml pins {channel:?}, which is not one exact release"
        );
        let parts: Vec<&str> = channel.split('.').collect();

        let workspace: toml::Value =
            toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        let declared = workspace["workspace"]["package"]["rust-version"]
            .as_str()
            .expect("[workspace.package] declares rust-version");
        assert_eq!(
            declared,
            parts[..2].join("."),
            "rust-version says {declared} and rust-toolchain.toml pins {channel}"
        );
    }

    /// The real command tree, built so every global flag reaches each
    /// subcommand.
    fn command_tree() -> clap::Command {
        let mut cli = crate::Cli::command();
        cli.build();
        cli
    }

    /// Every `cargo xtask` command written anywhere in the repository parses
    /// against the command tree clap builds: every subcommand exists, and every
    /// flag exists on the command it follows. A reference that names a flag or
    /// a subcommand the tree lacks is a command a reader runs and gets refused.
    ///
    /// Every file is read, code and configuration included, because an error
    /// message telling somebody what to run is as much a copy as a document.
    /// Prose also contributes every code span that opens with a command group
    /// and one of its subcommands, written without the `cargo xtask` in front.
    ///
    /// What passes: a missing required argument, because a list of commands
    /// leaves them out; any value given to a flag or a positional; a
    /// subcommand group named alone; and everything after `server` or
    /// `schema`, which forward to Ember's xtask unread. Ember's own suite
    /// holds those arguments to Ember's command tree.
    #[test]
    fn every_cargo_xtask_reference_parses() {
        let cli = command_tree();
        let mut problems: Vec<String> = Vec::new();
        let mut carriers: BTreeSet<String> = BTreeSet::new();
        for path in prose_files(&repo_root()) {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let file = shown(&path);
            for (line, words) in xtask_references(&text) {
                carriers.insert(file.clone());
                if let Some(problem) = reference_problem(&cli, &words) {
                    problems.push(format!(
                        "{file}:{line}: `{XTASK} {}`: {problem}",
                        words.join(" ")
                    ));
                }
            }
            for paragraph in prose_paragraphs(&path, &text) {
                for words in bare_references(&cli, &paragraph) {
                    if let Some(problem) = reference_problem(&cli, &words) {
                        problems.push(format!("{file}: `{}`: {problem}", words.join(" ")));
                    }
                }
            }
        }

        for carrier in [
            ".githooks/pre-push",
            ".github/workflows/ci.yml",
            ".claude/settings.json",
            "CONTRIBUTING.md",
            "xtask/src/main.rs",
        ] {
            assert!(
                carriers.contains(carrier),
                "no reference was read from {carrier}, so the scan misses that kind of file"
            );
        }
        assert!(
            problems.is_empty(),
            "these references do not parse against the command tree: {problems:#?}"
        );
    }

    /// Each shape a reference is written in reads as the words of the
    /// command.
    #[test]
    fn xtask_references_read_each_shape_a_reference_takes() {
        let cases: Vec<(String, Vec<Vec<&str>>)> = vec![
            (
                format!("Run `{XTASK} hooks install` once."),
                vec![vec!["hooks", "install"]],
            ),
            (
                format!("/// Run `{XTASK}\n/// scopes` for the list"),
                vec![vec!["scopes"]],
            ),
            (
                format!("key: >\n  `{XTASK} server\n  seed` then"),
                vec![vec!["server", "seed"]],
            ),
            (
                format!("{XTASK} server run --fixture x   # launch"),
                vec![vec!["server", "run", "--fixture", "x"]],
            ),
            (
                format!("{XTASK} server seed --fixture \"a b\""),
                vec![vec!["server", "seed", "--fixture", "a b"]],
            ),
            (
                format!("\"Bash({XTASK} schema extract:*)\","),
                vec![vec!["schema", "extract"]],
            ),
            (
                format!("\"PowerShell({XTASK} check)\","),
                vec![vec!["check"]],
            ),
            (format!("Then run {XTASK} check."), vec![vec!["check"]]),
            (
                format!("      - run: {XTASK} check\n      - run: other"),
                vec![vec!["check"]],
            ),
            (format!("exec {XTASK} check"), vec![vec!["check"]]),
            (
                format!("`{XTASK} server ...` forwards"),
                vec![vec!["server", "..."]],
            ),
            (
                format!("`{XTASK} scopes` and `{XTASK} check`"),
                vec![vec!["scopes"], vec!["check"]],
            ),
            (format!("`{XTASK}` alone"), vec![vec![]]),
            (format!("my{XTASK} check"), vec![]),
            (format!("{XTASK}s check"), vec![]),
            (String::new(), vec![]),
        ];
        for (text, expected) in cases {
            let found: Vec<Vec<String>> = xtask_references(&text)
                .into_iter()
                .map(|(_, words)| words)
                .collect();
            assert_eq!(found, expected, "{text:?}");
        }
    }

    /// A reference parses exactly when the command tree accepts it, and a
    /// refusal names what the tree lacks. Everything after a forwarding
    /// command is Ember's to check.
    #[test]
    fn reference_problem_holds_a_reference_to_the_command_tree() {
        let cli = command_tree();
        let accepted: &[&[&str]] = &[
            &[],
            &["check"],
            &["scopes"],
            &["hooks", "install"],
            &["server", "fetch"],
            &["server", "fetch", "--anything", "at", "all"],
            &["server", "--", "--help"],
            &["schema", "diff", "old", "new"],
            &["server", "..."],
            &["<command>"],
            &["check", "--help"],
            &[
                "package",
                "--mod",
                "private-chests",
                "--tag",
                "private-chests-v0.1.0",
                "--out",
                "dist",
            ],
            &["package", "--ember-loader", "ember/POWRPROF.dll"],
        ];
        for words in accepted {
            let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
            assert_eq!(reference_problem(&cli, &words), None, "{words:?}");
        }

        let refused: &[(&[&str], &str)] = &[
            (&["publish"], "publish"),
            (&["package", "--sign"], "--sign"),
            (&["hooks", "remove"], "remove"),
            (&["check", "--force"], "--force"),
            (&["scopes", "extra"], "`extra`"),
            (&["hooks", "install", "now"], "`now`"),
            (&["check", "-x"], "-x"),
        ];
        for (words, reason) in refused {
            let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
            let problem = reference_problem(&cli, &words);
            assert!(
                problem
                    .as_deref()
                    .is_some_and(|problem| problem.contains(reason)),
                "{words:?} gave {problem:?}, which does not name {reason}"
            );
        }
    }

    /// A span naming a group and one of its subcommands is a reference, and
    /// nothing else in a span is. A forwarding command has no subcommands of
    /// its own to name.
    #[test]
    fn bare_references_read_a_group_and_its_subcommand() {
        let cli = command_tree();
        let cases: &[(&str, &[&[&str]])] = &[
            ("run `hooks install` once", &[&["hooks", "install"]]),
            ("`server fetch` forwards", &[]),
            ("`server` and `schema` forward", &[]),
            ("the `check` step", &[]),
            ("```sh\nhooks install\n```", &[]),
            ("", &[]),
        ];
        for (prose, expected) in cases {
            let found = bare_references(&cli, prose);
            assert_eq!(found, *expected, "{prose:?}");
        }
    }

    /// The README's documentation table is the list a reader opens first. A
    /// document it leaves out is one nobody reading the table knows exists,
    /// and a link to a file that is gone is a dead end.
    #[test]
    fn the_documentation_table_links_every_document() {
        let root = repo_root();
        let readme = read("README.md");
        let table =
            section(&readme, "Documentation").expect("README.md has a Documentation section");
        let linked = first_column_links(&table);
        assert!(
            !linked.is_empty(),
            "the Documentation section links nothing"
        );

        for target in &linked {
            assert!(
                root.join(target).is_file(),
                "README.md links {target}, which does not exist"
            );
        }
        let documents: Vec<String> = fs::read_dir(root.join("docs"))
            .expect("docs/ is readable")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some(format!("docs/{name}"))
            })
            .collect();
        assert!(!documents.is_empty(), "docs/ holds no document");
        for document in documents {
            assert!(
                linked.contains(&document),
                "the README documentation table does not link {document}"
            );
        }
    }

    /// The README's layout table is the map a reader opens first. A workspace
    /// member it leaves out is a crate nobody reading the table knows exists,
    /// and a row naming a path that is gone describes nothing.
    #[test]
    fn the_layout_table_names_every_member_and_nothing_gone() {
        let root = repo_root();
        let readme = read("README.md");
        let layout = section(&readme, "Layout").expect("README.md has a Layout section");
        let rows = first_column(&layout);
        assert!(!rows.is_empty(), "the Layout section names no path");

        for row in &rows {
            assert!(
                root.join(row).is_dir(),
                "README.md's layout names {row}, which is not a directory"
            );
        }
        let workspace: toml::Value =
            toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        for member in members(&workspace) {
            assert!(
                rows.contains(&member),
                "README.md's layout table leaves out the member {member}"
            );
        }
    }

    /// Every `EMBER_` name a document here gives is one this repository's code
    /// reads or a workflow here holds as a secret, and every variable the code
    /// reads is named in `docs/dev.md`. A name copied from Ember's documents
    /// describes a variable nothing here reads. Ember's own documents name
    /// Ember's variables, and this repository points at them.
    ///
    /// A read is a Rust string literal that is exactly an `EMBER_` name, in
    /// any member's sources. No code here reads one, so the second half holds
    /// nothing until some code does. What passes: a variable outside the
    /// `EMBER_` namespace, and a name built at run time.
    #[test]
    fn every_documented_variable_is_one_the_code_reads() {
        let root = repo_root();
        let workspace: toml::Value =
            toml::from_str(&read("Cargo.toml")).expect("workspace manifest");
        let own = root.join("xtask").join("src").join("policy.rs");
        let mut read_names: BTreeSet<String> = BTreeSet::new();
        for member in members(&workspace) {
            for source in rust_sources(&root.join(member)) {
                if source == own {
                    continue;
                }
                read_names.extend(ember_variable_literals(&read_path(&source)));
            }
        }

        let mut secrets: BTreeSet<String> = BTreeSet::new();
        for (_, text) in workflows() {
            for (at, _) in text.match_indices("secrets.") {
                let rest = &text[at + "secrets.".len()..];
                if rest.starts_with("EMBER_") {
                    secrets.extend(ember_names(rest).into_iter().take(1));
                }
            }
        }

        let documented: BTreeSet<String> = ember_names(&read("docs/dev.md")).into_iter().collect();
        let undocumented: Vec<&String> = read_names.difference(&documented).collect();
        assert!(
            undocumented.is_empty(),
            "the code reads {undocumented:?} and docs/dev.md never names them"
        );

        let mut documents = 0;
        let mut stray: Vec<String> = Vec::new();
        for (path, text) in prose_texts() {
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            documents += 1;
            for name in ember_names(&text) {
                if !read_names.contains(&name) && !secrets.contains(&name) {
                    stray.push(format!("{}: {name}", shown(&path)));
                }
            }
        }
        assert!(
            documents > 0,
            "no document was read, so nothing was checked"
        );
        assert!(
            stray.is_empty(),
            "a document names a variable no code here reads and no workflow holds: {stray:?}"
        );
    }

    /// A variable read is a literal that is the whole name, and a mention in
    /// prose is a name at a word boundary.
    #[test]
    fn the_variable_readers_take_the_shapes_they_name() {
        let source = "const A: &str = \"EMBER_ONE\";\n\
                      // const B: &str = \"EMBER_COMMENTED\";\n\
                      #[ignore = \"needs EMBER_TWO\"]\n\
                      let c = var(\"EMBER_THREE_3\");\n\
                      let d = \"XEMBER_FOUR\";\n";
        assert_eq!(
            ember_variable_literals(source),
            ["EMBER_ONE", "EMBER_THREE_3"]
        );
        assert_eq!(
            ember_names("`EMBER_ONE` and EMBER_TWO, not XEMBER_THREE"),
            ["EMBER_ONE", "EMBER_TWO"]
        );
        assert!(ember_names("").is_empty());
    }

    /// A path under a build directory names the placeholder, never a build.
    /// Prose that names one is true of that build and reads as true of
    /// whichever build is current. This repository keeps no build record, so
    /// the check reads the shape of the path rather than a list of ids.
    ///
    /// What passes: a placeholder such as `<buildid>`; a path in code rather
    /// than prose, which is where test fixtures build theirs; and an issue
    /// form's placeholder value, which is an example by construction.
    #[test]
    fn no_prose_names_a_build_directory_by_its_id() {
        let mut named: Vec<String> = Vec::new();
        for (path, text) in prose_texts() {
            let in_forms = path
                .components()
                .any(|part| part.as_os_str() == "ISSUE_TEMPLATE");
            let text = if in_forms {
                without_placeholders(&text)
            } else {
                text
            };
            for paragraph in prose_paragraphs(&path, &text) {
                for directory in build_directory_ids(&paragraph) {
                    named.push(format!("{}: {directory}", shown(&path)));
                }
            }
        }
        assert!(
            named.is_empty(),
            "prose names a build directory by a build's id. Write the placeholder instead: \
             {named:#?}"
        );
    }

    /// The build directory reader takes the shapes it names.
    #[test]
    fn build_directory_ids_read_a_concrete_id_and_nothing_else() {
        let cases: &[(&str, &[&str])] = &[
            (
                "  server/23178631/     The fetched server",
                &["server/23178631"],
            ),
            ("`.cache\\schema\\1024`", &["schema/1024"]),
            ("server/<buildid>/", &[]),
            ("tools/watch-builds.ts 2278520", &[]),
            ("runs/12", &[]),
            ("", &[]),
        ];
        for (text, expected) in cases {
            assert_eq!(build_directory_ids(text), *expected, "{text:?}");
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
          bun-version-file: .bun-version
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
