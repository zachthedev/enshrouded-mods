# Contributing

The gate opens by holding `mise.toml` and `mise.lock` to their rules, before it
runs any tool. `cargo xtask pins` runs that on its own. A lockfile entry with a
url and no checksum installs whatever the url serves, so a rule that ran later
would report a finding about a binary that had already executed. The steps after
it:

| Step | What it checks

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

It runs, in order and stopping at the first failure:

| Step         | What it checks                                           |
| ------------ | -------------------------------------------------------- |
| `fmt`        | Rust formatting                                          |
| `taplo`      | TOML formatting                                          |
| `clippy`     | Lints on every target, with warnings denied              |
| `tests`      | The workspace's tests, through `cargo nextest`           |
| `doctests`   | Every documented example                                 |
| `deny`       | Advisories, licenses, bans and sources                   |
| `machete`    | Dependencies a crate declares and never uses             |
| `audit`      | The lockfile against the RustSec advisory database       |
| `prettier`   | Markup, JavaScript and TypeScript formatting             |
| `shellcheck` | The git hooks, and the release actionlint's analyzer is  |
| `actionlint` | Workflow syntax, runner labels and expressions           |
| `zizmor`     | Workflow pinning, credentials, permissions and injection |

`taplo` reads `.taplo.toml` for the files it covers, and leaves Ember's checkout
under `vendor/` alone.

`shellcheck` reads the hooks in `.githooks`, and its step is also where the gate
holds the installed ShellCheck to the version `mise.toml` pins. That step runs
before `actionlint` and the gate stops at the first step that does not pass, so
`actionlint` is never reached on a host whose ShellCheck the pin does not cover.

`actionlint` checks workflow syntax, runner labels and every expression,
including whether a `needs.<job>.outputs.<name>` names an output that job
declares. It runs an external analyzer when it finds one on `PATH` and says
nothing at all when it does not, so an absent analyzer is a pass for a pass
nobody ran. pyflakes is therefore off, because no Windows package manager ships
it and off is the only setting both matrix legs agree on. shellcheck is on, and
the step before it holds the release, so both legs run the same analysis. What
that analysis reads is the shell in a `run:` block actionlint resolves to sh or
bash. A block declaring `shell: pwsh`, and every block in the `gate` job, is not
shell it can read, so the hooks the `shellcheck` step names are the bulk of what
is covered here.

`zizmor` audits the same files for supply chain and credential problems: an
action not pinned to a commit, a checkout that leaves the job token behind, a
workflow with no `permissions` block, and expression injection through untrusted
context. It is given `.github/workflows` and `.github/dependabot.yml` by name,
so it never reaches the workflows in Ember's checkout under `vendor/`.
`--strict-collection` makes a file it cannot parse fail the step rather than
drop out of the audit. `--offline` keeps it from needing a GitHub token, so a
runner and a laptop get the same findings. `--config` names
`.github/zizmor.yml`, which holds the Dependabot cooldown threshold, so the
environment cannot swap it for another. A test allowlists every inline zizmor
ignore comment.

`doctests` runs whether or not `cargo-nextest` is installed, because
`cargo nextest` runs none of them and a doctest that stops compiling would
otherwise pass the gate in silence.

`tests` falls back to `cargo test --workspace` when `cargo-nextest` is absent,
and the summary says which runner ran. Any other missing tool stops the gate and
names itself, because a check that did not run is not a check that passed.

The pre-push hook and continuous integration call the same command, so neither
can run a different gate.

Continuous integration runs the same gate on Windows and on Linux, because the
pre-push hook runs it on whichever host a contributor uses.

## Hooks

`.githooks` holds the hooks: `commit-msg` runs commitlint, and `pre-push` runs
the gate. Install them once per clone:

```sh
bun install
```

Without Bun:

```sh
cargo xtask hooks install
```

Either one points `core.hooksPath` at `.githooks`.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), enforced by the
`commit-msg` hook. `.github/commit-scopes.json` holds the scope list.
`cargo xtask scopes` prints it, and `commitlint.config.js` enforces it:

`private-chests`, `common`, `xtask`, `fixtures`, `deps`, `ci`, `release`.

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
- No schema dump, string table, protocol registry or other recovered game data
  is committed. The extractors are committed; their output is not.
