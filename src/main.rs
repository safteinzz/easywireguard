//! ewg - wireguard config generation and management without the hand-editing.

mod keys;
mod manifest;
mod registry;
mod tui;
mod wg;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use manifest::Manifest;
use registry::Registry;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "ewg",
    version,
    about = "make wireguard easy - interfaces, keys and full-mesh configs in one CLI + TUI",
    after_help = "Run bare `ewg` for the interface manager TUI. `dir` registers \
                  where your .conf files live so `list`/`up`/`down`/`status`/TUI \
                  span them all. `mesh` edits a manifest; `mesh gen` turns it into \
                  each node's config as \"all peers minus itself\"."
)]
struct Cli {
    /// Use only this dir for this run, overriding the registry (or set $EWG_DIR)
    #[arg(long, global = true, env = "EWG_DIR")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Interface manager TUI (also the default with no subcommand)
    Tui,

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

    /// Update ewg to the latest release
    ///   -y   skip the confirm prompt
    #[command(verbatim_doc_comment)]
    Update {
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Args)]
struct DirArgs {
    #[command(subcommand)]
    action: Option<DirAction>,
    /// Verbose: show each dir's .conf count
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Machine-readable JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum DirAction {
    /// Register a directory that holds .conf files  <PATH>
    #[command(verbatim_doc_comment)]
    Add { path: PathBuf },
    /// Unregister a directory  <PATH>
    #[command(verbatim_doc_comment)]
    Rm { path: PathBuf },
    /// List the directories being scanned (same as bare `dir`)
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Args)]
struct MeshArgs {
    #[command(subcommand)]
    action: Option<MeshAction>,
    /// Verbose: show address and endpoint, not just names
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Machine-readable JSON
    #[arg(long, global = true)]
    json: bool,
    /// Manifest file to read/edit
    #[arg(short = 'm', long, global = true, default_value = "mesh.toml")]
    manifest: PathBuf,
}

#[derive(Subcommand)]
enum MeshAction {
    /// Add a node to the manifest  <NAME>
    #[command(verbatim_doc_comment)]
    Add {
        name: String,
        #[arg(long)]
        address: String,
        #[arg(long)]
        pubkey: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        private: Option<String>,
        #[arg(long)]
        postup: Option<String>,
        #[arg(long)]
        postdown: Option<String>,
    },
    /// Remove a node  <NAME>
    #[command(verbatim_doc_comment)]
    Rm { name: String },
    /// List nodes in the manifest (same as bare `mesh`)
    #[command(visible_alias = "ls")]
    List,
    /// Generate each node's wg config from the manifest
    ///   -o DIR   output directory (default: current directory)
    #[command(verbatim_doc_comment)]
    Gen {
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    // Keep piped output clean: no ANSI unless stdout is a terminal.
    if !std::io::stdout().is_terminal() {
        colored::control::set_override(false);
    }
    let cli = Cli::parse();
    match cli.cmd {
        None | Some(Cmd::Tui) => {
            let dirs = resolve_dirs(cli.dir)?;
            elevate_for(&dirs)?;
            tui::run(&dirs)
        }
        Some(Cmd::List) => {
            let dirs = resolve_dirs(cli.dir)?;
            elevate_for(&dirs)?;
            cmd_list(&dirs)
        }
        Some(Cmd::Status) => {
            let dirs = resolve_dirs(cli.dir)?;
            elevate_for(&dirs)?;
            cmd_status(&dirs)
        }
        Some(Cmd::Up { name }) => {
            let dirs = resolve_dirs(cli.dir)?;
            elevate_for(&dirs)?;
            wg::up(&wg::find(&dirs, &name)?)
        }
        Some(Cmd::Down { name }) => {
            let dirs = resolve_dirs(cli.dir)?;
            elevate_for(&dirs)?;
            wg::down(&wg::find(&dirs, &name)?)
        }
        Some(Cmd::Dir(action)) => cmd_dir(action),
        Some(Cmd::Mesh(action)) => cmd_mesh(action),
        Some(Cmd::Key) => cmd_key(),
        Some(Cmd::Psk) => cmd_psk(),
        Some(Cmd::Pubkey { private }) => cmd_pubkey(&private),
        Some(Cmd::Update { yes }) => cmd_update(yes),
    }
}

/// The dirs to operate on: a `--dir` override, else the registry (which falls
/// back to `/etc/wireguard` when empty).
fn resolve_dirs(cli_dir: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(dir) = cli_dir {
        return Ok(vec![dir]);
    }
    Ok(Registry::load(&registry::default_path()?)?.effective())
}

/// All configs across the dirs, each marked up/down: the "what can I up" list.
/// Green ● up / dim ○ down on a TTY; plain when piped (color is TTY-gated in main).
fn cmd_list(dirs: &[PathBuf]) -> Result<()> {
    let ifaces = wg::interfaces(dirs)?;
    if ifaces.is_empty() {
        println!("no .conf files found (register a dir: ewg dir add <path>)");
        return Ok(());
    }
    for i in &ifaces {
        let marker = if i.up {
            "● up  ".green()
        } else {
            "○ down".dimmed()
        };
        println!("{marker}  {}", i.name);
    }
    Ok(())
}

/// Only the interfaces currently up: real wireguard status.
fn cmd_status(dirs: &[PathBuf]) -> Result<()> {
    let up: Vec<_> = wg::interfaces(dirs)?.into_iter().filter(|i| i.up).collect();
    if up.is_empty() {
        println!("nothing up");
        return Ok(());
    }
    for i in up {
        println!("{}", i.name);
    }
    Ok(())
}

fn cmd_dir(args: DirArgs) -> Result<()> {
    let path = registry::default_path()?;
    let mut reg = Registry::load(&path)?;
    match args.action {
        Some(DirAction::Add { path: dir }) => {
            if reg.add(dir.clone()) {
                reg.save(&path)?;
                println!("registered {}", dir.display());
            } else {
                println!("{} already registered", dir.display());
            }
        }
        Some(DirAction::Rm { path: dir }) => {
            if reg.remove(&dir) {
                reg.save(&path)?;
                println!("unregistered {}", dir.display());
            } else {
                println!("{} was not registered", dir.display());
            }
        }
        None | Some(DirAction::List) => {
            let dirs = reg.effective();
            if args.json {
                println!("{}", serde_json::to_string_pretty(&dirs)?);
            } else if args.verbose {
                for d in &dirs {
                    match conf_count(d) {
                        Some(n) => println!("{}  ({n} configs)", d.display()),
                        None => println!("{}  (unreadable)", d.display()),
                    }
                }
            } else {
                for d in &dirs {
                    println!("{}", d.display());
                }
            }
        }
    }
    Ok(())
}

/// Number of `.conf` files in a dir, or None if it can't be read.
fn conf_count(dir: &Path) -> Option<usize> {
    let count = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("conf"))
        .count();
    Some(count)
}

