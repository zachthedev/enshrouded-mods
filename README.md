# Enshrouded mods

Server-side mods for Enshrouded dedicated servers, built on
[Ember](https://github.com/zachthedev/enshrouded-ember).

Nothing here is affiliated with or endorsed by Keen Games.

## private-chests

Every magic chest inside a base feeds every player's crafting from one shared
pool. This mod gives each container an owner and a scope, so a crafting pull
draws only from containers the crafting player can reach.

- **Automatic.** A container is private to whoever placed it, or to that
  player's team. Nobody has to type anything.
- **Teams.** Leaderless: any member invites, the invitee accepts, leaving is
  free, active members cannot be removed, and removing an inactive member takes
  unanimous agreement from the rest.
- **Public when you want it.** One command puts a container back in the shared
  pool.
- **Several characters.** Items follow the character; teams and settings follow
  the account, so every character of one account shares them.
- **Server side only.** No client install, no launcher, no web panel. Players
  see heads-up notifications and a handful of chat verbs.

Crafting scope is the only behavior on by default. Restricting who can open a
container and who can quick-stack into one are separate settings, both off.

## Layout

| Path                  | Holds                                              |
| --------------------- | -------------------------------------------------- |
| `mods/private-chests` | The mod                                            |
| `crates/mods-common`  | Helpers the mods here share                        |
| `xtask`               | Development server, fixtures, packaging            |
| `fixtures`            | Worlds and configuration a test server starts from |
| `vendor`              | Ember, as a submodule, for local development       |

## Requirements

- Rust 1.98.1, pinned in `rust-toolchain.toml`
- A C toolchain, for the hook engine Ember uses
- `git clone --recurse-submodules`, or `git submodule update --init`

## License

MIT. See `LICENSE`.
