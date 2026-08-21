//! The Mesh tab: nodes in a manifest, their QRs, rotation and export.

use anyhow::Result;

use crate::keys;
use crate::manifest::{Manifest, Node};

use super::edit::write_conf;
use super::overlay::ExportKind;
use super::overlay::compact_for_qr;
use super::*;
use qrcode::QrCode;

impl App {
    /// Delete `name` from the manifest and refresh the in-memory list.
    pub(super) fn delete_node(&mut self, name: &str) {
        let mut m = match Manifest::load_or_empty(&self.manifest_path) {
            Ok(m) => m,
            Err(e) => {
                self.set_status(format!("manifest: {e}"));
                return;
            }
        };
        match m.remove(name) {
            Ok(()) => {
                if let Err(e) = m.save(&self.manifest_path) {
                    self.set_status(format!("save failed: {e}"));
                    return;
                }
                self.nodes = m.nodes;
                Self::clamp(&mut self.node_state, self.nodes.len());
                self.set_status(format!("deleted `{name}`"));
            }
            Err(e) => self.set_status(format!("delete failed: {e}")),
        }
    }

    /// Re-show a node's QR from the manifest. The manifest is public-only, so this
    /// yields a placeholder (paste-key) QR - fine for reference, not a live tunnel.
    pub(super) fn show_qr(&mut self, m: &Manifest, node_name: String) {
        let Some(node) = m.nodes.iter().find(|n| n.name == node_name) else {
            self.set_status("node vanished");
            return;
        };
        let config = m.node_config(node);
        self.show_qr_config(&node_name, &config);
    }

    /// Pop `config` as a QR overlay. Refuses a keyless config - a QR of a
    /// `<PASTE PRIVATE KEY>` placeholder imports a broken tunnel, so there's no
    /// point showing it; rotate to mint a fresh key instead.
    pub(super) fn show_qr_config(&mut self, node_name: &str, config: &str) {
        if config.contains("<PASTE PRIVATE KEY>") {
            self.set_status(format!(
                "`{node_name}` has no private key - rotate (R) to re-onboard"
            ));
            return;
        }
        // Compact + lowest error-correction = fewest modules = smallest QR.
        let compact = compact_for_qr(config);
        match QrCode::with_error_correction_level(compact.as_bytes(), qrcode::EcLevel::L) {
            Ok(code) => {
                let width = code.width();
                let dark = code
                    .to_colors()
                    .into_iter()
                    .map(|c| c == qrcode::Color::Dark)
                    .collect();
                self.overlay = Some(Overlay::Qr {
                    title: format!(" scan `{node_name}` "),
                    width,
                    dark,
                });
            }
            Err(e) => self.set_status(format!("QR failed: {e}")),
        }
    }

    /// Config for a fresh QR: the node's public-only config with `private` spliced
    /// into the `[Interface]` (so the scan imports a working tunnel).
    pub(super) fn working_config(m: &Manifest, node: &Node, private: &str) -> String {
        m.node_config(node).replace("<PASTE PRIVATE KEY>", private)
    }

    pub(super) fn gen_all(&self) -> Result<usize> {
        let m = Manifest::load(&self.manifest_path)?;
        std::fs::create_dir_all("out")?;
        for node in &m.nodes {
            std::fs::write(format!("out/{}.conf", node.name), m.node_config(node))?;
        }
        Ok(m.nodes.len())
    }

    // --- wizard -----------------------------------------------------------

    /// Regenerate the selected node's keypair (public-only): update its pubkey,
    /// hand out the new key via QR + out/<name>.conf. The pubkey CHANGES, so its
    /// hub/Ansible must be updated to keep accepting it.
    pub(super) fn rotate_selected(&mut self) {
        let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
            self.set_status("no node selected");
            return;
        };
        let mut m = match Manifest::load_or_empty(&self.manifest_path) {
            Ok(m) => m,
            Err(e) => {
                self.set_status(format!("manifest: {e}"));
                return;
            }
        };
        let private = keys::gen_private();
        let public = match keys::public_from_private(&private) {
            Ok(p) => p,
            Err(e) => {
                self.set_status(format!("keygen failed: {e}"));
                return;
            }
        };
        let Some(node) = m.nodes.iter_mut().find(|n| n.name == name) else {
            self.set_status("node vanished");
            return;
        };
        let was_stored = node.private_key.is_some();
        node.public_key = public;
        node.private_key = was_stored.then(|| private.clone()); // preserve stored/redacted choice
        if let Err(e) = m.save(&self.manifest_path) {
            self.set_status(format!("save failed: {e}"));
            return;
        }
        let node_ref = m.nodes.iter().find(|n| n.name == name).unwrap();
        let cfg = Self::working_config(&m, node_ref, &private);
        self.show_qr_config(&name, &cfg);
        let where_ = write_conf(&name, &cfg);
        self.nodes = m.nodes;
        self.set_status(format!(
            "rotated `{name}` - NEW pubkey, update its hub/Ansible; QR or {where_}"
        ));
    }

    /// Open the Export menu for the selected node.
    pub(super) fn open_export(&mut self) {
        let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
            self.set_status("no node selected");
            return;
        };
        let items = vec![
            ("write out/<name>.conf".to_string(), ExportKind::Conf),
            ("install to /etc/wireguard".to_string(), ExportKind::Install),
            ("QR code".to_string(), ExportKind::Qr),
            ("Ansible peer entry".to_string(), ExportKind::Ansible),
        ];
        self.overlay = Some(Overlay::Menu {
            title: format!(" export `{name}` "),
            name,
            items,
            idx: 0,
        });
    }

    /// Carry out an export for `name`. `.conf`/install/QR need the node's config;
    /// it carries the private key only if the node stores one (else placeholder).
    pub(super) fn export(&mut self, name: &str, kind: ExportKind) {
        let m = match Manifest::load_or_empty(&self.manifest_path) {
            Ok(m) => m,
            Err(e) => {
                self.set_status(format!("manifest: {e}"));
                return;
            }
        };
        let Some(node) = m.nodes.iter().find(|n| n.name == name) else {
            self.set_status("node vanished");
            return;
        };
        let cfg = m.node_config(node);
        let redacted = cfg.contains("<PASTE PRIVATE KEY>");
        match kind {
            ExportKind::Conf => {
                let where_ = write_conf(name, &cfg);
                self.set_status(if redacted {
                    format!("wrote {where_} (NO private key - redacted node; rotate to reissue)")
                } else {
                    format!("wrote {where_}")
                });
            }
            ExportKind::Install => {
                if redacted {
                    self.set_status("redacted node has no private key - rotate first");
                    return;
                }
                let path = format!("/etc/wireguard/{name}.conf");
                match std::fs::write(&path, &cfg) {
                    Ok(()) => self.set_status(format!("installed {path} (wg-quick up {name})")),
                    Err(e) => self.set_status(format!("can't write {path}: {e} (need sudo ewg?)")),
                }
            }
            ExportKind::Qr => self.show_qr_config(name, &cfg),
            ExportKind::Ansible => {
                let ip = node.mesh_ip().rsplit('.').next().unwrap_or("?");
                let line = format!(
                    "  - {{ name: {}, ip: {}, public_key: \"{}\" }}",
                    node.name, ip, node.public_key
                );
                self.overlay = Some(Overlay::Text {
                    title: " Ansible peer entry ".into(),
                    body: line,
                    scroll: 0,
                });
            }
        }
    }
}
