//! `ewg psk`: a fresh preshared key.

use anyhow::Result;

use crate::keys;

pub fn run() -> Result<()> {
    println!("{}", keys::gen_psk());
    Ok(())
}
