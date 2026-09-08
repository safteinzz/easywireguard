//! ewg - wireguard config generation and management without the hand-editing.
//!
//! This file is the clap `Cmd` enum and the dispatch match; what you can run is
//! `ewg --help`, which renders from the manifest, those doc comments and
//! `AFTER`, and is the only copy of that list.

mod commands;
mod elevate;
mod keys;
mod manifest;
mod registry;
mod selfcmd;
mod tui;
mod wg;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::dir::DirArgs;
use commands::mesh::MeshArgs;
use std::io::IsTerminal;
use std::path::PathBuf;

/// clap's own layout with one change: `{before-help}` moves from above the
/// description to just under `Usage:`, so the shapes block lands on top of the
/// command list rather than on top of the screen.
const TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}\n\n{before-help}{all-args}{after-help}\n";

/// The one shape clap cannot list, because the TUI is the bare invocation.
const WAYS: &str = "\x1b[1mWays to run it (not subcommands):\x1b[0m
  ewg    the interface manager TUI, across every registered dir";

/// The rest of the block: what a script can expect, then where to look next.
const AFTER: &str = concat!(
    "\
Output is written for people, and `dir` and `mesh` take `--json` when something
has to read it; `check` is the machine-facing one and exits non-zero when a
config is broken. Failures name themselves on stderr and exit non-zero.
Run `ewg <command> --help` for a command's details.",
    "\n\n",
    env!("CARGO_PKG_REPOSITORY"),
    "\ncontributors: ",
    env!("CARGO_PKG_AUTHORS"),
);

/// `-V` stays a bare version string for scripts; `--version` spells out the
/// license, where it lives, and who's contributed. Every field comes from
/// Cargo.toml, so none of it can drift from the manifest.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n",
    env!("CARGO_PKG_LICENSE"),
    "  ",
    env!("CARGO_PKG_REPOSITORY"),
    "\ncontributors: ",
    env!("CARGO_PKG_AUTHORS"),
);

#[derive(Parser)]
#[command(
    name = "easywireguard",
    bin_name = "ewg",
    version,
    long_version = LONG_VERSION,
    about,
    // The shapes come first: this is a bare-first binary, so the command list is
    // the leftovers and putting it on top answers the wrong question first.
    help_template = TEMPLATE,
    before_help = WAYS,
    after_help = AFTER
)]
struct Cli {
    /// Use only this dir for this run, overriding the registry
    #[arg(long, global = true, env = "EWG_DIR")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // clap arg enums: boxing fights the derive
enum Cmd {
    /// Interface manager TUI. Hidden because the bare invocation is the
    /// documented way in, and two entries pointing at each other is repetition.
    #[command(hide = true)]
    Tui,

    /// List every interface you can up/down across your dirs, with up/down state
    #[command(visible_alias = "ls")]
    List,

    /// Show only the interfaces currently up (real wireguard status)
    Status,

    /// Bring an interface up  <NAME>
    #[command(verbatim_doc_comment)]
    Up {
        /// The interface to bring up, named by its `.conf` without the extension
        name: String,
    },

    /// Bring an interface down  <NAME>
    #[command(verbatim_doc_comment)]
    Down {
        /// The interface to bring down, named by its `.conf` without the extension
        name: String,
    },

    /// Register where your .conf files live, so every other command spans them
    ///   dir                         list them (-v counts configs, --json for a script)
    ///   dir add <PATH>              register a directory
    ///   dir rm <PATH>               forget one
    #[command(verbatim_doc_comment)]
    Dir(DirArgs),

    /// Design a mesh: nodes in a manifest, then each node's config
    ///   mesh                        list the nodes (-m picks the manifest)
    ///   mesh add <NAME>             add a node
    ///     --address <ADDRESS>       its address in the mesh, required
    ///     --pubkey <PUBKEY>         its public key, required
    ///   mesh rm <NAME>              remove one
    ///   mesh gen                    write each config, as "all peers minus itself"
    #[command(verbatim_doc_comment)]
    Mesh(MeshArgs),

    /// Generate a new WireGuard keypair (private + public)
    Key,

    /// Generate a new preshared key
    Psk,

    /// Derive the public key from a private key  <PRIVATE>
    #[command(verbatim_doc_comment)]
    Pubkey {
        /// A base64 private key, as `ewg key` and `wg genkey` print one
        private: String,
    },

    /// Render a wg config as a scannable QR - a .conf file or a manifest node  <TARGET>
    ///   ewg qr <path.conf>          QR for that file (scan into the wg app)
    ///   ewg qr <node> -m mesh.toml  QR for that node's generated config
    #[command(verbatim_doc_comment)]
    Qr {
        /// A `.conf` file path, or a node name in the manifest
        target: String,
        /// Manifest to resolve a node name from
        #[arg(short = 'm', long, default_value = "mesh.toml")]
        manifest: PathBuf,
        /// Also write the QR as a PNG here
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Validate WireGuard configs, exit non-zero if any is broken  <PATH>...
    ///   ewg check ~/wg/home.conf
    ///   ewg check /etc/wireguard/*.conf
    #[command(verbatim_doc_comment)]
    Check {
        /// One or more `.conf` files to validate
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,
    },

    /// Manage easywireguard itself: `self update` reinstalls, `self check` looks for a newer release
    #[command(name = "self", subcommand)]
    Selfie(selfcmd::Cmd),
}

fn main() -> Result<()> {
    // Keep piped output clean: no ANSI unless stdout is a terminal.
    if !std::io::stdout().is_terminal() {
        colored::control::set_override(false);
    }
    let cli = Cli::parse();
    match cli.cmd {
        Some(Cmd::Selfie(cmd)) => selfcmd::run(cmd),
        None | Some(Cmd::Tui) => {
            let dirs = registry::resolve_dirs(cli.dir)?;
            elevate::elevate_for(&dirs)?;
            tui::run(&dirs)
        }
        Some(Cmd::List) => {
            let dirs = registry::resolve_dirs(cli.dir)?;
            elevate::elevate_for(&dirs)?;
            commands::list::run(&dirs)
        }
        Some(Cmd::Status) => {
            let dirs = registry::resolve_dirs(cli.dir)?;
            elevate::elevate_for(&dirs)?;
            commands::status::run(&dirs)
        }
        Some(Cmd::Up { name }) => {
            let dirs = registry::resolve_dirs(cli.dir)?;
            elevate::elevate_for(&dirs)?;
            wg::up(&wg::find(&dirs, &name)?)
        }
        Some(Cmd::Down { name }) => {
            let dirs = registry::resolve_dirs(cli.dir)?;
            elevate::elevate_for(&dirs)?;
            wg::down(&wg::find(&dirs, &name)?)
        }
        Some(Cmd::Dir(action)) => commands::dir::run(action),
        Some(Cmd::Mesh(action)) => commands::mesh::run(action),
        Some(Cmd::Key) => commands::key::run(),
        Some(Cmd::Psk) => commands::psk::run(),
        Some(Cmd::Pubkey { private }) => commands::pubkey::run(&private),
        Some(Cmd::Qr {
            target,
            manifest,
            out,
        }) => commands::qr::run(&target, &manifest, out.as_deref()),
        Some(Cmd::Check { paths }) => commands::check::run(&paths),
    }
}
