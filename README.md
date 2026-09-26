# Enshrouded mods

Server-side mods for Enshrouded dedicated servers, built on
[Ember](https://github.com/zachthedev/enshrouded-ember).

Nothing here is affiliated with or endorsed by Keen Games.

This repository is in development. Each release is published on the
[releases page](https://github.com/zachthedev/enshrouded-mods/releases).

Each mod ships as one archive on its GitHub release, and
[docs/install.md](docs/install.md) puts it on a server. `cargo xtask check` is
the gate a change passes, as [CONTRIBUTING.md](CONTRIBUTING.md#the-gate) says.

## private-chests

The first mod. [docs/usage.md](docs/usage.md#private-chests) says what it
changes.

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
channel a dedicated server can drive.
[Turning on chat](docs/install.md#turning-on-chat) says what the server needs
for any of it to reach a player.

## Documentation

| File                                                             | For                                                                 |
| ---------------------------------------------------------------- | ------------------------------------------------------------------- |
| [docs/install.md](docs/install.md)                               | Installing a mod on a server, checking the download, upgrading      |
| [docs/usage.md](docs/usage.md)                                   | What each mod does, and where its own documents are                 |
| [Running private-chests](mods/private-chests/docs/admin.md)      | The mod's settings and the chat verbs                               |
| [Draw and deposit paths](mods/private-chests/docs/draw-paths.md) | How the game itself moves items into and out of containers          |
| [fixtures/README.md](fixtures/README.md)                         | The worlds and configurations the dev server is seeded with         |
| [CONTRIBUTING.md](CONTRIBUTING.md)                               | Setup, the dev server, the gate, commits, releases, troubleshooting |
| [SECURITY.md](SECURITY.md)                                       | How to report a vulnerability and what is in scope                  |
| [AGENTS.md](AGENTS.md)                                           | What an agent reads first, runs to verify, and never does           |

## Layout

| Path                  | Holds                                                  |
| --------------------- | ------------------------------------------------------ |
| `mods/private-chests` | The mod                                                |
| `crates/mods-common`  | Helpers the mods here share                            |
| `xtask`               | The gate, the release bundle, the forwards to Ember    |
| `fixtures`            | Worlds and configuration for `cargo xtask server seed` |
| `vendor`              | Ember, as a submodule, for local development           |

## License

MIT. See `LICENSE`.

Manifests carry the brand, `ZachTheDev`. The legal name belongs to the copyright
line in `LICENSE` and nowhere else.
