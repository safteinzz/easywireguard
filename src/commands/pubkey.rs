//! `ewg pubkey`: derive the public key from a private one.

use anyhow::Result;

use crate::keys;

pub fn run(private: &str) -> Result<()> {
    println!("{}", keys::public_from_private(private)?);
    Ok(())
}
