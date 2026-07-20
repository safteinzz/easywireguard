//! WireGuard key material: X25519 keypairs and preshared keys, pure Rust.
//!
//! Keys are the base64 of 32 raw bytes, exactly like `wg genkey`/`wg pubkey`.
//! We clamp the private scalar on generation so output is canonical; the public
//! key is `clamped_private * basepoint`, which is what WireGuard computes too.

use anyhow::{Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use x25519_dalek::{X25519_BASEPOINT_BYTES, x25519};

/// A fresh private key (base64), clamped so it round-trips identically to `wg`.
pub fn gen_private() -> String {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).expect("system RNG unavailable");
    clamp(&mut b);
    STANDARD.encode(b)
}

/// A fresh preshared key: 32 random bytes, base64 (same as `wg genpsk`).
pub fn gen_psk() -> String {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).expect("system RNG unavailable");
    STANDARD.encode(b)
}

/// Derive the public key from a base64 private key.
pub fn public_from_private(private_b64: &str) -> Result<String> {
    let bytes = decode_key(private_b64)?;
    // x25519() clamps its scalar input, matching `wg pubkey`.
    let public = x25519(bytes, X25519_BASEPOINT_BYTES);
    Ok(STANDARD.encode(public))
}

/// Curve25519 secret-key clamping (see RFC 7748): the low 3 bits and the top
/// two bits are fixed so every 32-byte value maps to a valid scalar.
fn clamp(b: &mut [u8; 32]) {
    b[0] &= 248;
    b[31] &= 127;
    b[31] |= 64;
}

fn decode_key(s: &str) -> Result<[u8; 32]> {
    let v = STANDARD
        .decode(s.trim())
        .map_err(|_| anyhow!("invalid key `{s}`: not valid base64"))?;
    if v.len() != 32 {
        bail!("invalid key `{s}`: expected 32 bytes, got {}", v.len());
    }
    let mut a = [0u8; 32];
    a.copy_from_slice(&v);
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_private_key_is_32_bytes_base64() {
        let k = gen_private();
        let raw = STANDARD.decode(&k).unwrap();
        assert_eq!(raw.len(), 32);
    }

    #[test]
    fn generated_private_key_is_clamped() {
        for _ in 0..100 {
            let raw = STANDARD.decode(gen_private()).unwrap();
            assert_eq!(raw[0] & 7, 0, "low 3 bits must be clear");
            assert_eq!(raw[31] & 128, 0, "top bit must be clear");
            assert_eq!(raw[31] & 64, 64, "bit 254 must be set");
        }
    }

    #[test]
    fn public_key_derivation_is_deterministic() {
        let priv_key = gen_private();
        let a = public_from_private(&priv_key).unwrap();
        let b = public_from_private(&priv_key).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn known_pair_matches_wireguard() {
        // Generated with `wg genkey | wg pubkey` and verified.
        let priv_key = "wFW7oUjIpLCfZW2UwsfTlLDGrZb9iJH3bK6nosB5IGI=";
        let expected_pub = "obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA=";
        assert_eq!(public_from_private(priv_key).unwrap(), expected_pub);
    }

    #[test]
    fn bad_key_gives_a_helpful_error() {
        let e = public_from_private("not base64!!!")
            .unwrap_err()
            .to_string();
        assert!(e.contains("not valid base64"), "got: {e}");

        let short = STANDARD.encode([0u8; 16]);
        let e = public_from_private(&short).unwrap_err().to_string();
        assert!(e.contains("expected 32 bytes"), "got: {e}");
    }

    #[test]
    fn psk_is_32_bytes_base64() {
        assert_eq!(STANDARD.decode(gen_psk()).unwrap().len(), 32);
    }
}
