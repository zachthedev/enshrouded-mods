# Contributing

## Setup

### Prerequisites

- Rust, at the release `rust-toolchain.toml` pins. rustup installs it on the
  first cargo command.
- A C toolchain, for MinHook, the hook engine Ember uses. On Windows that is
  Visual Studio Build Tools.
- [Bun](https://bun.sh), for the repository's own tooling, at the release
  `packageManager` in `package.json` pins. Continuous integration reads the
  same field.
- [mise](https://mise.jdx.dev), for every gate tool that rustup, cargo and bun
  do not provide. `mise.toml` pins a version per tool and `mise.lock` records
  a checksum per platform, so an install takes the recorded artifact or fails.
  `mise.semver.toml` and `mise.semver.lock` are the same pair for
  cargo-semver-checks alone, which only the release workflow loads.
- Ember, as the submodule at `vendor/enshrouded-ember`, for `cargo xtask server`
  and `schema`, which run Ember's xtask from it. The mods themselves build
  against Ember's crates.io release, at the requirement `Cargo.toml` names.
- git, which the gate starts to list the tracked files.
- [gh](https://cli.github.com), optional. With a login, the gate's zizmor row
  runs its online audits, and without one it runs them offline.

Install mise, then:

```sh
cargo xtask setup
```

That holds `mise.toml` and `mise.lock` to their rules first, then installs what
the lockfile records, under the environment the gate gives every mise child. A
bare `mise install` reads a committed `mise.local.toml` before any rule runs.
The install asks api.github.com for the attestations of each tool mise's
registry does not name, and a caller with no token gets 60 requests an hour. If
it reports a rate limit, set `MISE_GITHUB_TOKEN` for that one command. The
install is the only mise child that receives it. Set it only in a checkout you
trust, because `cargo xtask` builds and runs the checkout's own code with the
environment it inherits.

Nothing from that lands on `PATH`. The gate asks `mise which` for each binary
and runs the path it gives back, so the binary it checked is the binary it ran.
Turning on `mise activate` in a shell puts the same binaries on `PATH` under
their own names, which is what makes `cargo nextest run` and its siblings work
at a prompt. The split is deliberate: a check runs the binary it resolved, and a
person gets the convenience. [Dependencies](#dependencies) says how each
download is held to its pin.

### First run

From a fresh clone to a green gate:

```sh
git clone --recurse-submodules https://github.com/zachthedev/enshrouded-mods.git
cd enshrouded-mods
cargo xtask setup             # the gate's tools, at the releases mise.lock records
bun install --frozen-lockfile # the hooks and the markup formatter
cargo xtask server fetch      # a dedicated server, into .cache
cargo xtask schema extract    # the reflection schema, out of that server
cargo xtask check             # the gate
```

An existing clone catches up with `git submodule update --init`. The gate needs
no submodule; `server` and `schema` do.

The fetch pulls a dedicated server from SteamCMD into `.cache`, which is
gitignored. Anonymous login works for app 2278520, so no credentials are
involved. The server runs to gigabytes. Fetch it once.

The extraction reads the fetched server's executable and writes the schema
dumps into `.cache/schema/<buildid>/`. Ember's
[CONTRIBUTING.md#running-it](https://github.com/zachthedev/enshrouded-ember/blob/main/CONTRIBUTING.md#running-it)
says what each one holds. The build id names the directory. It is not what the
loader matches at run time: Ember identifies a build by the CodeView
fingerprint in the image itself, because the build id is not readable from the
running process.

**The extraction is required.** Every contributor runs the committed
extractors against a server they fetched themselves, and what they produce
lives under `.cache` and is regenerated rather than shared.
[What never happens](#what-never-happens) says why none of it is committed. The
loop is fetch, extract, check.

### Hooks

Install the packages and the hooks before the first commit:

```sh
bun install --frozen-lockfile
```

In a linked worktree, run `bun install --frozen-lockfile --ignore-scripts`
instead. The hooks sit in the `.git/hooks` every worktree shares and name the
installing checkout's `node_modules`, so a worktree's install skips the
`prepare` script that rewrites them.

`lefthook.yml` holds them: `commit-msg` runs commitlint, and `pre-push` runs
the gate. [lefthook](https://lefthook.dev) installs them into `.git/hooks` when
`bun install` runs the `prepare` script. Each hook starts its tool through
`bunx --bun --no-install`, which runs the copy in `node_modules`, or one from a
parent directory or `PATH` when the install is missing. If
`git config core.hooksPath` prints a path, unset it first: git ignores
`.git/hooks` while that setting names another directory.

The hooks run in your own environment, and [Safety](#safety) says which of your
settings reach them and why they are no control.

## Safety

Read a pull request's diff before running anything on its branch. The branch
supplies the install, the hooks and the gate, so `bun install`, a commit and
`cargo xtask check` each run code the branch chose, build scripts and
procedural macros included.

Some of your own environment reaches the tools:

- `BUN_OPTIONS` reaches every Bun you start directly: `bun install`, and the
  lefthook install its `prepare` script starts through Bun. Under
  `bun install`, a `--preload` in it runs once, in that `prepare` start, and
  never in the install process itself. A tool started through `bun x` reads
  none itself, and a tool that starts Bun children of its own hands them
  `BUN_OPTIONS` and `BUN_INSPECT_PRELOAD`. No tool here starts one. The gate
  withholds it from every process it starts.
  `BUN_INSPECT`, `BUN_INSPECT_CONNECT_TO` and `BUN_INSPECT_PRELOAD` open an
  inspector or run a module in any Bun that sees them, the gate's own
  included. Leave all four unset.
- A personal env file reaches the JavaScript tools. bunx never passes
  `--no-env-file` to the tool it starts, so Prettier in the gate and commitlint
  in the hooks load an env file at the checkout's root. Every Bun the gate
  starts itself carries the flag.
- The gate withholds from every child the variables that change a result:
  `BUN_OPTIONS`, `SHELLCHECK_OPTS`, the rustdoc flag variables and every
  `NEXTEST_` variable. [The gate](#the-gate) says what each would change.

Hooks are not a control. They run in your shell's environment and clear no
variable. In a fresh clone before `bun install`, or once `node_modules` is
gone, no hook runs and every commit and push goes through unchecked.
Continuous integration's `commits` job and gate decide a merge either way.

Nothing here is ever pointed at a Steam library, as
[What never happens](#what-never-happens) says.

## Running it

The fetched dedicated server is what a mod runs against, never an installed
copy of the game. `server` and `schema` forward to Ember's xtask through the
submodule, with `--root` set to this repository, so every file they write lands
under `.cache` here rather than in Ember's checkout:

```sh
cargo xtask server seed --fixture <path>   # lay a fixture world into a run directory
cargo xtask server run --inject <dll>      # start it, with the loader injected
cargo xtask server logs --follow           # tail it
cargo xtask server stop                    # ask it to shut down, and wait
```

[fixtures/README.md](fixtures/README.md) says what a fixture directory holds.

Arguments pass through untouched, so Ember's own subcommands and flags are the
only ones. Ember's
[CONTRIBUTING.md#running-it](https://github.com/zachthedev/enshrouded-ember/blob/main/CONTRIBUTING.md#running-it)
lists them, and `cargo xtask server -- --help` and
`cargo xtask schema -- --help` print them for the Ember this repository vendors.

A build lands in a directory named for its Steam build id, which is the key
Steam, SteamCMD and the depot manifest all speak:

```text
.cache/
  steamcmd/            SteamCMD itself
  server/<buildid>/    The fetched server
  schema/<buildid>/    What the extractor reads out of it
```

Every byte under `.cache` is regenerated by a fetch or an extract, which is why
none of it is committed.

A path under `steamapps/common` is refused, for the reasons
[What never happens](#what-never-happens) gives.

### Generated files

| File               | Regenerated by                                              |
| ------------------ | ----------------------------------------------------------- |
| `Cargo.lock`       | any cargo build after a manifest edit; the gate runs locked |
| `bun.lock`         | `bun install` after a `package.json` edit                   |
| `mise.lock`        | `mise lock` after a `mise.toml` edit                        |
| `mise.semver.lock` | `MISE_ENV=semver mise lock` after a `mise.semver.toml` edit |

`mise.lock` keeps the hand-computed taplo hashes across a relock at the same
version, as [Dependencies](#dependencies) says.

### Building against Ember's source

The mods build against Ember's crates.io release. For a change that spans both
repositories, `.cargo/ember-local.toml` points `ember-sdk` at the submodule:

```sh
cargo --config .cargo/ember-local.toml build
```

Cargo reads that file only when a command names it. The patch rewrites
`Cargo.lock`, and the gate runs cargo `--locked`, so it refuses the patched
lockfile. Restore the lockfile before a push:

```sh
git checkout Cargo.lock
```

That reverts every uncommitted lockfile edit, not the patch alone, so commit a
dependency change before building against Ember's source.

Renovate keeps the submodule at Ember's newest commit on main, which can run
ahead of the release on crates.io.

## Where code goes

Ask what the code describes:

- Keen's engine or the Enshrouded server:
  [Ember](https://github.com/zachthedev/enshrouded-ember), not here
- Something every mod here shares: `crates/mods-common`
- One mod's own idea: that mod, under `mods/<mod>/`, with its documentation
  beside it under `mods/<mod>/docs/`

## Code

- A lint waiver is an `expect` naming the one lint it waives, with a reason
  holding a letter or a digit, so a waiver whose lint stops firing fails the
  build. clippy refuses an `allow` wherever its cfg holds, and
  `cargo xtask pins` refuses one on every platform. It refuses a waiver naming
  a lint group, and `rustfmt::skip` in any form, `rustfmt_skip` and a raw
  identifier included. It also refuses any attribute naming `allow_attributes`
  or `allow_attributes_without_reason`, and a crate whose `[lints]` holds
  anything but `workspace = true`. The scan reads each tracked `.rs` file as
  Rust tokens, so it refuses an attribute built from a macro argument, a module
  file set by `#[path]` even under `cfg_attr`, `include!`, and a `use` that
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

## Tests

- Every command the gate runs goes through the `Runner` seam in
  `xtask/src/runner.rs`, so a test drives the step table with a fake and
  spawns nothing. The same seam carries the release bundle and the forwards
  to Ember's xtask.
- A test that needs a fetched server finds it under `.cache` and never under a
  Steam library, and is `#[ignore]` so the suite runs on a machine with none.
  No test needs one yet.
- A case states its expectation as a literal, never as a value computed from
  the subject under test. An expectation derived from the subject can only
  restate it.

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
their rules before any tool runs, and `cargo xtask pins` runs that row on its
own. A lockfile entry the rules reject installs whatever its url serves, so a
rule that ran later would report a finding about a binary that had already
executed. It also refuses any link, and any other mise configuration or
lockfile, at the root or under `.config`, `.mise` or `mise`. mise would merge
such a file and its lockfile over the pair the rules read, and follow a link to
whatever it names. `mise.toml` holds `[tools]`, `[tool_config]` and
`[settings]` alone, and each tool entry holds its version and tag prefix alone.
mise runs hooks, tasks and postinstall commands from that file, and no rule
reads them.

The same row runs the tree rules in `xtask/src/tree.rs`. rustfmt,
cargo-deny, taplo, zizmor, Prettier and commitlint each run with their one
config named, and none of them reads another under that flag. clippy searches
upward from the root, where the committed `clippy.toml` stops it, and a root
`.clippy.toml`, which would win beside it, is refused. A program that
finds its config by name with no flag naming one has every other name refused:
a second lefthook config, an actionlint config, a nested `.cargo/config` or
toolchain file, and a root `.config` directory or `package.yaml`, which
commitlint's cosmiconfig reads even under `--config`. `tree.rs` lists every
name. Such a file is refused on disk, tracked or not, so a local run agrees
with continuous integration. A personal file, such as an env file Bun loads or
a `lefthook-local` config, is refused only when tracked, and `.gitignore` lists
it. The rules run again before every later row, since the build and test rows
run repository code. They read the tree through git, and refuse to when the
work tree git names is not the root: a `.git` that holds no repository sends
git to the repository above, and a `core.worktree` setting sends it elsewhere.

What a config holds is for a reviewer to judge, and CODEOWNERS sends every
change to one to a code owner. The rules refuse only a key that runs or
redirects code from a file that reads as data:

- `.cargo/config.toml` holds the `xtask` alias, and nothing else.
- No `package.json` carries a `cosmiconfig` key, which commitlint's cosmiconfig
  reads even under `--config`.
- `rust-toolchain.toml` names a channel and its components, and nothing else.
- No TypeScript project config sets `noCheck`, and one the gate does not name
  is refused.
- No tracked `package.json` carries `patchedDependencies`, since `bun install`
  rewrites each package it names with a patch. The shared `commits` job refuses
  it too, but its check passes a file its `jq` cannot parse.
- No tracked `package.json` or named TypeScript project config repeats a key
  within one object, since Bun keeps the first and a JSON parser the last.

The shared `commits` job refuses the data files that run code before a merge,
reading the committed tree, and the gate does not repeat them: a `bunfig.toml`
key beyond `[install] minimumReleaseAge`, and TypeScript `paths` or `baseUrl`.

Every JavaScript tool a row or a hook starts runs through
`bunx --bun --no-install`, which fetches nothing. bunx runs a copy from a
parent directory or `PATH` when the checkout holds none, so a row first checks
that `node_modules/.bin` holds its tool as a regular file. Every Bun the gate
starts itself, a script it evaluates or `bun test`, carries `--no-env-file`.
bunx passes that flag to no tool it starts, so an untracked env file reaches
those. No child gets `BUN_OPTIONS`, which Bun reads into every process as
flags, a preload or a test filter among them.

`cargo xtask` runs `--locked`, and so does every cargo row, so a manifest edit
with no relock is refused before the gate starts rather than rewriting
`Cargo.lock`. Every row that walks the tree hands its tool the tracked files it
reads, fails when none was handed, and prints the count. Where the tool names
what it read, as rustfmt, taplo, actionlint and zizmor do, the row reads that
back and fails on a file it skipped. Prettier is handed exactly the files its
own file info keeps. cargo-machete names each crate directory it visited, and
the row fails when it says it could not read one. A tool whose output a row
reads runs with `NO_COLOR` set, and the row strips any color or link code
before it reads. `cargo machete` is handed each tracked crate's directory with
ignore files off, so Ember's checkout under `vendor/` never enters it.

`actionlint` checks workflow syntax, runner labels and every expression,
including whether a `needs.<job>.outputs.<name>` names an output that job
declares. It runs an external analyzer when it finds one on `PATH` and says
nothing at all when it does not, so an absent analyzer is a pass for a pass
nobody ran, and no flag changes that. pyflakes is therefore off, because no
Windows package manager ships it and off is the only setting every matrix leg
agrees on. ShellCheck runs through a stand-in. `-shellcheck` names this xtask
under a hidden subcommand, which reads each `run:` script as actionlint decoded
it. The stand-in refuses any line holding a ShellCheck directive, and otherwise
runs the ShellCheck mise resolved over the same bytes. `SHELLCHECK_OPTS` never
reaches it. A directive drops a finding from the report, and YAML escapes and
folding hide one from any reading of the workflow file. The row first runs two
canary workflows. One must come back with `SC2086`, and the other, which
carries a directive, must come back refused. actionlint hands only a bash or sh
script to ShellCheck, so the row also reads every step's `shell:` through Bun's
YAML parser and refuses any shell but bash, sh and pwsh. A pwsh block is not
shell ShellCheck can read.

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
`workflows` job on every pull request. `--config` names `.github/zizmor.yml`,
which holds the hash-pin policy, the Dependabot cooldown threshold and every
waiver, so the environment cannot swap it for another. The opening row refuses
an inline `zizmor: ignore` comment under `.github`, so nothing waives an audit
outside that file. The shared `workflows` job fails unless every job passing
`secrets: inherit` calls a workflow under
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

- It runs the gate on Windows, Linux and macOS, because the pre-push hook runs
  it on whichever host a contributor uses, and the mod builds wherever Ember
  does.
- `commits` runs commitlint over every commit in a pull request, because the
  `commit-msg` hook checks one commit on one machine and a rebase or a
  `--no-verify` reaches the branch unchecked.

A local run that fails, or passes where CI fails, is covered under
[Troubleshooting](#troubleshooting).

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), enforced by the
`commit-msg` hook and by the `commits` job. `.github/commit-scopes.json` holds
the scope list, one sentence per scope saying what it covers.
`cargo xtask scopes` prints it, and `commitlint.config.js` enforces it.

A new mod earns a scope. Omit the scope rather than invent one. A scope that
only repeats the type, such as `ci(ci)`, is never written: the bare type says
the same.

The header and every body line stop at 72 columns; commitlint refuses longer.
The subject is imperative and lowercase with no trailing period.

A pull request's title takes the type of its most user-facing commit, and `!`
when any commit breaks something users see. A one-commit pull request lands its
commit's subject and body. A longer one lands its title, with each commit as a
bullet, and release-plz cannot recover a break the title dropped.

github.com cuts a commit subject at 73 characters, so the 72 applies to the
header that lands. A squash appends ` (#N)` to that header, and the `commits`
job lints what lands with it appended: a pull request's title, or a one-commit
pull request's subject. A Dependabot pull request whose landed header runs past
72 fails that job. It is closed, and its bump is taken by hand.

A revert is `revert(<scope>): <what is undone, in fresh words>`, with a
`Refs: <sha>` footer naming each reverted commit. git's own
`Revert "<header>"` subject carries no type, so neither the changelog nor the
release decision sees it, and repeating the reverted header overflows 72
columns.

A body says what was wrong, what the change does now, and what a reader needs
that the diff cannot show, such as what was deliberately not done. Past tense
belongs here and nowhere else: a code comment describes the code as it is, and
the commit message carries the history.

## Dependencies

`bunfig.toml` sets the install cooldown for Bun and travels with the clone, so
a container run with no user-level configuration sees the same gate. Cargo has
no cooldown file of its own: every version bump comes through Renovate, and the
preset `.github/renovate.json` extends holds the cooldown. `renovate.json` adds
what is true of this repository alone, and says why beside each entry.

`ember-sdk` comes from crates.io, and Renovate takes a new release of it with no
cooldown, because the crate is this author's own. For a change that spans both
repositories, `.cargo/ember-local.toml` points it at Ember's checkout under
`vendor/`, as [Building against Ember's source](#building-against-embers-source)
says. Renovate moves that submodule's pointer with no cooldown as well, because
a git ref carries no release timestamp for a cooldown to read.

Every gate tool comes through mise, and `mise.lock` is generated, so the gate
holds it to something that is not in it. `xtask/src/pins.rs` carries the owner
and the repository every tool's artifacts come from, and the rules refuse a
lockfile entry whose `url` or `backend` names anything else. A bare registry
key in `mise.toml` names no owner, so for those tools that table is the only
record of the account outside the generated file. Moving a tool to another
account takes an edit there, in the same diff as the lockfile it explains.

Each tool's lockfile entry rests on one integrity tier. Provenance means the
publisher attests the release and mise checks the attestation at every install.
A checksum in a pinned tree comes from the registry entry mise resolves through.
A checksum mise hashed at lock time binds every later install to the bytes the
lock fetched, and nothing outside the lockfile vouches for them.

| Tool                | Tier                                |
| ------------------- | ----------------------------------- |
| actionlint          | provenance                          |
| cargo-deny          | a checksum in a pinned tree         |
| cargo-machete       | a checksum mise hashed at lock time |
| cargo-nextest       | provenance                          |
| cargo-semver-checks | a checksum mise hashed at lock time |
| release-plz         | a checksum mise hashed at lock time |
| shellcheck          | a checksum in a pinned tree         |
| taplo               | a checksum hashed here, as below    |
| zizmor              | provenance                          |

`MISE_BACKENDS_<TOOL>` in the environment replaces a tool's backend, and no
mise setting reports it. The gate and `cargo xtask setup` start mise with it
cleared. A bare `mise install` or a shell with `mise activate` reads it.

`mise.toml` sets `locked` twice, under `[settings]` and under `[tool_config]`,
because they are not the same setting. `MISE_LOCKED=false` and a `locked_scopes`
that drops `project` each turn the `[settings]` one off. The `[tool_config]` one
holds regardless of the environment, and mise reads it from the file alone, so
the gate asserts it from the file.

`taplo` is the one tool whose checksum does not come from its publisher. GitHub
began recording a digest for release assets after the taplo release `mise.toml`
pins was published, so its hashes were computed here and committed. They say the
bytes came from that release URL and that every install since has to match them,
which is narrower than a digest the publisher recorded and is not provenance.
Bumping taplo writes a lockfile entry with no checksum at all, which the gate's
own tests refuse, so whoever bumps it computes and commits the new hashes. A
relock at the same version keeps them, so only a bump drops them.

The gate's `deny` row runs `cargo deny check licenses bans sources`.
Advisories are not a row, because an advisory published overnight would turn
a change red that touched nothing. Two legs read the lockfile for them, and
they fail in opposite directions: Dependabot alerts read GitHub's database on
every push, which lacks part of RustSec, and `.github/workflows/audit.yml`
runs `cargo deny check advisories` against RustSec daily, reading the
`[advisories]` table in `deny.toml`. A red audit run is a report, never a
check, and no ruleset requires it.

On every pull request, the shared `dependency-review` job fails on a change
that adds a package with a high advisory. Under Cargo it reads the whole
`Cargo.lock`. Under Bun, GitHub's dependency graph holds `package.json`'s direct
dependencies alone, so a package they pull in reaches only the daily
`bun run audit` in `audit.yml`, which reads the whole `bun.lock`.

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
3. The `loader` job downloads Ember's loader and its digest file from Ember's
   own release through gh. It runs no repository code, so the token gh needs
   never sits beside a build. It hands the files on as a run artifact and the
   loader's digest as a job output, which no other job in the run can write.
   `cargo xtask package --ember-release` then builds the mod's archive from
   that commit, holds the loader to that digest, to its digest file and to the
   release the lockfile resolves, and writes a `SHA256SUMS` beside it.
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
it. A release pull request merges only with every required check green, like
any other.

A user-facing change to a mod or to `crates/mods-common` releases every mod: a
`feat`, `fix`, `perf` or `revert` subject, or a breaking change of any type.
Every other type is hidden from the changelog and releases nothing. The
workspace shares one version, so the released crates are one `version_group`
and move together. mods-common takes a tag and no GitHub release, and a mod's
release notes carry mods-common's commits. The commit type sets the changelog
section and the bump size, and below 1.0.0 a `feat` bumps the patch and a
breaking change the minor. `0.x` promises no compatibility.

Every version heading in a changelog links GitHub's compare view from the
previous tag, which lists every change in the release, hidden types included.
`git log --oneline <mod>-v<old>..<mod>-v<new>` lists the same, and each GitHub
release carries GitHub's generated notes after its changelog.

No mod is released until Ember's release carries the loader, because the
release takes it from there. Run with no loader flag, `cargo xtask package`
downloads it itself through gh's own login or a `GH_TOKEN`. The command also
refuses a lockfile in which any Ember crate comes from somewhere other than
crates.io: a directory or a git checkout can carry a release's version number
without its code.

A failed release is recovered by cutting the next version, never by moving a
tag. Only the releaser app can create a tag.

## Troubleshooting

A local run that fails, or that differs from continuous integration:

- **A JavaScript tool is not installed in this checkout.** The gate refuses a
  row whose tool `node_modules/.bin` lacks. Run `bun install --frozen-lockfile`
  after every pull, and in a linked worktree add `--ignore-scripts`, as
  [Hooks](#hooks) says. The `commit-msg` hook does not check, and bunx then
  runs a copy from a parent directory or `PATH`, which can differ from the one
  CI runs.
- **A package `bun.lock` no longer names still loads.** A frozen install never
  removes a package the lockfile dropped, and `node_modules` stays the same
  across a branch switch. Delete `node_modules` and install again.
- **A result differs on your machine alone.** A personal env file at the
  checkout's root reaches Prettier and commitlint, and the Bun variables reach
  Bun as [Safety](#safety) says. Move the file aside, leave `BUN_OPTIONS` and
  the `BUN_INSPECT` names unset, and run again.
- **The gate stops before its first row after a manifest edit.** `cargo xtask`
  runs `--locked`, so relock with a plain `cargo build` first.
- **The gate refuses `Cargo.lock` after building against Ember's source.**
  Restore it with `git checkout Cargo.lock`, as
  [Building against Ember's source](#building-against-embers-source) says.
- **zizmor passes here and fails in CI, or the reverse.** Locally it runs online
  when `gh auth token` answers, and CI's gate runs it offline; CI's shared
  `workflows` job runs the online audits.
- **A tool is missing.** The gate names it. Run `cargo xtask setup`.
- **The gate stops on a checkout another account owns.** git refuses it as
  dubious ownership, and the gate's git reads no system or global config, so no
  `safe.directory` entry reaches it. Give the directory to the account that runs
  the gate. A `safe.directory` the gate would read is never the fix.

## What never happens

- Nobody hand-edits a version in a `Cargo.toml` or a crate's `CHANGELOG.md`.
  release-plz writes both from the commits, as [Releases](#releases) says, and
  a hand edit is overwritten or shifts the next version it computes.
- No commit message leaves the convention [Commit messages](#commit-messages)
  sets. release-plz computes each version and changelog entry from the
  messages, so a message outside it becomes a wrong entry in a release.
- No path inside a Steam library is ever written to, launched, or injected into.
  An installed copy of the game is not a server, and Steam overwrites its own
  files. A dedicated server for development is fetched separately into
  `.cache`. `.claude/settings.json` carries two `deny` entries that refuse an
  agent an edit under a Steam library, because a rule read is a rule that can
  be forgotten and a deny cannot.
- Nothing recovered from a Keen binary is committed: no schema dump, string
  table, protocol registry or other game data. The extractors are committed,
  and their output is derived data.
