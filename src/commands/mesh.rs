//! `ewg mesh`: design a mesh in a manifest, then generate each node's config.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::manifest;
use crate::manifest::Manifest;
use clap::{Args, Subcommand};
use std::io::IsTerminal;

#[derive(Args)]
pub struct MeshArgs {
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
pub enum MeshAction {
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

pub fn run(args: MeshArgs) -> Result<()> {
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
