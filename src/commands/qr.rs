//! `ewg qr`: render a config as a scannable QR, and optionally a PNG.

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

use crate::manifest::Manifest;
use std::io::IsTerminal;

/// Render a wg config as a QR. `target` is an existing `.conf` file, else a node
/// name generated from the manifest. Prints a scannable QR to the terminal;
/// `-o` also writes a PNG.
pub fn run(target: &str, manifest: &Path, out: Option<&Path>) -> Result<()> {
    use qrcode::QrCode;
    use qrcode::render::unicode;

    let config = qr_config(target, manifest)?;

    if config.contains("<PASTE PRIVATE KEY>") {
        eprintln!(
            "{}",
            "warning: config has no private key (placeholder) - the QR won't import a \
             working tunnel; give the node a `private` key first"
                .yellow()
        );
    }

    let code = QrCode::with_error_correction_level(config.as_bytes(), qrcode::EcLevel::L)
        .context("encoding config into a QR")?;

    // Compact half-block QR with a quiet zone - scan it straight off the screen.
    let art = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
    println!("{art}");

    if let Some(path) = out {
        write_png(&code, path)?;
        if std::io::stderr().is_terminal() {
            eprintln!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Resolve the text for `qr`: an existing file's contents win; otherwise treat
/// `target` as a manifest node name and generate that node's config.
pub(crate) fn qr_config(target: &str, manifest: &Path) -> Result<String> {
    if Path::new(target).is_file() {
        return std::fs::read_to_string(target).with_context(|| format!("reading `{target}`"));
    }
    let m = Manifest::load(manifest).with_context(|| {
        format!(
            "`{target}` is not a file, and manifest `{}` couldn't be loaded to resolve it as a node",
            manifest.display()
        )
    })?;
    let node = m
        .nodes
        .iter()
        .find(|n| n.name == target)
        .with_context(|| format!("no `.conf` file or manifest node named `{target}`"))?;
    Ok(m.node_config(node))
}

/// Black-on-white PNG from the QR matrix: each module scaled up, with a 4-module
/// quiet zone. Decoupled from `qrcode`'s own image feature so versions can't clash.
pub(crate) fn write_png(code: &qrcode::QrCode, path: &Path) -> Result<()> {
    use image::{ImageBuffer, Luma};
    const SCALE: u32 = 8;
    const QUIET: u32 = 4;
    let w = code.width() as u32;
    let side = (w + 2 * QUIET) * SCALE;
    let colors = code.to_colors();
    let img = ImageBuffer::from_fn(side, side, |x, y| {
        let (mx, my) = (x / SCALE, y / SCALE);
        if mx < QUIET || my < QUIET || mx >= w + QUIET || my >= w + QUIET {
            return Luma([255u8]); // quiet zone
        }
        let idx = ((my - QUIET) * w + (mx - QUIET)) as usize;
        match colors[idx] {
            qrcode::Color::Dark => Luma([0u8]),
            qrcode::Color::Light => Luma([255u8]),
        }
    });
    img.save(path)
        .with_context(|| format!("writing PNG `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_config_reads_an_existing_conf_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.conf");
        std::fs::write(&p, "[Interface]\nAddress = 10.0.0.9/24\n").unwrap();
        let got = qr_config(p.to_str().unwrap(), Path::new("does-not-exist.toml")).unwrap();
        assert!(got.contains("10.0.0.9/24"));
    }

    #[test]
    fn qr_config_generates_a_node_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let mp = dir.path().join("mesh.toml");
        std::fs::write(
            &mp,
            "[[node]]\nname='a'\naddress='10.0.0.1/24'\npublic_key='PUB_A'\n\
             [[node]]\nname='b'\naddress='10.0.0.2/24'\npublic_key='PUB_B'\nendpoint='vpn-b:51820'\n",
        )
        .unwrap();
        let got = qr_config("a", &mp).unwrap();
        assert!(got.contains("[Interface]"), "node config missing interface");
        assert!(got.contains("PUB_B"), "peer b should appear");
        assert!(!got.contains("PUB_A"), "a must not peer with itself");
    }

    #[test]
    fn qr_config_unknown_target_errors() {
        let err = qr_config("ghost", Path::new("nope.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a file"), "got: {err}");
    }

    #[test]
    fn a_typical_wg_config_fits_in_a_qr() {
        let cfg = "[Interface]\nPrivateKey = MN9c1GcVMpNJ7kV1ZeC6ccml6q9Swz0plla2axvHa0E=\n\
                   Address = 10.10.1.2/24\nDNS = 192.168.10.250\n\n[Peer]\n\
                   PublicKey = FcNyhptCK62097pnVF2P092kob9+8vJtsFMc7Ws4ojc=\n\
                   Endpoint = vpn-villena.safteinzz.com:51820\nAllowedIPs = 0.0.0.0/0\n";
        assert!(qrcode::QrCode::new(cfg.as_bytes()).is_ok());
    }
}
