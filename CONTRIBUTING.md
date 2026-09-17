# Contributing

## Getting the source

Ember lives in its own repository and is vendored here as a submodule:

```sh
git clone --recurse-submodules https://github.com/zachthedev/enshrouded-mods.git
```

An existing clone catches up with `git submodule update --init`.

## Toolchain

`rust-toolchain.toml` pins Rust 1.98.1. A C toolchain is needed for the hook
engine Ember uses.

## The gate

Every one of these must pass before a commit is pushed:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Commit messages

Conventional Commits, enforced by a `commit-msg` hook running commitlint.
Install the hook once per clone with `bun install`.

## Scopes

`private-chests`, `common`, `xtask`, `fixtures`, `deps`, `ci`, `release`.

A new mod earns a scope. Omit the scope rather than invent one. Run
`cargo xtask scopes` for the live list.

## Where code goes

Ask what the code describes:

- Keen's engine or the Enshrouded server: Ember, not here
- Something every mod here shares: `crates/mods-common`
- One mod's own idea: that mod

## Testing against a server

`cargo xtask` fetches a dedicated server into `.local/`, seeds a fixture world
and launches it with the mod loaded. It never touches an installed copy of the
game, and any command given a path inside a Steam library refuses it.
