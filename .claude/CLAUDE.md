# Claude Code in this repository

Everything a contributor needs is in the human-facing files. This one only
points at them.

Read [CONTRIBUTING.md](../CONTRIBUTING.md) and [docs/dev.md](../docs/dev.md)
before changing anything.

## The rules that do not bend

- **The gate is `cargo xtask check`.** Run it, and never run its steps
  separately as a substitute. A missing tool stops it and names itself.
- **Never write to, launch, or inject into a Steam library path.** A dedicated
  server for development is fetched separately into `.cache`.
- **Nothing recovered from a Keen binary is committed.** No schema dump, no
  string table, no protocol registry, no game data. The extractors are
  committed; their output lives under `.cache` and is regenerated.
- **Commit scopes come from `cargo xtask scopes`.** It prints
  `.github/commit-scopes.json`, the file the commit hook reads. Omit the scope
  rather than invent one.

## Which repository a change belongs in

Ask what the code describes:

- Keen's engine or Keen's game: it goes in
  [Ember](https://github.com/zachthedev/enshrouded-ember)
- The mod's own idea: it goes here

Something every mod here shares goes in `crates/mods-common`. One mod's own idea
goes in that mod.

## The documentation

| File                                  | Holds                                      |
| ------------------------------------- | ------------------------------------------ |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | The gate, the commit convention, the hooks |
| [docs/dev.md](../docs/dev.md)         | The first run, end to end                  |
| [docs/install.md](../docs/install.md) | Installing on a dedicated server           |
| [docs/admin.md](../docs/admin.md)     | The mod's settings and the chat verbs      |
| [README.md](../README.md)             | What the mods here do                      |
