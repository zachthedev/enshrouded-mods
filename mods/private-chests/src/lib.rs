//! Container ownership and scope for an Enshrouded server.
//!
//! Every placed container has an owner, the character who placed it and the
//! account behind that character, and a scope: `Private`, `Team` or `Public`.
//! The owner sets the scope for `Private` and `Public`, any team member sets it
//! for `Team`, and an admin sets any of them.
//!
//! Each behavior is a separate setting, and this crate's own `docs/admin.md`
//! lists every one with its default. `craftingPool` filters what a crafting
//! pull draws from: workshop crafting, the build hammer, the Material Catalog
//! and factory pulls see only the acting player's own scope. `restrictOpen`
//! and `quickStack` extend the same ownership model to opening a container and
//! to quick-stack deposits.
//!
//! Identity splits the way the game already splits it. A character owns items,
//! so `Private` matches a character. An account owns settings, so team
//! membership and the placement default follow every character of that account.
//!
//! Teams are leaderless. Any member invites, the invitee accepts, leaving is
//! free, active members cannot be removed, and removing an inactive member or
//! renaming the team takes unanimous confirmation from the others.
//!
//! Server side only: no client install, no launcher, no web panel. Chat is the
//! only text channel a dedicated server can drive, so every message to a player
//! is a chat line. Dedicated servers ship with text chat off, and with it off
//! the mod enforces scope silently.

// `cargo xtask package` bundles the loader release matching the ember-sdk
// version in Cargo.lock, so the crate names the SDK before any of its code uses
// it, and cargo-machete reads this line as that use.
use ember_sdk as _;
