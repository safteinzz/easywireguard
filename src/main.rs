//! ewg - wireguard config generation and management without the hand-editing.

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

const AFTER: &str = concat!(
    "Run bare `ewg` for the interface manager TUI. `dir` registers \
     where your .conf files live so `list`/`up`/`down`/`status`/TUI \
     span them all. `mesh` edits a manifest; `mesh gen` turns it into \
     each node's config as \"all peers minus itself\". `qr` renders any \
     config (a .conf or a mesh node) as a scannable QR for a phone.",
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
    name = "ewg",
    version,
    long_version = LONG_VERSION,
    about,
    after_help = AFTER
)]
struct Cli {
    /// Use only this dir for this run, overriding the registry (or set $EWG_DIR)
    #[arg(long, global = true, env = "EWG_DIR")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // clap arg enums: boxing fights the derive
enum Cmd {
    /// Interface manager TUI (also the default with no subcommand)
    Tui,

    /// Manage easywireguard itself: `self update` reinstalls, `self check` looks for a newer release
    #[command(name = "self", subcommand)]
    Selfie(selfcmd::Cmd),

    /// List every interface you can up/down across your dirs, with up/down state
    #[command(visible_alias = "ls")]
    List,

    /// Show only the interfaces currently up (real wireguard status)
    Status,

    /// Bring an interface up  <NAME>
    #[command(verbatim_doc_comment)]
    Up { name: String },

    /// Bring an interface down  <NAME>
    #[command(verbatim_doc_comment)]
    Down { name: String },

    /// List registered config directories (bare = list; -v verbose; add/rm to edit)
    Dir(DirArgs),

    /// Design a mesh: add/list/rm nodes in a manifest, then gen the configs
    Mesh(MeshArgs),

    /// Generate a new WireGuard keypair (private + public)
    Key,

    /// Generate a new preshared key
    Psk,

    /// Derive the public key from a private key  <PRIVATE>
    #[command(verbatim_doc_comment)]
    Pubkey { private: String },

    /// Render a wg config as a scannable QR - a .conf file or a manifest node
    ///   ewg qr <path.conf>           QR for that file (scan into the wg app)
    ///   ewg qr <node> -m mesh.toml   QR for that node's generated config
    ///   -o FILE.png                  also write a PNG
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
    ///   ewg check /etc/wireguard/*.conf   # sanity-check a whole dir in CI
    #[command(verbatim_doc_comment)]
    Check {
        /// One or more `.conf` files to validate
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
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
