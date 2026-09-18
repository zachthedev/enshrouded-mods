//! Tests over the repository's own configuration.
//!
//! Each test reads a committed file and asserts a property the supply chain
//! depends on: what continuous integration runs is pinned, what a Claude Code
//! session may write is fenced, and no hook fetches from a registry. They live
//! here because the files they read have no other test.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

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

/// A git commit hash is forty hexadecimal characters.
fn is_commit_hash(reference: &str) -> bool {
    reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit())
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

/// Every string literal in a JavaScript source, paired with the bracket depth
/// it sits at, in source order.
///
/// Characters inside a literal open no bracket and comments are skipped, so a
/// URL, an apostrophe in a sentence and a commented-out list all read
/// correctly.
fn js_string_literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut depth: usize = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'*') => {
                index += 2;
                while index < chars.len()
                    && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/'))
                {
                    index += 1;
                }
                index += 2;
            }
            '[' => {
                depth += 1;
                index += 1;
            }
            ']' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            quote @ ('\'' | '"' | '`') => {
                index += 1;
                let mut text = String::new();
                while index < chars.len() && chars[index] != quote {
                    if chars[index] == '\\' {
                        index += 1;
                        if index >= chars.len() {
                            break;
                        }
                    }
                    text.push(chars[index]);
                    index += 1;
                }
                index += 1;
                found.push((depth, text));
            }
            _ => index += 1,
        }
    }
    found
}

/// The scope list `commitlint.config.js` enforces.
///
/// The `scope-enum` rule is `[level, applicability, [scopes]]`, so the scopes
/// are the run of literals two brackets deeper than the rule's own name. An
/// empty result means the file declares no such rule.
fn commitlint_scopes(source: &str) -> Vec<String> {
    let literals = js_string_literals(source);
    let Some(rule) = literals.iter().position(|(_, text)| text == "scope-enum") else {
        return Vec::new();
    };
    let wanted = literals[rule].0 + 2;
    literals[rule + 1..]
        .iter()
        .skip_while(|(depth, _)| *depth != wanted)
        .take_while(|(depth, _)| *depth == wanted)
        .map(|(_, text)| text.clone())
        .collect()
}

/// One `##` section of a markdown document, its heading excluded.
///
/// A heading is a stabler anchor than the sentence under it, so a section can
/// be reworded without moving what reads it.
fn section<'a>(markdown: &'a str, heading: &str) -> Option<&'a str> {
    let opening = format!("\n## {heading}\n");
    let start = markdown.find(&opening)? + opening.len();
    let rest = &markdown[start..];
    Some(rest.find("\n## ").map_or(rest, |end| &rest[..end]))
}

