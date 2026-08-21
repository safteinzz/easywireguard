//! Submitting a prompt: what each wizard writes when the last field is done.

use std::path::PathBuf;

use crate::manifest::{Manifest, Node};
use crate::{keys, wg};

use std::path::Path;

use super::edit::{backup, stem, write_conf};
use super::*;

impl App {
    /// Consume the wizard and carry out its action. On a validation error it puts
    /// the prompt back so the field can be fixed.
    pub(super) fn submit_prompt(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        match prompt.action {
            Action::AddNode | Action::EditNode { .. } => self.submit_node(prompt),
            Action::SaveConf { .. } => self.submit_conf(prompt),
        }
    }

    /// Write the edited interface config to `<name>.conf`. Backs up any file it
    /// overwrites; on a rename it also backs up and removes the old file, but
    /// refuses to rename a live interface (the running one would strand under its
    /// old name). Validation errors put the prompt back so the name can be fixed.
    pub(super) fn submit_conf(&mut self, prompt: Prompt) {
        let (content, original, was_up) = match &prompt.action {
            Action::SaveConf {
                content,
                original,
                was_up,
            } => (content.clone(), original.clone(), *was_up),
            _ => return,
        };
        let name = prompt.value_of("Name");
        if let Err(e) = wg::valid_iface_name(&name) {
            self.set_status(e.to_string());
            self.prompt = Some(prompt);
            return;
        }
        // Target dir: the picker if present, else the edited file's dir, else the
        // first registered dir.
        let dir: PathBuf = if prompt
            .fields
            .iter()
            .any(|f| f.label.starts_with("Directory"))
        {
            PathBuf::from(prompt.value_of("Directory"))
        } else if let Some(orig) = &original {
            orig.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            self.dirs
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."))
        };
        let target = dir.join(format!("{name}.conf"));

        let renaming = original.as_ref().is_some_and(|o| *o != target);
        if renaming && was_up {
            self.set_status(format!(
                "take `{}` down (↵) before renaming it",
                stem(original.as_ref().unwrap())
            ));
            self.prompt = Some(prompt);
            return;
        }
        backup(&target); // keep any file we're about to clobber
        if let Err(e) = std::fs::write(&target, &content) {
            self.set_status(format!(
                "can't write {}: {e} (need sudo ewg?)",
                target.display()
            ));
            self.prompt = Some(prompt);
            return;
        }
        if renaming && let Some(orig) = &original {
            backup(orig);
            let _ = std::fs::remove_file(orig);
        }
        let verb = if original.is_some() {
            "saved"
        } else {
            "created"
        };
        let tail = if was_up && !renaming {
            " - toggle (↵) to apply"
        } else {
            ""
        };
        self.reload(format!("{verb} {}{tail}", target.display()));
        self.select_iface(&name);
    }

