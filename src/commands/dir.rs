//! `ewg dir`: the registry of directories where your `.conf` files live.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::registry;
use crate::registry::Registry;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct DirArgs {
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
pub enum DirAction {
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

pub fn run(args: DirArgs) -> Result<()> {
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
pub(crate) fn conf_count(dir: &Path) -> Option<usize> {
    let count = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("conf"))
        .count();
    Some(count)
}
