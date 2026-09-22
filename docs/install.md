# Installing on a dedicated server

For server admins. The mod is server side. Players install nothing and join with
a stock client.

## Requirements

- An Enshrouded dedicated server you run yourself, on Windows or under Wine or
  Proton
- `private-chests-v<version>.zip`, from this repository's
  [releases](https://github.com/zachthedev/enshrouded-mods/releases)

The archive carries Ember, the loader, along with the mod, so a first install
needs nothing else. Upgrading Ember on its own is a separate download, described
below.

## Install

Ember is a proxy library that sits beside the server executable. The mod is a
separate library under `ember/mods/`.

```text
enshrouded_server.exe
enshrouded_server.json
POWRPROF.dll                          Ember, the proxy library
ember/
  config.json                         Ember's own settings
  logs/                               Written at run time
  mods/
    private-chests/
      private_chests.dll              The mod
      config.json                     The mod's settings
```

The archive holds the two libraries and nothing else. Each library writes its
own `config.json` on first start.

`POWRPROF.dll` is the default proxy name, because no other public Enshrouded
loader claims it. The same library renamed to `IPHLPAPI.dll` or `dbghelp.dll`
forwards those instead, for a server that already has something in the first
slot.

One directory to drop in, and mods compose without further steps. The Windows
and the Wine sections below say how to start the server once the files are in
place.

## Check the download

Every release attaches a `SHA256SUMS` file beside the archive, written by the
same command that built the archive. Check what you downloaded against it
before extracting:

```powershell
Get-FileHash private-chests-v<version>.zip -Algorithm SHA256
```

The build that produced the archive is attested by GitHub, so the archive can
also be held to the workflow run that made it:

```sh
gh attestation verify private-chests-v<version>.zip --repo zachthedev/enshrouded-mods
```

## Upgrade

Extract a newer archive over the server you already run. No `config.json` is in
the archive, so your settings stay where they are. Ember refuses to load a mod
built against an ABI it does not match, and names that mod in the startup
report; take that mod's newest release when you see it named.

## Uninstall

Delete `POWRPROF.dll` and the `ember/` directory. The mod keeps its settings
under `ember/mods/private-chests/` and nothing outside it, so removal leaves
the server as it was.

## Windows

Extract the archive into the server directory as laid out above and start the
server the way you already do. Ember writes a startup report to `ember/logs`,
naming every mod it loaded and every symbol it failed to resolve.

## Wine and Proton

Keen ships a Windows dedicated server only. A Linux host runs it under Wine or
Proton, which is a supported and tested path.

Wine and Proton ship their own builtin copy of every library Ember can stand in
for, and prefer the builtin over the one in the server directory. Tell Wine to
prefer the native file:

```sh
WINEDLLOVERRIDES="powrprof=n,b" wine enshrouded_server.exe
```

The name on the left is the proxy you installed, without the extension. A server
running the `IPHLPAPI.dll` build uses `iphlpapi=n,b` instead.

Without the override the server starts and no mod loads, which looks exactly
like a vanilla server.

## Upgrading Ember on its own

Ember releases on its own schedule, in
[enshrouded-ember](https://github.com/zachthedev/enshrouded-ember). Each release
there attaches `POWRPROF.dll` with a `SHA256SUMS` of its own. Replace the file
beside the server executable and leave `ember/` alone.

It is the same library a mod's archive carries. Ember builds the loader once and
a mod's release downloads that asset rather than building its own, so the file
you install either way is the same one.

## Turning on chat

Every message the mod sends a player is a chat line. Dedicated servers ship with
text chat off, so set it on in `enshrouded_server.json`:

```json
{
  "enableTextChat": true
}
```

With text chat off, scope is still enforced and the chat verbs do nothing. The
mod logs an error naming the key and keeps running.

The server writes `enshrouded_server.json` itself and its parser is strict. Edit
it with the server stopped. No mod ever writes it.

## A note on your game install

Never install Ember or a mod into a Steam library copy of the game. A dedicated
server is a separate download. Steam verifies and overwrites its own files, and
the client install is not a server.

## Upgrading the game

Ember matches a server build by a fingerprint read from the executable. A build
it does not recognize stops with a message rather than hooking the wrong
function.

Which builds a release covers is Ember's to decide, and its README's
[Surviving game updates](https://github.com/zachthedev/enshrouded-ember#surviving-game-updates)
says which. A build that is not covered is reported there. Open a "New Keen
build" issue in
[Ember](https://github.com/zachthedev/enshrouded-ember/issues/new/choose). Its
form asks for the identifiers the fix needs.