    /// Turn a completed create/edit wizard into a manifest entry. The manifest is
    /// PUBLIC-ONLY (never stores a private key); a freshly generated key is handed
    /// out once via the QR + an out/<name>.conf, then discarded. On a validation
    /// error the prompt is put back so the field can be fixed.
    pub(super) fn submit_node(&mut self, prompt: Prompt) {
        let original = match &prompt.action {
            Action::EditNode { original } => Some(original.clone()),
            Action::AddNode | Action::SaveConf { .. } => None,
        };
        let editing = original.is_some();
        // Read every field up front (owned Strings) so `prompt` is free to move.
        let kind = prompt.kind();
        let keysrc = prompt.keysrc();
        let name = prompt.value_of("Name");
        let address = prompt.value_of("Address");
        let dns = prompt.value_of("DNS");
        let endpoint = prompt.value_of("Endpoint");
        let allowed = prompt.value_of("Allowed-IPs");
        let keepalive = prompt.value_of("Keepalive");
        let pasted_pubkey = prompt.value_of("Public key");
        let hub = prompt.value_of("Hub to dial");
        let redact = prompt.value_of("Private key").starts_with("redact");

        if name.is_empty() {
            self.set_status("a node name is required");
            self.prompt = Some(prompt);
            return;
        }
        if kind == NodeKind::Hub && endpoint.is_empty() {
            self.set_status("a hub needs an endpoint (host:port peers dial)");
            self.prompt = Some(prompt);
            return;
        }

        let mut m = match Manifest::load_or_empty(&self.manifest_path) {
            Ok(m) => m,
            Err(e) => {
                self.set_status(format!("manifest: {e}"));
                return;
            }
        };

        // Key material, three outputs:
        //   public_key     - goes in the node
        //   stored_private - written to the node (None = not kept: pasted, or redacted)
        //   fresh_private  - available NOW for QR/file delivery (only a new generate)
        // Editing -> keep the node's key untouched. Paste -> reference the pubkey.
        // Generate -> new keypair, stored unless redacted.
        let existing = original
            .as_ref()
            .and_then(|o| m.nodes.iter().find(|n| &n.name == o));
        let existing_pub = existing.map(|n| n.public_key.clone());
        let existing_priv = existing.and_then(|n| n.private_key.clone());
        let (public_key, stored_private, fresh_private): (String, Option<String>, Option<String>) =
            if editing {
                match existing_pub {
                    Some(p) => (p, existing_priv, None),
                    None => {
                        self.set_status("original node vanished");
                        return;
                    }
                }
            } else if keysrc == KeySource::Paste {
                if pasted_pubkey.is_empty() {
                    self.set_status("paste a public key, or switch Key to Generate");
                    self.prompt = Some(prompt);
                    return;
                }
                (pasted_pubkey, None, None)
            } else {
                let private = keys::gen_private();
                match keys::public_from_private(&private) {
                    Ok(public) => {
                        let stored = if redact { None } else { Some(private.clone()) };
                        (public, stored, Some(private))
                    }
                    Err(e) => {
                        self.set_status(format!("keygen failed: {e}"));
                        self.prompt = Some(prompt);
                        return;
                    }
                }
            };

        let node = Node {
            name: name.clone(),
            address,
            public_key,
            endpoint: (kind == NodeKind::Hub && !endpoint.is_empty()).then_some(endpoint),
            allowed_ips: (kind == NodeKind::Hub && !allowed.is_empty()).then_some(allowed),
            dns: (kind == NodeKind::Spoke && !dns.is_empty()).then_some(dns),
            keepalive: (kind == NodeKind::Hub)
                .then(|| keepalive.parse().ok())
                .flatten(),
            hubs: if kind == NodeKind::Spoke && !hub.is_empty() {
                vec![hub]
            } else {
                Vec::new()
            },
            private_key: stored_private,
            post_up: None,
            post_down: None,
        };

        // On edit, drop the original first so the re-add doesn't collide with itself.
        if let Some(o) = &original {
            let _ = m.remove(o);
        }
        if let Err(e) = m.add(node) {
            self.set_status(format!(
                "{} failed: {e}",
                if editing { "edit" } else { "add" }
            ));
            self.prompt = Some(prompt);
            return;
        }
        if let Err(e) = m.save(&self.manifest_path) {
            self.set_status(format!("save failed: {e}"));
            return;
        }

        // Deliver: a fresh key -> working QR + out/<name>.conf; otherwise a
        // placeholder QR (new referenced hub) or nothing to hand out (edit).
        if let Some(private) = &fresh_private {
            let node_ref = m.nodes.iter().find(|n| n.name == name).unwrap();
            let cfg = Self::working_config(&m, node_ref, private);
            self.show_qr_config(&name, &cfg);
            let where_ = write_conf(&name, &cfg);
            let how = if redact {
                "redacted, key in QR/file only"
            } else {
                "key stored"
            };
            self.set_status(format!(
                "added {} `{name}` ({how}) - QR or {where_}",
                kind.short()
            ));
        } else if editing {
            self.set_status(format!(
                "edited `{name}` - rotate (R) to re-onboard if its config changed"
            ));
        } else {
            self.show_qr(&m, name.clone());
            self.set_status(format!("added {} `{name}`", kind.short()));
        }
        self.nodes = m.nodes; // move (Node isn't Clone); m is done being borrowed
        Self::clamp(&mut self.node_state, self.nodes.len());
    }
}
