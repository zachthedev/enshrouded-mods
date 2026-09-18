# Contributing

## Getting the source

Ember lives in its own repository and is vendored here as a submodule:

```sh
git clone --recurse-submodules https://github.com/zachthedev/enshrouded-mods.git
```

An existing clone catches up with `git submodule update --init`.

## Toolchain

`rust-toolchain.toml` pins Rust 1.98.1. A C toolchain is needed for the hook
engine Ember uses. [Bun](https://bun.sh) runs the repository's own tooling.

The gate calls four cargo subcommands that rustup does not install.
`.github/cargo-tools` pins their versions and is the only place those numbers
live, so this installs what continuous integration installs:

```powershell
cargo install --locked @(Get-Content .github/cargo-tools | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() })
```

On a shell without PowerShell:

```sh
cargo install --locked $(grep -v '^#' .github/cargo-tools | grep .)
```

## The gate

One command, and the only one:

```sh
cargo xtask check
```

It runs, in order and stopping at the first failure:

| Step       | Command                                                              |
| ---------- | -------------------------------------------------------------------- |
| `fmt`      | `cargo fmt --check`                                                  |
| `clippy`   | `cargo clippy --workspace --all-targets -- -D warnings`              |
| `tests`    | `cargo nextest run --workspace`                                      |
| `deny`     | `cargo deny check`                                                   |
| `machete`  | `cargo machete`                                                      |
| `audit`    | `cargo audit`                                                        |
| `prettier` | `bunx --no-install --bun prettier --check` over markdown, YAML, JSON |

`tests` falls back to `cargo test --workspace` when `cargo-nextest` is absent,
and the summary says which runner ran. Any other missing tool stops the gate and
names itself, because a check that did not run is not a check that passed.

The pre-push hook and continuous integration call the same command, so the three
cannot drift apart.

## Hooks

Two hooks live in `.githooks`: `commit-msg` runs commitlint, and `pre-push` runs
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
`commit-msg` hook. Run `cargo xtask scopes` for the live scope list, which is
the same list `commitlint.config.js` enforces:

`private-chests`, `common`, `xtask`, `fixtures`, `deps`, `ci`, `release`.

A new mod earns a scope. Omit the scope rather than invent one.

## Where code goes

Ask what the code describes:

- Keen's engine or the Enshrouded server: Ember, not here
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
- No schema dump, string table or other recovered game data is committed. The
  extractors are committed; their output is not.
