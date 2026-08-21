//! `ewg key`: a fresh WireGuard keypair.

use anyhow::Result;

use crate::keys;

pub fn run() -> Result<()> {
    let private = keys::gen_private();
    let public = keys::public_from_private(&private)?;
    println!("PrivateKey = {private}");
    println!("PublicKey  = {public}");
    Ok(())
}
