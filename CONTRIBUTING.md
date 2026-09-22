# Contributing

The gate opens by holding `mise.toml` and `mise.lock` to their rules, before it
runs any tool. `cargo xtask pins` runs that on its own. A lockfile entry the
rules reject installs whatever its url serves, so a rule that ran later would
report a finding about a binary that had already executed.
[The gate](#the-gate) names every step in order.

## Getting the source

Ember lives in its own repository and is vendored here as a submodule:

```sh
git clone --recurse-submodules https://github.com/zachthedev/enshrouded-mods.git
```

An existing clone catches up with `git submodule update --init`.

## Toolchain

`rust-toolchain.toml` pins the Rust release, and rustup installs it on the first
cargo command. A C toolchain is needed for MinHook, the hook engine Ember uses.
[Bun](https://bun.sh) runs the repository's own tooling, at the release
`.bun-version` pins. Continuous integration reads the same file.

The gate calls tools that rustup, cargo and bun do not provide.
[mise](https://mise.jdx.dev) installs every one of them. `mise.toml` pins a
version per tool and `mise.lock` records a checksum per platform, so an install
takes the recorded artifact or fails. Install mise, then:

```sh
mise install
```

Nothing from that lands on `PATH`. The gate asks `mise which` for each binary
and runs the path it gives back, so the binary it checked is the binary it ran.
Turning on `mise activate` in a shell puts the same binaries on `PATH` under
their own names, which is what makes `cargo nextest run` and its siblings work
at a prompt. The split is deliberate: a check runs the binary it resolved, and a
person gets the convenience.

`mise.lock` is generated, so the gate holds it to something that is not in it.
`xtask/src/pins.rs` carries the owner and the repository every tool's artifacts
come from, and the rules refuse a lockfile entry whose `url` or `backend` names
anything else. A bare registry key in `mise.toml` names no owner, so for those
tools that table is the only record of the account outside the generated file.
Moving a tool to another account takes an edit there, in the same diff as the
lockfile it explains.

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
says which. `--config` names
`.github/zizmor.yml`, which holds the Dependabot cooldown threshold, so the
environment cannot swap it for another.

`doctests` runs beside `tests`, because `cargo nextest` runs none of them and a
doctest that stops compiling would otherwise pass the gate in silence. `doc`
builds every crate's documentation with warnings denied, so a broken link is a
failure.

A missing tool stops the gate and names itself, because a check that did not
run is not a check that passed. Advisories are not a row: Dependabot alerts read
RustSec for every pushed lockfile.

The pre-push hook and continuous integration call the same command, so neither
can run a different gate.

Continuous integration runs the same gate on Windows and on Linux, because the
pre-push hook runs it on whichever host a contributor uses.

## Hooks

`lefthook.yml` holds the hooks: `commit-msg` runs commitlint, and `pre-push`
runs the gate. [lefthook](https://lefthook.dev) installs them into `.git/hooks`
when `bun install` runs the `prepare` script, once per clone:

```sh
bun install
```

A clone that ran an earlier `prepare` still points `core.hooksPath` at a
directory that no longer exists, so run `git config --unset core.hooksPath`
once before installing.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), enforced by the
`commit-msg` hook. `.github/commit-scopes.json` holds the scope list, one
sentence per scope saying what it covers. `cargo xtask scopes` prints it, and
`commitlint.config.js` enforces it.

A new mod earns a scope. Omit the scope rather than invent one.

## Where code goes

Ask what the code describes:

- Keen's engine or the Enshrouded server:
  [Ember](https://github.com/zachthedev/enshrouded-ember), not here
- Something every mod here shares: `crates/mods-common`
- One mod's own idea: that mod

## The first run

[docs/dev.md](docs/dev.md) has it end to end. In short: fetch a dedicated server
into `.cache`, extract the schema from it, then run the gate. The extract step
is required, because nothing recovered from a Keen binary is committed to this
repository.

`cargo xtask server ...` and `cargo xtask schema ...` forward to Ember's xtask
through the submodule, with `--root` set to this repository, so everything they
write stays under `.cache`.

## What never happens

- No path inside a Steam library is ever written to, launched, or injected into.
  A dedicated server for development is fetched separately into `.cache`.
  `.claude/settings.json` carries two `deny` entries that refuse an agent an
  edit under a Steam library, because a rule read is a rule that can be
  forgotten and a deny cannot.
- No schema dump, string table, protocol registry or other recovered game data
  is committed. The extractors are committed; their output is not.
