# Running private-chests

For server admins. [docs/install.md](install.md) covers getting the files in
place.

## Scopes

Every placed container and every placed crafting station carries one scope.

| Scope     | Who reaches it                          |
| --------- | --------------------------------------- |
| `Private` | The character who placed it, and admins |
| `Team`    | Every member of the placer's team       |
| `Public`  | Everyone on the server                  |

A station draws only from containers in its own scope. A Public station spends
communal stock, a Private station spends its owner's, a Team station spends the
team's, and none reaches into another pool.

A new container or station takes the placer's default, which is their team when
they have one and Private otherwise. The owner sets the scope for Private and
Public, any team member sets it for Team, and admins set any of them.

A container that predates the mod goes Private to its placer when the save
records one, and Public otherwise. A container from an imported base has an
owner who is not on this server, so it is treated as having no record.

A container picked up and placed one cell over is a new container. It loses its
record, takes the placement default again, and the mod says so in chat.

## Settings

The mod reads `ember/mods/private-chests/config.json`. A missing file is written
from the defaults with every field present. A file that fails to parse is never
overwritten. An unknown key is an error, and the mod disables itself and says
which key it was.

### Behaviors

Each behavior is a separate setting, and only the crafting pool is on.

| Key            | Default | What it does                                                                                          |
| -------------- | ------- | ----------------------------------------------------------------------------------------------------- |
| `craftingPool` | `true`  | Workshop crafting, the build hammer, the Material Catalog and factory pulls see only their own scope. |
| `restrictOpen` | `false` | A player outside a container's scope opens a take-only window rather than the container.              |
| `quickStack`   | `false` | The Quick Stack Station deposits only into containers in the depositor's scope.                       |

### Ownership

| Key            | Default               | What it does                                                                                                  |
| -------------- | --------------------- | ------------------------------------------------------------------------------------------------------------- |
| `unowned`      | `allow`               | What happens at a container with no record. `allow` leaves an existing world unchanged.                       |
| `privateScope` | account and character | A Private container matches both. Set it to `Account` to let every character on the owner's account reach it. |

Identity splits the way the game splits it. Items attach to the character.
Settings, teams and the placement default attach to the account.

### Teams

| Key                   | Default | What it does                                                                         |
| --------------------- | ------- | ------------------------------------------------------------------------------------ |
| `teams.maxSize`       | `5`     | How many accounts one team holds.                                                    |
| `teams.allowMultiple` | `false` | Whether an account holds several teams, which turns on the team-name argument forms. |
| `inactiveDays`        | `30`    | How long a member is away before the others can vote to remove them.                 |
| `voteExpiryDays`      | `7`     | How long a removal vote stays open. It must not exceed `inactiveDays`.               |

### Admins

| Key                 | Default     | What it does                                                   |
| ------------------- | ----------- | -------------------------------------------------------------- |
| `admins.userGroups` | `["Admin"]` | User groups from `enshrouded_server.json` that count as admin. |
| `admins.accounts`   | `[]`        | Accounts that count as admin whatever their group.             |

`Admin` is the group Keen ships with kick and ban rights. Renaming that group in
`enshrouded_server.json` means renaming it here too.

## Teams, in full

Teams follow the Hypixel SkyBlock co-op model. There is no leader.

- Any member invites. The invitee accepts.
- Leaving is free and takes nobody's agreement.
- An active member cannot be removed.
- Removing a member who has been away longer than `inactiveDays` takes unanimous
  confirmation from every other active member. Renaming the team takes the same.
- A confirmation survives disconnects, because the target is away by definition
  and the vote spans days.
- The target coming online voids the vote. It can reopen once they are away for
  the full `inactiveDays` again.
- A member who leaves during a vote drops out of it, and the vote is judged
  again at once.
- An empty team dissolves. Its containers go Private to whoever placed them.

## Chat verbs

Every verb is typed in chat. `enableTextChat` has to be true, or none of them
reach the server.

| Verb      | What it does                                                      |
| --------- | ----------------------------------------------------------------- |
| `/chest`  | Acts on the container you opened last. Open it, then type.        |
| `/chests` | Summarizes the containers in range and their scopes.              |
| `/team`   | Invites, accepts, leaves, votes, and names the placement default. |
| `/help`   | Lists these.                                                      |

Admins use the same verbs, reaching containers they do not own.

The wording is the game's own. No mod name, no version, no bracketed prefix
reaches a player.

## What to expect

The client builds its crafting display from its own copy of the base's
containers, so a recipe can look craftable and then be refused. The refusal is
the server's answer, and the scope is what decided it.

The server writes its own crash dumps. A dump taken with Ember loaded has a
wrong or truncated stack through any hooked frame, and Ember's startup report
says so.
