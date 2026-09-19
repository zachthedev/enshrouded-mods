# Security

Ember loads as a proxy library beside the dedicated server executable and hooks
functions inside the running process. The mods here run in that process. That is
the design, it is written down in [docs/install.md](docs/install.md), and it is
not what this file is about.

## Reporting

Open a private advisory:
<https://github.com/zachthedev/enshrouded-mods/security/advisories/new>

Never open a public issue for a vulnerability. Everything else belongs in the
issue tracker.

A report is most useful with the Steam build id, the Ember and mod versions, and
the shortest steps that show it. Name what a player gains that the scope on a
container does not allow, and attach the Ember log from `ember/logs` when a mod
was loaded. Keep a proof of concept inert. Something that writes to standard
output proves the hole is reachable as well as something that acts on it.

## What is supported

No release is published yet, so `main` is the only supported version. A fix
lands on the newest release once one exists. A release supports the dedicated
server build current at the time plus the four before it.

## In scope

- A player reaching a container, a station or a team the scope on it does not
  cover.
- A chat line that crashes or hangs the server, or that reaches a verb only an
  admin is meant to reach.
- A crafted `config.json` or save record that does more than fail to parse.
- A published release whose library does not match the source it names, or the
  archive it is served from.
- A workflow in this repository that hands write access or a credential to a
  pull request.

## Out of scope

- A defect in Enshrouded or in Keen's dedicated server that a mod here does not
  introduce. Report those to Keen Games.
- Anything that needs admin rights the server already granted. An admin holds
  every scope on purpose.
- A server the host left open: a shared password, an exposed port, or an `Admin`
  group with everyone in it.
- Wine and Proton themselves. A sandbox escape there belongs upstream.
- Chat spam a connected player can already aim at a stock server.
- The proxy library and the hooking, which is the first paragraph.

## After a report

One person maintains this project, and a first reply takes up to a week. There
is no bounty. A report gets an acknowledgment, a fix, and a credit in the
advisory unless you ask to stay anonymous.
