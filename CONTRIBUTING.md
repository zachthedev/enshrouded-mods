# Contributing

## Setup

Install the hooks before the first commit:

```sh
bun install
```

`lefthook.yml` holds them: `commit-msg` runs commitlint, and `pre-push` runs
the gate. [lefthook](https://lefthook.dev) installs them into `.git/hooks` when
`bun install` runs the `prepare` script. Each hook starts its tool through
`bunx --bun --no-install`, which runs the copy in `node_modules`, or one from
a parent directory or `PATH` when the install is missing. Hooks run in your shell's environment, so unset `BUN_OPTIONS`,
`BUN_INSPECT`, `BUN_INSPECT_CONNECT_TO` and `BUN_INSPECT_PRELOAD` before you
commit if you ever set them. The hooks themselves need `node_modules`:
in a fresh clone before `bun install`, or once `node_modules` is gone, no hook
runs and every commit and push goes through unchecked. Continuous integration's
`commits` job and gate are the control that holds either way. If
`git config core.hooksPath` prints a path, unset it first: git ignores
`.git/hooks` while that setting names another directory.

[docs/dev.md#prerequisites](docs/dev.md#prerequisites) lists what to install
and the file that pins each version.

## The gate

One command, and the only one:

```sh
cargo xtask check
```

It runs its rows in order and stops at the first failure. The rows that read
files run before the rows that build and run repository code. The rows, and what
each one covers, are printed by the same table the gate runs:

```sh
cargo xtask check --rows
```

The opening row holds `mise.toml`, `mise.semver.toml` and their lockfiles to
their rules before any tool runs, and `cargo xtask pins` runs that row on its own. A lockfile entry
the rules reject installs whatever its url serves, so a rule that ran later
would report a finding about a binary that had already executed. It also
refuses any link, and any other mise configuration or lockfile, at the root or
under `.config`, `.mise` or `mise`. mise would merge such a file and its
lockfile over the pair the rules read, and follow a link to whatever it names.
`mise.toml` holds `[tools]`, `[tool_config]` and `[settings]` alone, and each
tool entry holds its version and tag prefix alone. mise runs hooks, tasks and
postinstall commands from that file, and no rule reads them.

The same row runs the tree rules in `xtask/src/tree.rs`. rustfmt,
cargo-deny, taplo, zizmor, Prettier and commitlint each run with their one
config named, and none of them reads another under that flag. clippy searches
upward from the root, where the committed `clippy.toml` stops it, and a root
`.clippy.toml`, which would win beside it, is refused. A program that
finds its config by name with no flag naming one has every other name refused:
a second lefthook config, an actionlint config, a nested `.cargo/config` or
toolchain file, and a root `.config` directory, which commitlint's cosmiconfig
reads even under `--config`. `tree.rs` lists every name.
Such a file is refused on disk, tracked or not, so a local run agrees with
continuous integration. A personal file, such as an env file Bun loads or a
`lefthook-local` config, is refused only when tracked, and `.gitignore` lists
it. The rules run again before every later row, since the build and test rows
run repository code. They read the tree through git, and refuse to when the
work tree git names is not the root: a `.git` that holds no repository sends
git to the repository above, and a `core.worktree` setting sends it elsewhere.

What a config holds is for a reviewer to judge, and CODEOWNERS sends every
change to one to a code owner. The rules refuse only a key that runs or
redirects code from a file that reads as data:

- `bunfig.toml` holds `[install] minimumReleaseAge`, and nothing else.
- `.cargo/config.toml` holds the `xtask` alias, and nothing else.
- No `package.json` carries a `cosmiconfig` key, which commitlint's cosmiconfig
  reads even under `--config`.
- `rust-toolchain.toml` names a channel and its components, and nothing else.
- No TypeScript project config sets `paths`, `baseUrl` or `noCheck`, and
  one the gate does not name is refused.

Every JavaScript tool a row or a hook starts runs through
`bunx --bun --no-install`, which fetches nothing. bunx runs a copy from a
parent directory or `PATH` when the checkout holds none, so a row first checks
that `node_modules/.bin` holds its tool. Every Bun the gate starts itself, a script
it evaluates or `bun test`, carries `--no-env-file`. bunx passes that flag to
no tool it starts, so an untracked env file reaches those. The opening row also refuses a
`package.json` carrying `patchedDependencies`, since a patch changes an
installed package away from the release `bun.lock` pins. No child gets
`BUN_OPTIONS`, which Bun reads into every process as flags, a preload or a test
filter among them.

`cargo xtask` runs `--locked`, and so does every cargo row, so a manifest edit
with no relock is refused before the gate starts rather than rewriting
`Cargo.lock`. Every row that walks the tree hands its tool the tracked files it
reads, fails when none was handed, and prints the count. Where the tool names
what it read, as rustfmt, taplo, actionlint and zizmor do, the row reads that
back and fails on a file it skipped. Prettier is handed exactly the files its
own file info keeps. cargo-machete names each crate directory it visited, and
the row fails when it says it could not read one. A tool whose output a row
reads runs with `NO_COLOR` set, and the row strips any color or link code
before it reads. `cargo machete` is
handed each tracked crate's directory with ignore files off, so
Ember's checkout under `vendor/` never enters it.

`actionlint` checks workflow syntax, runner labels and every expression,
including whether a `needs.<job>.outputs.<name>` names an output that job
declares. It runs an external analyzer when it finds one on `PATH` and says
nothing at all when it does not, so an absent analyzer is a pass for a pass
nobody ran, and no flag changes that. pyflakes is therefore off, because no
Windows package manager ships it and off is the only setting both matrix legs
agree on. ShellCheck runs through a stand-in. `-shellcheck` names this xtask
under a hidden subcommand, which reads each `run:` script as actionlint decoded
it. The stand-in refuses any line holding a ShellCheck directive, and otherwise
runs the ShellCheck mise resolved over the same bytes. A directive drops a
finding from the report, and YAML escapes and folding hide one from any reading
of the workflow file. The row first runs two canary workflows. One must come
back with `SC2086`, and the other, which carries a directive, must come back
refused. actionlint hands only a bash or sh script to ShellCheck, so the row
also reads every step's `shell:` through Bun's YAML parser and refuses any shell
but bash, sh and pwsh. A pwsh block is not shell ShellCheck can read.

`zizmor` audits the same files for supply chain and credential problems: an
action not pinned to a commit, a checkout that leaves the job token behind, a
workflow with no `permissions` block, and expression injection through untrusted
context. It is given `.github` with `--collect=all`, which turns off every
ignore file, so a committed ignore line cannot hide a workflow, and it never
reaches the workflows in Ember's checkout under `vendor/`.
`--strict-collection` makes a file it cannot parse fail the step rather than
drop out of the audit. Locally it runs online when `gh auth token` answers, so
the audits that read the GitHub API run before a push, and `--offline`
otherwise. The row's note says which. In CI it always runs offline and holds no
token. Those audits catch an impostor commit, an advisory against a pinned
action and a version comment naming the wrong tag, and they run in CI's shared
`workflows` job on every pull request. `--config` names `.github/zizmor.yml`, which holds the hash-pin
policy, the Dependabot cooldown threshold and every waiver, so the environment
cannot swap it for another. The opening row refuses an inline `zizmor: ignore`
comment under `.github`, so nothing waives an audit outside that file. A second pass with no config and no
ignores fails unless every job passing `secrets: inherit` calls a workflow under
`zachthedev/.github/.github/workflows/`. That hold is what lets `zizmor.yml`
waive the audit by file.

`tests` fails when no test ran, a run that skipped every test included, and
reads no nextest user config. No child gets a `NEXTEST_` variable, since one
can pass such a run or retry a failing test into a pass.

`doctests` runs beside `tests`, because `cargo nextest` runs none of them and a
doctest that stops compiling would otherwise pass the gate in silence. It
counts every documented example and fails on a filtered run, on every example
ignored, and on none. A repository that holds none yet declares that with
`NO_DOC_EXAMPLES` beside the step table, and the row fails once it counts one.
No child gets `RUSTDOCFLAGS`, `CARGO_BUILD_RUSTDOCFLAGS` or
`CARGO_ENCODED_RUSTDOCFLAGS`, where a test filter drops every example. `doc`
builds every crate's documentation with warnings denied, so a broken link is a
failure.

A missing tool stops the gate and names itself, because a check that did not
run is not a check that passed.

The pre-push hook and continuous integration call the same command, so neither
can run a different gate. Continuous integration adds what one machine cannot
check:

- It runs the gate on Windows and on Linux, because the pre-push hook runs it
  on whichever host a contributor uses, and the mod builds wherever Ember does.
- `commits` runs commitlint over every commit in a pull request, because the
  `commit-msg` hook checks one commit on one machine and a rebase or a
  `--no-verify` reaches the branch unchecked.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), enforced by the
`commit-msg` hook and by the `commits` job. `.github/commit-scopes.json` holds
the scope list, one sentence per scope saying what it covers.
`cargo xtask scopes` prints it, and `commitlint.config.js` enforces it.

A new mod earns a scope. Omit the scope rather than invent one.

The header and every body line stop at 72 columns; commitlint refuses longer.
The subject is imperative and lowercase with no trailing period.

A pull request's title takes the type of its most user-facing commit, and `!`
when any commit breaks something users see. A squash lands the title alone,
and release-plz cannot recover a break the title dropped.

github.com cuts a commit subject at 73 characters, so the 72 applies to the
header that lands. A squash appends ` (#N)` to the title, and the `commits`
job lints the title with it appended. A Dependabot pull request whose landed
header runs past 72 fails that job. It is closed, and its bump is taken by
hand.

A revert is `revert(<scope>): <what is undone, in fresh words>`, with a
`Refs: <sha>` footer naming each reverted commit. git's own
`Revert "<header>"` subject carries no type, so neither the changelog nor the
release decision sees it, and repeating the reverted header overflows 72
columns.

A body says what was wrong, what the change does now, and what a reader needs
that the diff cannot show, such as what was deliberately not done. Past tense
belongs here and nowhere else: a code comment describes the code as it is, and
the commit message carries the history.

## Where code goes

Ask what the code describes:

- Keen's engine or the Enshrouded server:
  [Ember](https://github.com/zachthedev/enshrouded-ember), not here
- Something every mod here shares: `crates/mods-common`
- One mod's own idea: that mod, under `mods/<mod>/`, with its documentation
  beside it under `mods/<mod>/docs/`

`cargo xtask server ...` and `cargo xtask schema ...` forward to Ember's xtask
through the submodule, with `--root` set to this repository, so everything they
write stays under `.cache` here.

## Tests

- Every command the gate runs goes through the `Runner` seam in
  `xtask/src/runner.rs`, so a test drives the step table with a fake and
  spawns nothing. The same seam carries the release bundle and the forwards
  to Ember's xtask.
- A test that needs a fetched server finds it under `.cache` and never under a
  Steam library, and is `#[ignore]` so the suite runs on a machine with none.
  [docs/dev.md#tests-that-need-a-real-thing](docs/dev.md#tests-that-need-a-real-thing)
  lists them.
- A case states its expectation as a literal, never as a value computed from
  the subject under test. An expectation derived from the subject can only
  restate it.

## Code

- A lint waiver is an `expect` naming the one lint it waives, with a reason
  holding a letter or a digit, so a waiver whose lint stops firing fails the
  build. clippy refuses an `allow` wherever its cfg holds, and
  `cargo xtask pins` refuses one on every platform. It refuses a waiver naming
  a lint group, and `rustfmt::skip` in any form, `rustfmt_skip` and a raw
  identifier included. It also
  refuses any attribute naming `allow_attributes` or
  `allow_attributes_without_reason`, and a crate whose `[lints]` holds anything
  but `workspace = true`. The scan reads each tracked `.rs` file as Rust
  tokens, so it refuses an attribute built from a macro argument, a module file
  set by `#[path]` even under `cfg_attr`, `include!`, and a `use` that
  imports `include` under another name.
- A crate declares only the dependencies its code uses. `cargo xtask pins`
  refuses a cargo-machete ignore list in any `Cargo.toml`.
- A TypeScript file keeps tsc's checking on. `cargo xtask pins` refuses
  `@ts-nocheck`, `@ts-ignore` and an `@ts-expect-error` with no reason.
- A module that needs unsafe code takes an `expect(unsafe_code)` with a
  reason on its `mod` line, so the list of those attributes is the list of
  modules that hold any.
- A mod is a `cdylib` plus an `rlib` that builds wherever `ember-sdk` does, so
  nothing in a mod carries a platform `cfg` of its own.
- A comment explains a constraint the reader can verify today. What was wrong
  before, and why an earlier approach failed, goes in the commit message.

## Dependencies

`bunfig.toml` sets the install cooldown for Bun and travels with the clone, so
a container run with no user-level configuration sees the same gate. Cargo has
no cooldown file of its own: every version bump comes through Renovate, and the
preset `.github/renovate.json` extends holds the cooldown. `renovate.json` adds
what is true of this repository alone, and says why beside each entry.

`ember-sdk` comes from crates.io, and Renovate takes a new release of it with no
cooldown, because the crate is this author's own. For a change that spans both
repositories, `.cargo/ember-local.toml` points it at Ember's checkout under
`vendor/`; [docs/dev.md](docs/dev.md#building-against-embers-source) says how.
Renovate moves that submodule's pointer with no cooldown as well, because a git
ref carries no release timestamp for a cooldown to read.

The gate's `deny` row runs `cargo deny check licenses bans sources`.
Advisories are not a row, because an advisory published overnight would turn
a change red that touched nothing. Two legs read the lockfile for them, and
they fail in opposite directions: Dependabot alerts read GitHub's database on
every push, which lacks part of RustSec, and `.github/workflows/audit.yml`
runs `cargo deny check advisories` against RustSec weekly, reading the
`[advisories]` table in `deny.toml`. A red audit run is a report, never a
check, and no ruleset requires it.

An advisory is fixed by the tool that sees the crate. A crate `Cargo.toml`
names gets Renovate's security pull request, which skips the schedule and the
cooldown. A transitive crate gets Dependabot's, a lockfile-only bump inside the
parent's range; `.github/dependabot.yml` opens that kind of pull request and no
other. When no fixed release satisfies the requirement, bump the direct
dependency that pulls it in.

## Releases

[release-plz](https://release-plz.dev) releases the workspace, configured in
`release-plz.toml`, and `.github/workflows/cd.yml` runs it. Nothing here is
published to a registry: a mod ships as one archive on its GitHub release.

1. Every push to main runs `release-update`, which computes each changed
   mod's next version and changelog entry, then `release-pr`, which applies
   that change and opens or updates one release pull request under the
   zachthedev-releaser app.
2. Merging that pull request, as a squash, is the release. The next run waits
   for the `release` environment's reviewer, then tags each mod the pull
   request names `<mod>-v<version>` and drafts its GitHub release.
3. `cargo xtask package` builds the mod's archive from that commit, with
   Ember's loader downloaded from Ember's own release and held to its digest
   file, and writes a `SHA256SUMS` beside it.
4. The publish job attaches both files, records a build provenance
   attestation, waits for the same reviewer a second time, and flips the draft
   public. Two approvals per release is the cost of creating every release as
   a draft.

release-plz's semver check runs in `release-update` and nowhere else. It
builds rustdoc for each changed library and its last release, which runs every
dependency's build script and proc macro, so that job holds no credential.
Its log carries the verdict, and the release pull request carries none.
`mise.toml` pins release-plz, and `mise.semver.toml` pins cargo-semver-checks,
which mise loads only where `MISE_ENV=semver` and so never beside the
releaser's key.

release-plz owns every version in the manifests and every crate's
`CHANGELOG.md`, beside the crate's `Cargo.toml`. Nobody edits either by hand;
to change what a release says, edit the release pull request before merging
it. A red release pull request is never merged with `--admin`, because the
bypass also skips the required checks.

A user-facing change to a mod or to `crates/mods-common` releases every mod: a
`feat`, `fix`, `perf` or `revert` subject, or a breaking change of any type.
Every other type is hidden from the changelog and releases nothing. The
workspace shares one version, so the released crates are one `version_group`
and move together. mods-common takes a tag and no GitHub release, and a mod's
release notes carry mods-common's commits. The commit type sets the changelog
section and the bump size, and below 1.0.0 a `feat` bumps the patch and a
breaking change the minor. The workspace starts at 0.1.0 because nothing
depends on it yet, and `0.x` promises no compatibility.

Every version heading in a changelog links GitHub's compare view from the
previous tag, which lists every change in the release, hidden types included.
`git log --oneline <mod>-v<old>..<mod>-v<new>` lists the same, and each GitHub
release carries GitHub's generated notes after its changelog.

No mod is released until Ember's release carries the loader, because
`cargo xtask package` downloads it from there. The command also refuses a
lockfile in which any Ember crate comes from somewhere other than crates.io: a
directory or a git checkout can carry a release's version number without its
code.

A failed release is recovered by cutting the next version, never by moving a
tag. Only the releaser app can create a tag.

## What never happens

- Nobody hand-edits a version in a `Cargo.toml` or a crate's `CHANGELOG.md`.
  release-plz writes both from the commits, as [Releases](#releases) says, and
  a hand edit is overwritten or shifts the next version it computes.
- No commit message leaves the convention [Commit messages](#commit-messages)
  sets. release-plz computes each version and changelog entry from the
  messages, so a message outside it becomes a wrong entry in a release.
- No path inside a Steam library is ever written to, launched, or injected into.
  A dedicated server for development is fetched separately into `.cache`.
  `.claude/settings.json` carries two `deny` entries that refuse an agent an
  edit under a Steam library, because a rule read is a rule that can be
  forgotten and a deny cannot.
- No schema dump, string table, protocol registry or other recovered game data
  is committed. The extractors are committed; their output is not.