/// Every inline code span in a markdown fragment.
fn code_spans(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Every paragraph of a markdown document that is a bare list of inline code
/// spans, as the spans it holds.
///
/// Anchoring on the paragraph's shape rather than on the sentence above it
/// means rewording the section around it leaves the check working.
fn code_span_lists(markdown: &str) -> Vec<Vec<String>> {
    markdown
        .split("\n\n")
        .filter_map(|paragraph| {
            let spans = code_spans(paragraph);
            if spans.is_empty() {
                return None;
            }
            let mut rest = paragraph.to_string();
            for span in &spans {
                rest = rest.replacen(&format!("`{span}`"), "", 1);
            }
            rest.chars()
                .all(|c| c == ',' || c == '.' || c.is_whitespace())
                .then_some(spans)
        })
        .collect()
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
    use std::fs;

    use std::collections::BTreeSet;

    use super::{
        cargo_install_package, code_span_lists, commitlint_scopes, is_commit_hash, mentions,
        repo_root, rust_sources, section, workflow_uses, workspace_inherited,
    };

    fn read(relative: &str) -> String {
        let path = repo_root().join(relative);
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    /// A mutable tag is retargeted by its owner with no pull request and no
    /// cooldown, so every action is pinned to a commit and the tag it stands
    /// for is kept as a comment for Dependabot to bump.
    #[test]
    fn ci_pins_every_action_to_a_commit() {
        let uses = workflow_uses(&read(".github/workflows/ci.yml"));
        assert!(!uses.is_empty(), "the workflow declares no actions");

        for (action, reference, comment) in uses {
            assert!(
                is_commit_hash(&reference),
                "{action} is pinned to {reference:?}, not a commit hash"
            );
            assert!(
                comment.starts_with('v') && comment[1..].starts_with(|c: char| c.is_ascii_digit()),
                "{action} carries no version comment, got {comment:?}"
            );
        }
    }

    /// The token stays read-only, so a compromised action on a runner cannot
    /// push, release or write to the repository.
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

    /// The workflow reads the pinned file rather than carrying its own copy of
    /// the versions.
    #[test]
    fn ci_reads_the_pinned_tool_file() {
        let text = read(".github/workflows/ci.yml");

        assert!(
            text.contains(".github/cargo-tools"),
            "the workflow does not read the pinned tool file"
        );
        for (tool, version) in pinned_tools() {
            assert!(
                !text.contains(&format!("{tool}@{version}")),
                "the workflow carries its own copy of {tool}@{version}"
            );
        }
    }

    /// A document that restates a version is a copy that drifts, so the docs
    /// point at the file instead.
    #[test]
    fn no_document_restates_a_pinned_tool_version() {
        let pinned = pinned_tools();

        for doc in ["CONTRIBUTING.md", "docs/dev.md", "README.md"] {
            let text = read(doc);
            for (tool, version) in &pinned {
                assert!(
                    !text.contains(&format!("{tool}@{version}")),
                    "{doc} restates {tool}@{version}"
                );
            }
        }
    }

    /// The commit scope vocabulary lives in three places: the constant
    /// `cargo xtask scopes` prints, the rule the commit hook enforces, and the
    /// sentence `CONTRIBUTING.md` restates. A contributor who reads one and a
    /// hook that enforces another disagree silently.
    #[test]
    fn every_copy_of_the_scope_list_agrees() {
        let declared: Vec<String> = crate::SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect();
        assert!(!declared.is_empty(), "xtask/src/main.rs declares no scopes");

        let commitlint = commitlint_scopes(&read("commitlint.config.js"));
        let contributing = read("CONTRIBUTING.md");
        let commits = section(&contributing, "Commit messages")
            .expect("CONTRIBUTING.md has a Commit messages section");
        let documented = code_span_lists(commits);
        assert_eq!(
            documented.len(),
            1,
            "the Commit messages section holds {} paragraphs that are a bare list of code spans, so which one restates the scopes is ambiguous",
            documented.len()
        );

        let mut disagree: Vec<String> = Vec::new();
        if commitlint != declared {
            disagree.push(format!(
                "commitlint.config.js scope-enum has {commitlint:?}"
            ));
        }
        if documented[0] != declared {
            disagree.push(format!("CONTRIBUTING.md restates {:?}", documented[0]));
        }
        assert!(
            disagree.is_empty(),
            "xtask/src/main.rs SCOPES has {declared:?}, and {}",
            disagree.join("; ")
        );
    }

    /// The parser reads the rule's nested array and leaves every other literal
    /// in the file alone, including one inside a comment and one holding a
    /// bracket.
    #[test]
    fn commitlint_scopes_reads_the_nested_rule_array() {
        let cases = [
            (
                "\
export default {
  extends: ['@commitlint/config-conventional'],
  ignores: [(message) => message.includes('Signed-off-by: dependabot[bot]')],
  rules: {
    'scope-enum': [2, 'always', ['one', 'two']],
    'body-max-line-length': [2, 'always', 72],
  },
};
",
                vec!["one", "two"],
            ),
            (
                "\
export default {
  rules: {
    'scope-enum': [
      2,
      'always',
      // A list at https://example.invalid that isn't the rule's own.
      /* 'commented' */
      ['one', 'two', 'three'],
    ],
  },
};
",
                vec!["one", "two", "three"],
            ),
            ("export default { rules: {} };", vec![]),
        ];

        for (source, expected) in cases {
            assert_eq!(commitlint_scopes(source), expected, "{source}");
        }
    }

    /// A paragraph of prose holding code spans is not a list, and a bullet is
    /// not a bare paragraph.
    #[test]
    fn code_span_lists_matches_only_a_bare_list() {
        let markdown = "\
# Title

Run `cargo xtask scopes` for the live list.

`one`, `two`, `three`.

- `four`, `five`
";
        assert_eq!(code_span_lists(markdown), [["one", "two", "three"]]);
    }

    /// A section runs to the next heading of its own level, and a heading that
    /// is not there yields nothing rather than the rest of the file.
    #[test]
    fn section_spans_one_heading_to_the_next() {
        let markdown = "\
# Title

## First

one

## Second

two

## Third

three
";
        assert_eq!(section(markdown, "First"), Some("\none\n"));
        assert_eq!(section(markdown, "Third"), Some("\nthree\n"));
        assert_eq!(section(markdown, "Fourth"), None);
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

    /// The parser reads the three shapes a `uses:` line takes.
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
        assert!(!is_commit_hash("abc123"));
        assert!(is_commit_hash("fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09"));
        assert!(!is_commit_hash("FBC6F3992D24B796D5A048FF273F7FCC4A7B6C0G"));
    }
}
