# Contributing

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

The gate calls tools that rustup does not install. `.github/cargo-tools` pins
the crates.io packages among them, and `.github/go-tools` pins the Go programs,
which need [Go](https://go.dev) to install. The versions live in those files and
nowhere else, so this installs what continuous integration installs:

```powershell
cargo install --locked @(Get-Content .github/cargo-tools | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() })
Get-Content .github/go-tools | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() } | ForEach-Object { go install $_ }
```

On a shell without PowerShell:

```sh
cargo install --locked $(grep -v '^#' .github/cargo-tools | grep .)
grep -v '^#' .github/go-tools | grep . | xargs -n 1 go install
```

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
| `actionlint` | Workflow syntax, runner labels and expressions           |
| `zizmor`     | Workflow pinning, credentials, permissions and injection |

`taplo` reads `.taplo.toml` for the files it covers, and leaves Ember's checkout
under `vendor/` alone.

`actionlint` checks workflow syntax, runner labels and every expression,
including whether a `needs.<job>.outputs.<name>` names an output that job
declares. It is a Go program and is installed from `.github/go-tools`. Its
external analyzers, shellcheck and pyflakes, are off: actionlint runs them when
it finds them on `PATH` and says nothing when it does not, and `ubuntu-latest`
carries shellcheck while `windows-latest` does not, so leaving them on would
have the matrix legs check different things and the quiet leg report a pass for
an analysis it never ran.

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
