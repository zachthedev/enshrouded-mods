# Enshrouded mods

Server-side mods for Enshrouded dedicated servers, built on
[Ember](https://github.com/zachthedev/enshrouded-ember).

Nothing here is affiliated with or endorsed by Keen Games.

This repository is in development. No release is published yet.

## private-chests

Every magic chest inside a base feeds every player's crafting from one shared
pool. This mod gives each container and each crafting station an owner and a
scope, so a craft draws only from containers the crafter can reach.

### What a player sees

- **Automatic.** A container or station is private to whoever placed it, or to
  that player's team. Nobody has to type anything.
- **Teams.** Leaderless: any member invites, the invitee accepts, leaving is
  free, active members cannot be removed, and removing an inactive member takes
  unanimous agreement from the rest.
- **Public when you want it.** One command puts a container back in the shared
  pool.
- **Several characters.** Items follow the character; teams and settings follow
  the account, so every character of one account shares them.
- **Server side only.** No client install, no launcher, no web panel.

### What an admin gets

- Private, Team and Public scopes. A station spends only from its own scope, so
  a Public station spends communal stock and a Private one spends its owner's.
- Each behavior is its own setting, and
  [the mod's own documentation](mods/private-chests/docs/admin.md#settings)
  lists every one with its default.
- Admins are a user group from `enshrouded_server.json`, plus an account list.
  The mod never writes that file.

Everything a player sees arrives as a chat line, because chat is the only text
channel a dedicated server can drive. Dedicated servers ship with text chat off,
so `enableTextChat` has to be true in `enshrouded_server.json` for any of it to
reach a player. With it off the mod still enforces scope; it just says nothing.

## Documentation

| File                               | For                                        |
| ---------------------------------- | ------------------------------------------ |
| [docs/install.md](docs/install.md) | Server admins installing the mod           |
| [docs/mods.md](docs/mods.md)       | What each mod does, and where its docs are |
| [docs/dev.md](docs/dev.md)         | The first run, end to end                  |
| [CONTRIBUTING.md](CONTRIBUTING.md) | The gate, the commit convention, the hooks |

## Layout

| Path                  | Holds                                                  |
| --------------------- | ------------------------------------------------------ |
| `mods/private-chests` | The mod                                                |
| `crates/mods-common`  | Helpers the mods here share                            |
| `xtask`               | The gate, the hooks, the server and schema commands    |
| `fixtures`            | Worlds and configuration for `cargo xtask server seed` |
| `vendor`              | Ember, as a submodule, for local development           |

## Requirements

- Rust, at the release `rust-toolchain.toml` pins
- A C toolchain, for MinHook, the hook engine Ember uses. On Windows, Visual
  Studio Build Tools.
- [Bun](https://bun.sh), for the repository's own tooling, at the release
  `.bun-version` pins
- `git clone --recurse-submodules`, or `git submodule update --init`

## License

MIT. See `LICENSE`.
