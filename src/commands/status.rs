//! `ewg status`: only the interfaces wireguard reports as up.

use anyhow::Result;
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
