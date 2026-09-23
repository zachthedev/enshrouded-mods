# Contributing

## Setup

Install the hooks before the first commit:

```sh
bun install
```

`lefthook.yml` holds them: `commit-msg` runs commitlint, and `pre-push` runs
the gate. [lefthook](https://lefthook.dev) installs them into `.git/hooks` when
`bun install` runs the `prepare` script. Each hook resolves its tool through
`bunx --no-install`, so a tool that is not installed fails the commit or the
push rather than letting it through. If `git config core.hooksPath` prints a
path, unset it first: git ignores `.git/hooks` while that setting names another
directory.

[docs/dev.md#prerequisites](docs/dev.md#prerequisites) lists what to install
and the file that pins each version.

## The gate

One command, and the only one:

```sh
cargo xtask check
```

It runs its rows in order and stops at the first failure. The rows, and what
each one covers, are printed by the same table the gate runs:

```sh
cargo xtask check --rows
```

The opening row holds `mise.toml` and `mise.lock` to their rules before any
tool runs, and `cargo xtask pins` runs that row on its own. A lockfile entry
the rules reject installs whatever its url serves, so a rule that ran later
would report a finding about a binary that had already executed.

`cargo xtask` runs `--locked`, and so does every cargo row, so a manifest edit
with no relock is refused before the gate starts rather than rewriting
`Cargo.lock`. `taplo` reads `.taplo.toml` for the files it covers, and leaves
Ember's checkout under `vendor/` alone; `cargo machete` reads `.ignore` for
the same exclusion.

`actionlint` checks workflow syntax, runner labels and every expression,
including whether a `needs.<job>.outputs.<name>` names an output that job
declares. It runs an external analyzer when it finds one on `PATH` and says
nothing at all when it does not, so an absent analyzer is a pass for a pass
nobody ran, and no flag changes that. pyflakes is therefore off, because no
Windows package manager ships it and off is the only setting both matrix legs
agree on. shellcheck is on, by the path mise resolved for the pinned release,
and the row first runs it over a canary workflow with one unquoted expansion
and is refused unless that run reports `SC2086`. What the analysis reads is
the shell in a `run:` block actionlint resolves to sh or bash; a block declaring
`shell: pwsh`, and every block in the `gate` job, is not shell it can read.

`zizmor` audits the same files for supply chain and credential problems: an
action not pinned to a commit, a checkout that leaves the job token behind, a
workflow with no `permissions` block, and expression injection through untrusted
context. It is given `.github/workflows` and `.github/dependabot.yml` by name,
so it never reaches the workflows in Ember's checkout under `vendor/`.
`--strict-collection` makes a file it cannot parse fail the step rather than
drop out of the audit. It runs online when `gh auth token` answers, so the
audits that read the GitHub API run, and `--offline` otherwise; the row's note
says which. `--config` names `.github/zizmor.yml`, which holds the hash-pin
policy and the Dependabot cooldown threshold, so the environment cannot swap it
for another.

`doctests` runs beside `tests`, because `cargo nextest` runs none of them and a
doctest that stops compiling would otherwise pass the gate in silence. `doc`
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

- A module that needs unsafe code takes an `allow(unsafe_code)` with a reason
  on its `mod` line, so the list of those attributes is the list of modules
  that hold any. No module takes one today.
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

1. Every push to main opens or updates one release pull request under the
   zachthedev-releaser app. It carries each changed mod's next version and its
   changelog entry.
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

release-plz owns every version in the manifests and every crate's
`CHANGELOG.md`, beside the crate's `Cargo.toml`. Nobody edits either by hand;
to change what a release says, edit the release pull request before merging
it. A red release pull request is never merged with `--admin`, because the
bypass also skips the required checks.

A commit that changes the packaged files of a mod or of `crates/mods-common`
releases every mod: the workspace shares one version, so release-plz moves
every releasable crate together. mods-common takes a tag and no GitHub release,
and a mod's release notes carry mods-common's commits. The commit type sets the
changelog section and the bump size, and below 1.0.0 a `feat` bumps the patch
and a breaking change the minor. The workspace starts at 0.1.0 because nothing
depends on it yet, and `0.x` promises no compatibility.

No mod is released until Ember's release carries the loader, because
`cargo xtask package` downloads it from there. The command also refuses a
lockfile in which any Ember crate comes from somewhere other than crates.io: a
directory or a git checkout can carry a release's version number without its
code.

A failed release is recovered by cutting the next version, never by moving a
tag. Only the releaser app can create a tag.

## What never happens

- No path inside a Steam library is ever written to, launched, or injected into.
  A dedicated server for development is fetched separately into `.cache`.
  `.claude/settings.json` carries two `deny` entries that refuse an agent an
  edit under a Steam library, because a rule read is a rule that can be
  forgotten and a deny cannot.
- No schema dump, string table, protocol registry or other recovered game data
  is committed. The extractors are committed; their output is not.
