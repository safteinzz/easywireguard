//! One file per command: each exposes a `run`, and `main` does nothing but
//! parse the arguments and call one of them.

pub mod check;
pub mod dir;
pub mod key;
pub mod list;
pub mod mesh;
pub mod psk;
pub mod pubkey;
pub mod qr;
pub mod status;
