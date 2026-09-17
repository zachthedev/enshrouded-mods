//! Repository automation, run as `cargo xtask <command>`.
//!
//! Commands land here as the development loop arrives: fetching a dedicated
//! server build by revision, seeding a fixture world, launching that server with
//! the loader injected, tailing its log, and packaging a release.

use clap::{Parser, Subcommand};

/// Commit scopes: the workspace crate names, the fixtures directory, and the
/// three cross-cutting names no crate will ever own.
const SCOPES: &[&str] = &[
    "private-chests",
    "common",
    "xtask",
    "fixtures",
    "deps",
    "ci",
    "release",
];

#[derive(Parser)]
#[command(name = "xtask", about = "Repository automation for the mods workspace")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the commit scopes this repository accepts.
    Scopes,
}

fn main() {
    match Cli::parse().command {
        Command::Scopes => {
            for scope in SCOPES {
                println!("{scope}");
            }
        }
    }
}
