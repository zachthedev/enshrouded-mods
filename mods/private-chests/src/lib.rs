//! Container ownership and scope for an Enshrouded server.
//!
//! Every placed container has an owner, the character who placed it and the
//! account behind that character, and a scope: private, team, or public. The
//! owner class configures the scope: the owner for private and public, any team
//! member for team, administrators always.
//!
//! Features are separate settings and most are off. `craftingPool`, the only one
//! on by default, filters what a crafting pull draws from: workshop crafting,
//! the build hammer, the material catalog and factory pulls see only the acting
//! player's own scope. `restrictOpen` and `quickStack` extend the same ownership
//! model to opening a container and to quick-stack deposits.
//!
//! Identity splits the way the game already splits it. A character owns items,
//! so private scope matches a character. An account owns settings, so team
//! membership and the placement default follow every character of that account.
//!
//! Teams are leaderless. Any member invites, the invitee accepts, leaving is
//! free, active members cannot be removed, and removing an inactive member or
//! renaming the team takes unanimous confirmation from the others.
//!
//! Server side only: no client install, no web panel. Chat is the only text
//! channel a dedicated server can drive, so every message to a player is a chat
//! line. Dedicated servers ship with text chat off, and with it off the mod
//! enforces scope silently.
