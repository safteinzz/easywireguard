//! Talking to the system's WireGuard: list `.conf` interfaces across one or more
//! directories, see which are up (`wg show interfaces`), and toggle them
//! (`wg-quick up|down <path>`, by full path so any registered dir works).
//!
//! When `wg` can't be queried (no root, not installed) we degrade gracefully:
//! interfaces just show as down rather than erroring the whole listing.

use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEFAULT_DIR: &str = "/etc/wireguard";

#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub up: bool,
    /// Whether `wg-quick@<name>` is enabled at boot, per systemd. `None` when we
    /// can't tell (no systemctl) so the UI hides the state instead of lying.
    pub enabled: Option<bool>,
    pub path: PathBuf,
}

/// Every `*.conf` across `dirs`, sorted by name, each flagged up/down. When a
/// name exists in more than one dir the earlier dir wins (later ones are
/// shadowed and skipped). Errors only if none of the dirs could be read.
pub fn interfaces(dirs: &[PathBuf]) -> Result<Vec<Iface>> {
    let active = active_interfaces().unwrap_or_default();
    let mut found: BTreeMap<String, Iface> = BTreeMap::new();
    let mut any_readable = false;
    let mut last_err: Option<(PathBuf, std::io::Error)> = None;

    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => {
                any_readable = true;
                e
            }
            Err(e) => {
                last_err = Some((dir.clone(), e));
                continue;
            }
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("conf") {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                found.entry(name.to_string()).or_insert_with(|| Iface {
                    name: name.to_string(),
                    up: active.contains(name),
                    enabled: boot_enabled(name),
                    path: path.clone(),
                });
            }
        }
    }

    if !any_readable {
        if let Some((dir, e)) = last_err {
            return Err(anyhow::Error::new(e)).with_context(|| {
                format!(
                    "reading `{}` (config dir needs root: sudo ewg)",
                    dir.display()
                )
            });
        }
        return Ok(Vec::new());
    }
    Ok(found.into_values().collect())
}

/// Full path of `<name>.conf` in the first `dirs` entry that has it.
pub fn find(dirs: &[PathBuf], name: &str) -> Result<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(format!("{name}.conf"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("no `{name}.conf` in any registered dir - add its dir: ewg dir add <path>");
}

/// Interface names currently up, from `wg show interfaces`.
pub fn active_interfaces() -> Result<BTreeSet<String>> {
    let out = Command::new("wg")
        .args(["show", "interfaces"])
        .output()
        .context("running `wg` (install wireguard-tools)")?;
    if !out.status.success() {
        bail!("`wg show interfaces` failed (need root?)");
    }
    Ok(parse_interfaces(&String::from_utf8_lossy(&out.stdout)))
}

/// `wg show interfaces` prints names separated by whitespace on one line.
pub fn parse_interfaces(s: &str) -> BTreeSet<String> {
    s.split_whitespace().map(str::to_string).collect()
}

pub fn up(config: &Path) -> Result<()> {
    wg_quick("up", config)
}

pub fn down(config: &Path) -> Result<()> {
    wg_quick("down", config)
}

fn wg_quick(action: &str, config: &Path) -> Result<()> {
    // Capture output rather than inherit: wg-quick prints a wall of `[#] ip ...`
    // lines that would otherwise flood and corrupt the TUI's alternate screen.
    let out = Command::new("wg-quick")
        .arg(action)
        .arg(config)
        .output()
        .context("running `wg-quick` (install wireguard-tools)")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.lines().last().unwrap_or("").trim();
        bail!("`wg-quick {action} {}` failed: {reason}", config.display());
    }
    Ok(())
}

