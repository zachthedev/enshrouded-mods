# Installing on a dedicated server

For server admins. The mod is server side. Players install nothing and join with
a stock client.

This repository is in development and publishes no release yet. What follows is
the install shape the mod is built for.

## What you need

- An Enshrouded dedicated server you run yourself
- Ember, the loader, from
  [enshrouded-ember](https://github.com/zachthedev/enshrouded-ember)
- The `private-chests` library

## Where the files go

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

`POWRPROF.dll` is the default proxy name, because no other public Enshrouded
loader claims it. The same library renamed to `IPHLPAPI.dll` or `dbghelp.dll`
forwards those instead, for a server that already has something in the first
slot.

One directory to drop in, and mods compose without further steps.

## Windows

Copy the files into the server directory as laid out above and start the server
the way you already do. Ember writes a startup report to `ember/logs`, naming
every mod it loaded and every symbol it failed to resolve.

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
function. A release supports the current server build plus the four before it.

When Keen ships a build that is not covered, open a "New Keen build" issue. The
template asks for the identifiers the fix needs.
