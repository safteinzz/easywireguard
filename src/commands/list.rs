//! `ewg list`: every interface across the registered dirs, with up/down state.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::wg;

/// All configs across the dirs, each marked up/down: the "what can I up" list.
/// Green ● up / dim ○ down on a TTY; plain when piped (color is TTY-gated in main).
pub fn run(dirs: &[PathBuf]) -> Result<()> {
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