/// Live `wg show <name>` for an up interface: peers, latest handshake, transfer.
/// The single most useful "is it actually working" readout, shown in inspect.
pub fn show(name: &str) -> Result<String> {
    let out = Command::new("wg")
        .args(["show", name])
        .output()
        .context("running `wg` (install wireguard-tools)")?;
    if !out.status.success() {
        bail!(
            "`wg show {name}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `wg-quick@<name>` is enabled to start at boot. `None` when we can't ask
/// (no systemd / `systemctl` not on PATH), so callers hide the state rather than
/// claim "disabled". Only a plain `enabled` counts as on.
pub fn boot_enabled(name: &str) -> Option<bool> {
    let out = Command::new("systemctl")
        .args(["is-enabled", &format!("wg-quick@{name}")])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim() == "enabled")
}

/// Enable or disable `wg-quick@<name>` at boot via systemd. Needs root + systemd;
/// the error says so rather than leaking a raw systemctl message.
pub fn set_boot(name: &str, enable: bool) -> Result<()> {
    let action = if enable { "enable" } else { "disable" };
    let out = Command::new("systemctl")
        .arg(action)
        .arg(format!("wg-quick@{name}"))
        .output()
        .context("running `systemctl` (need systemd)")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.lines().last().unwrap_or("").trim();
        bail!("`systemctl {action} wg-quick@{name}` failed: {reason}");
    }
    Ok(())
}

/// Parse a wg config into `(section-header, [(key, value)])`, comments and blank
/// lines dropped. Deliberately lenient: unknown keys and provider-specific extras
/// are kept, since we only sanity-check structure, never rewrite the file.
fn sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            out.push((line.to_string(), Vec::new()));
        } else if let Some((k, v)) = line.split_once('=')
            && let Some((_, kvs)) = out.last_mut()
        {
            kvs.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// Structurally validate a WireGuard config before we save it, so a create/edit
/// catches an empty or malformed paste instead of writing a `.conf` that only
/// fails later at `wg-quick up`. Checks the essentials (an `[Interface]` with a
/// valid `PrivateKey` and an `Address`, and at least one `[Peer]` with a valid
/// `PublicKey`) and stays lenient about everything else. Errors are ready to show.
pub fn validate_config(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("the config is empty - paste or write one (or clear it to cancel)");
    }
    let secs = sections(text);
    let get = |kvs: &[(String, String)], key: &str| {
        kvs.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let Some((_, iface)) = secs
        .iter()
        .find(|(h, _)| h.eq_ignore_ascii_case("[Interface]"))
    else {
        bail!("no `[Interface]` section - is this a WireGuard config?");
    };
    match get(iface, "PrivateKey") {
        None => bail!("`[Interface]` is missing a `PrivateKey`"),
        Some(k) if !crate::keys::is_wg_key(&k) => {
            bail!("`PrivateKey` is not a valid WireGuard key (expected 44-char base64)")
        }
        _ => {}
    }
    if get(iface, "Address").is_none() {
        bail!("`[Interface]` is missing an `Address` (the interface IP, e.g. 10.0.0.2/24)");
    }
    let peers: Vec<_> = secs
        .iter()
        .filter(|(h, _)| h.eq_ignore_ascii_case("[Peer]"))
        .collect();
    if peers.is_empty() {
        bail!("no `[Peer]` section - add the server/hub this connects to");
    }
    for (_, kvs) in &peers {
        match get(kvs, "PublicKey") {
            None => bail!("a `[Peer]` is missing a `PublicKey`"),
            Some(k) if !crate::keys::is_wg_key(&k) => {
                bail!("a `[Peer]` `PublicKey` is not a valid WireGuard key")
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate a `.conf` stem, which `wg-quick` uses verbatim as the kernel interface
/// name: 1..=15 bytes and only chars `ip link` accepts. Returns a ready-to-show
/// lowercase error that names the fix, so a bad name is caught here, not as a raw
/// `wg-quick` failure later.
pub fn valid_iface_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("an interface name is required");
    }
    if name.len() > 15 {
        bail!("`{name}` is too long - interface names are 15 characters max");
    }
    if name == "." || name == ".." {
        bail!("`{name}` is not a usable interface name");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@')))
    {
        bail!("`{name}` has an invalid character `{bad}` - use letters, digits, `-` `_` `.` `@`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interfaces_splits_on_whitespace() {
        let set = parse_interfaces("wg0 wg1  mesh\n");
        assert_eq!(set.len(), 3);
        assert!(set.contains("wg0") && set.contains("wg1") && set.contains("mesh"));
    }

    #[test]
    fn interfaces_lists_conf_files_across_dirs_sorted() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("wg1.conf"), "").unwrap();
        std::fs::write(a.path().join("notes.txt"), "").unwrap();
        std::fs::write(b.path().join("wg0.conf"), "").unwrap();

        let dirs = vec![a.path().to_path_buf(), b.path().to_path_buf()];
        let names: Vec<_> = interfaces(&dirs)
            .unwrap()
            .into_iter()
            .map(|i| i.name)
            .collect();
        assert_eq!(names, vec!["wg0", "wg1"]); // merged + sorted, .txt ignored
    }

    #[test]
    fn earlier_dir_shadows_a_duplicate_name() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("wg0.conf"), "").unwrap();
        std::fs::write(b.path().join("wg0.conf"), "").unwrap();

        let dirs = vec![a.path().to_path_buf(), b.path().to_path_buf()];
        let ifaces = interfaces(&dirs).unwrap();
        assert_eq!(ifaces.len(), 1);
        assert!(ifaces[0].path.starts_with(a.path()), "first dir wins");
    }

    #[test]
    fn find_locates_a_config_in_the_first_matching_dir() {
        let a = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("wg0.conf"), "").unwrap();
        let dirs = vec![a.path().to_path_buf()];
        assert_eq!(find(&dirs, "wg0").unwrap(), a.path().join("wg0.conf"));

        let e = find(&dirs, "nope").unwrap_err().to_string();
        assert!(e.contains("ewg dir add"), "got: {e}");
    }

    #[test]
    fn valid_iface_name_accepts_and_rejects() {
        assert!(valid_iface_name("wg0").is_ok());
        assert!(valid_iface_name("wg-home_1.a@b").is_ok());
        assert!(valid_iface_name("").is_err(), "empty is rejected");
        assert!(valid_iface_name("a/b").is_err(), "slash is rejected");
        assert!(
            valid_iface_name("has space").is_err(),
            "whitespace is rejected"
        );
        assert!(
            valid_iface_name("0123456789abcdef").is_err(),
            "16 chars is too long"
        );
        assert!(valid_iface_name("..").is_err(), "dot-dot is rejected");
    }

    #[test]
    fn validate_config_accepts_a_real_config_and_flags_problems() {
        // A known-good key pair (from the keys tests) keeps the base64 checks honest.
        let priv_k = "wFW7oUjIpLCfZW2UwsfTlLDGrZb9iJH3bK6nosB5IGI=";
        let pub_k = "obuvsSP3vVFDjzrcwCWqgLmZeqEEVBGHIqzX3v4hYHA=";
        let good = format!(
            "[Interface]\nPrivateKey = {priv_k}\nAddress = 10.0.0.2/24\n\n[Peer]\nPublicKey = {pub_k}\nEndpoint = vpn.example:51820\nAllowedIPs = 0.0.0.0/0\n"
        );
        assert!(
            validate_config(&good).is_ok(),
            "a complete config is accepted"
        );

        assert!(
            validate_config("   \n")
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
        assert!(
            validate_config("PrivateKey = x\n")
                .unwrap_err()
                .to_string()
                .contains("[Interface]")
        );

        let no_priv =
            format!("[Interface]\nAddress = 10.0.0.2/24\n\n[Peer]\nPublicKey = {pub_k}\n");
        assert!(
            validate_config(&no_priv)
                .unwrap_err()
                .to_string()
                .contains("PrivateKey")
        );

        let bad_priv = format!(
            "[Interface]\nPrivateKey = not-a-key\nAddress = 10.0.0.2/24\n\n[Peer]\nPublicKey = {pub_k}\n"
        );
        assert!(
            validate_config(&bad_priv)
                .unwrap_err()
                .to_string()
                .contains("valid WireGuard key")
        );

        let no_peer = format!("[Interface]\nPrivateKey = {priv_k}\nAddress = 10.0.0.2/24\n");
        assert!(
            validate_config(&no_peer)
                .unwrap_err()
                .to_string()
                .contains("[Peer]")
        );
    }

    #[test]
    fn interfaces_all_unreadable_dirs_report_sudo() {
        let e = interfaces(&[PathBuf::from("/nope/x")])
            .unwrap_err()
            .to_string();
        assert!(e.contains("sudo ewg"), "got: {e}");
    }
}