fn cmd_mesh(args: MeshArgs) -> Result<()> {
    match args.action {
        Some(MeshAction::Add {
            name,
            address,
            pubkey,
            endpoint,
            private,
            postup,
            postdown,
        }) => {
            let mut m = Manifest::load_or_empty(&args.manifest)?;
            m.add(manifest::Node {
                name: name.clone(),
                address,
                public_key: pubkey,
                endpoint,
                private_key: private,
                post_up: postup,
                post_down: postdown,
            })?;
            m.save(&args.manifest)?;
            println!("added node `{name}`");
        }
        Some(MeshAction::Rm { name }) => {
            let mut m = Manifest::load_or_empty(&args.manifest)?;
            m.remove(&name)?;
            m.save(&args.manifest)?;
            println!("removed node `{name}`");
        }
        Some(MeshAction::Gen { out }) => {
            let manifest = Manifest::load(&args.manifest)?;
            std::fs::create_dir_all(&out)
                .with_context(|| format!("creating output dir `{}`", out.display()))?;
            for node in &manifest.nodes {
                let path = out.join(format!("{}.conf", node.name));
                std::fs::write(&path, manifest.node_config(node))
                    .with_context(|| format!("writing `{}`", path.display()))?;
                if std::io::stderr().is_terminal() {
                    eprintln!("wrote {}", path.display());
                }
            }
        }
        None | Some(MeshAction::List) => {
            let m = Manifest::load_or_empty(&args.manifest)?;
            if args.json {
                let view: Vec<_> = m
                    .nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "address": n.address,
                            "mesh_ip": n.mesh_ip(),
                            "public_key": n.public_key,
                            "endpoint": n.endpoint,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else if args.verbose {
                for n in &m.nodes {
                    println!(
                        "{:<12} {:<16} {}",
                        n.name,
                        n.mesh_ip(),
                        n.endpoint.as_deref().unwrap_or("-")
                    );
                }
            } else {
                for n in &m.nodes {
                    println!("{}", n.name);
                }
            }
        }
    }
    Ok(())
}

fn cmd_key() -> Result<()> {
    let private = keys::gen_private();
    let public = keys::public_from_private(&private)?;
    println!("PrivateKey = {private}");
    println!("PublicKey  = {public}");
    Ok(())
}

fn cmd_psk() -> Result<()> {
    println!("{}", keys::gen_psk());
    Ok(())
}

fn cmd_pubkey(private: &str) -> Result<()> {
    println!("{}", keys::public_from_private(private)?);
    Ok(())
}

fn cmd_update(yes: bool) -> Result<()> {
    if !yes {
        print!("update ewg via `cargo install easywireguard --force`? [y/N] ");
        std::io::stdout().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y") {
            eprintln!("cancelled");
            return Ok(());
        }
    }
    let status = std::process::Command::new("cargo")
        .args(["install", "easywireguard", "--force"])
        .status()
        .context("running `cargo install` - is cargo on your PATH?")?;
    if !status.success() {
        anyhow::bail!("update failed");
    }
    Ok(())
}

/// Re-exec under sudo when a config dir isn't readable, so `ewg` "just works"
/// without `sudo $(which ewg)`. If every dir reads fine (root, or readable) we
/// proceed as-is; set `EWG_NO_SUDO=1` to never auto-elevate.
#[cfg(unix)]
fn elevate_for(dirs: &[PathBuf]) -> Result<()> {
    use std::io::ErrorKind;
    use std::os::unix::process::CommandExt;

    if std::env::var_os("EWG_NO_SUDO").is_some() {
        return Ok(());
    }
    let blocked = dirs.iter().find(
        |d| matches!(std::fs::read_dir(d), Err(e) if e.kind() == ErrorKind::PermissionDenied),
    );
    let Some(dir) = blocked else {
        return Ok(());
    };

    eprintln!(
        "easywireguard cannot access the config files in {} without root.\n\
         elevating with sudo (set EWG_NO_SUDO=1 to disable)...",
        dir.display()
    );
    let exe = std::env::current_exe().context("finding own executable path")?;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let err = std::process::Command::new("sudo")
        .arg("--")
        .arg(&exe)
        .args(args)
        .exec(); // replaces this process; only returns on failure
    anyhow::bail!(
        "could not elevate via sudo: {err} (try: sudo {})",
        exe.display()
    );
}

#[cfg(not(unix))]
fn elevate_for(_dirs: &[PathBuf]) -> Result<()> {
    Ok(())
}
