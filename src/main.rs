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
    about,
    after_help = "Run bare `ewg` for the interface manager TUI. `dir` registers \
                  where your .conf files live so `list`/`up`/`down`/`status`/TUI \
                  span them all. `mesh` edits a manifest; `mesh gen` turns it into \
                  each node's config as \"all peers minus itself\". `qr` renders any \
                  config (a .conf or a mesh node) as a scannable QR for a phone."
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
#[allow(clippy::large_enum_variant)] // clap arg enums: boxing fights the derive
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
        /// What peers route TO this node: `0.0.0.0/0` (full-tunnel exit) or a LAN
        /// subnet (site-to-site). Defaults to this node's own `/32`.
        #[arg(long = "allowed-ips")]
        allowed_ips: Option<String>,
        /// DNS for this node's own interface (e.g. a Pi-hole behind the tunnel)
        #[arg(long)]
        dns: Option<String>,
        /// Seconds between keepalives peers send to this node (e.g. 25)
        #[arg(long)]
        keepalive: Option<u16>,
        /// Hub(s) this spoke dials, by name (repeatable). Omit = all hubs. Ignored
        /// for a hub (one with an endpoint), which meshes with everyone.
        #[arg(long = "hub")]
        hub: Vec<String>,
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
        Some(Cmd::Qr {
            target,
            manifest,
            out,
        }) => cmd_qr(&target, &manifest, out.as_deref()),
        Some(Cmd::Check { paths }) => cmd_check(&paths),
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
            allowed_ips,
            dns,
            keepalive,
            hub,
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
                allowed_ips,
                dns,
                keepalive,
                hubs: hub,
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

/// Validate each config with the same check the TUI's create/edit uses. Valid
/// files report `ok` on stdout; problems (or an unreadable file) go to stderr, and
/// any failure makes the whole run exit non-zero so it drops into a CI pipeline.
fn cmd_check(paths: &[PathBuf]) -> Result<()> {
    let mut all_ok = true;
    for path in paths {
        let label = path.display();
        match std::fs::read_to_string(path) {
            Ok(text) => match wg::validate_config(&text) {
                Ok(()) => println!("{label}: {}", "ok".green()),
                Err(e) => {
                    all_ok = false;
                    eprintln!("{label}: {}", e.to_string().red());
                }
            },
            Err(e) => {
                all_ok = false;
                eprintln!("{label}: {}", format!("can't read: {e}").red());
            }
        }
    }
    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}

/// Render a wg config as a QR. `target` is an existing `.conf` file, else a node
/// name generated from the manifest. Prints a scannable QR to the terminal;
/// `-o` also writes a PNG.
fn cmd_qr(target: &str, manifest: &Path, out: Option<&Path>) -> Result<()> {
    use qrcode::QrCode;
    use qrcode::render::unicode;

    let config = qr_config(target, manifest)?;

    if config.contains("<PASTE PRIVATE KEY>") {
        eprintln!(
            "{}",
            "warning: config has no private key (placeholder) - the QR won't import a \
             working tunnel; give the node a `private` key first"
                .yellow()
        );
    }

    let code = QrCode::with_error_correction_level(config.as_bytes(), qrcode::EcLevel::L)
        .context("encoding config into a QR")?;

    // Compact half-block QR with a quiet zone - scan it straight off the screen.
    let art = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
    println!("{art}");

    if let Some(path) = out {
        write_png(&code, path)?;
        if std::io::stderr().is_terminal() {
            eprintln!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Resolve the text for `qr`: an existing file's contents win; otherwise treat
/// `target` as a manifest node name and generate that node's config.
fn qr_config(target: &str, manifest: &Path) -> Result<String> {
    if Path::new(target).is_file() {
        return std::fs::read_to_string(target).with_context(|| format!("reading `{target}`"));
    }
    let m = Manifest::load(manifest).with_context(|| {
        format!(
            "`{target}` is not a file, and manifest `{}` couldn't be loaded to resolve it as a node",
            manifest.display()
        )
    })?;
    let node = m
        .nodes
        .iter()
        .find(|n| n.name == target)
        .with_context(|| format!("no `.conf` file or manifest node named `{target}`"))?;
    Ok(m.node_config(node))
}

/// Black-on-white PNG from the QR matrix: each module scaled up, with a 4-module
/// quiet zone. Decoupled from `qrcode`'s own image feature so versions can't clash.
fn write_png(code: &qrcode::QrCode, path: &Path) -> Result<()> {
    use image::{ImageBuffer, Luma};
    const SCALE: u32 = 8;
    const QUIET: u32 = 4;
    let w = code.width() as u32;
    let side = (w + 2 * QUIET) * SCALE;
    let colors = code.to_colors();
    let img = ImageBuffer::from_fn(side, side, |x, y| {
        let (mx, my) = (x / SCALE, y / SCALE);
        if mx < QUIET || my < QUIET || mx >= w + QUIET || my >= w + QUIET {
            return Luma([255u8]); // quiet zone
        }
        let idx = ((my - QUIET) * w + (mx - QUIET)) as usize;
        match colors[idx] {
            qrcode::Color::Dark => Luma([0u8]),
            qrcode::Color::Light => Luma([255u8]),
        }
    });
    img.save(path)
        .with_context(|| format!("writing PNG `{}`", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_config_reads_an_existing_conf_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.conf");
        std::fs::write(&p, "[Interface]\nAddress = 10.0.0.9/24\n").unwrap();
        let got = qr_config(p.to_str().unwrap(), Path::new("does-not-exist.toml")).unwrap();
        assert!(got.contains("10.0.0.9/24"));
    }

    #[test]
    fn qr_config_generates_a_node_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let mp = dir.path().join("mesh.toml");
        std::fs::write(
            &mp,
            "[[node]]\nname='a'\naddress='10.0.0.1/24'\npublic_key='PUB_A'\n\
             [[node]]\nname='b'\naddress='10.0.0.2/24'\npublic_key='PUB_B'\nendpoint='vpn-b:51820'\n",
        )
        .unwrap();
        let got = qr_config("a", &mp).unwrap();
        assert!(got.contains("[Interface]"), "node config missing interface");
        assert!(got.contains("PUB_B"), "peer b should appear");
        assert!(!got.contains("PUB_A"), "a must not peer with itself");
    }

    #[test]
    fn qr_config_unknown_target_errors() {
        let err = qr_config("ghost", Path::new("nope.toml")).unwrap_err().to_string();
        assert!(err.contains("not a file"), "got: {err}");
    }

    #[test]
    fn a_typical_wg_config_fits_in_a_qr() {
        let cfg = "[Interface]\nPrivateKey = MN9c1GcVMpNJ7kV1ZeC6ccml6q9Swz0plla2axvHa0E=\n\
                   Address = 10.10.1.2/24\nDNS = 192.168.10.250\n\n[Peer]\n\
                   PublicKey = FcNyhptCK62097pnVF2P092kob9+8vJtsFMc7Ws4ojc=\n\
                   Endpoint = vpn-villena.safteinzz.com:51820\nAllowedIPs = 0.0.0.0/0\n";
        assert!(qrcode::QrCode::new(cfg.as_bytes()).is_ok());
    }
}
