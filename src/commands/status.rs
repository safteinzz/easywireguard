//! `ewg status`: only the interfaces wireguard reports as up.

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

use crate::wg;

/// Only the interfaces currently up: real wireguard status.
pub fn run(dirs: &[PathBuf]) -> Result<()> {
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

/// One up interface as `status --json` prints it; the field names are fixed.
#[derive(Serialize)]
struct Up {
    name: String,
    interface: String,
    config: Option<PathBuf>,
}

/// The interfaces up as a JSON array, from the kernel rather than `wg`, so it
/// never needs root: callers must not elevate before it.
pub fn run_json(dirs: &[PathBuf]) -> Result<()> {
    let up: Vec<Up> = wg::kernel_interfaces()?
        .into_iter()
        .map(|interface| Up {
            // `wg-quick` names the interface after its `.conf`, so the two agree.
            name: interface.clone(),
            config: wg::readable_config(dirs, &interface),
            interface,
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&up)?);
    Ok(())
}
