# Developing the mods

## Prerequisites

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

Install mise, then:

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

## First run

From a fresh clone to a green gate:

```sh
git clone --recurse-submodules https://github.com/zachthedev/enshrouded-mods.git
cd enshrouded-mods
mise install                  # the gate's tools, at the releases mise.lock records
bun install                   # the hooks and the markup formatter
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
[docs/dev.md](https://github.com/zachthedev/enshrouded-ember/blob/main/docs/dev.md)
says what each one holds. The build id names the directory. It is not what the
loader matches at run time: Ember identifies a build by the CodeView
fingerprint in the image itself, because the build id is not readable from the
running process.

**The extraction is required.** Nothing recovered from a Keen binary is
committed to this repository: no schema dump, no string table, no protocol
registry, no game data. The extractors are committed and every contributor runs
them against a server they fetched themselves. Anything the extractor produces
is derived data, lives under `.cache`, and is regenerated rather than shared.
The loop is fetch, extract, check.

`cargo xtask check --rows` prints the gate's rows and what each covers.
[CONTRIBUTING.md#the-gate](../CONTRIBUTING.md#the-gate) says what the gate does
when a tool is missing. `pre-push` runs the same command, and so does
continuous integration.

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

[fixtures/README.md](../fixtures/README.md) says what a fixture directory
holds.

Arguments pass through untouched, so Ember's own subcommands and flags are the
only ones. Ember's
[docs/dev.md](https://github.com/zachthedev/enshrouded-ember/blob/main/docs/dev.md)
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

**Never point any of this at a Steam library.** A path under `steamapps/common`
is refused. Your installed copy of the game is not a server, Steam overwrites
its own files, and nothing here ever writes to, launches, or injects into one.

## Generated files

| File               | Regenerated by                                              |
| ------------------ | ----------------------------------------------------------- |
| `Cargo.lock`       | any cargo build after a manifest edit; the gate runs locked |
| `bun.lock`         | `bun install` after a `package.json` edit                   |
| `mise.lock`        | `mise lock` after a `mise.toml` edit                        |
| `mise.semver.lock` | `MISE_ENV=semver mise lock` after a `mise.semver.toml` edit |

`mise.lock` keeps the hand-computed taplo hashes across a relock at the same
version, as Prerequisites says.

## Tests that need a real thing

None.

## Building against Ember's source

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
