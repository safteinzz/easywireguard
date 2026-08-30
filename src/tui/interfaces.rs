//! The Interfaces tab: creating, editing, deleting and toggling a `.conf`.

use std::path::PathBuf;

use crate::wg;

use super::edit::{backup, stem, write_temp};
use super::overlay::ConfirmAction;
use super::*;

impl App {
    /// The interface selected in the list, if any.
    pub(super) fn selected_iface(&self) -> Option<&Iface> {
        self.iface_state.selected().and_then(|i| self.ifaces.get(i))
    }

    /// Re-select the interface named `name` after a reload (best effort).
    pub(super) fn select_iface(&mut self, name: &str) {
        if let Some(i) = self.ifaces.iter().position(|f| f.name == name) {
            self.iface_state.select(Some(i));
        }
    }

    /// Suggest the next free `wgN` interface name from those already present.
    pub(super) fn next_iface_name(&self) -> String {
        let used: std::collections::BTreeSet<&str> =
            self.ifaces.iter().map(|i| i.name.as_str()).collect();
        (0..=254)
            .map(|n| format!("wg{n}"))
            .find(|c| !used.contains(c.as_str()))
            .unwrap_or_else(|| "wg0".into())
    }

    /// Start creating an interface: seed a temp file with a skeleton and ask the
    /// event loop to open `$EDITOR` on it. On save we prompt for the name.
    pub(super) fn start_create_conf(&mut self) {
        const SKELETON: &str = "[Interface]\n# Paste your provider's config over this, or fill it in.\nPrivateKey = \nAddress = \n# DNS = 1.1.1.1\n\n[Peer]\nPublicKey = \n# PresharedKey = \nEndpoint = \nAllowedIPs = 0.0.0.0/0\n";
        match write_temp(SKELETON) {
            Ok(tmp) => {
                self.pending_editor = Some(EditorReq {
                    tmp,
                    original: None,
                    was_up: false,
                    seed: SKELETON.to_string(),
                })
            }
            Err(e) => self.set_status(format!("can't open an editor buffer: {e}")),
        }
    }

    /// Start editing the selected interface: seed the temp file with its current
    /// contents and open `$EDITOR`. On save we prompt for the name (rename-aware).
    pub(super) fn start_edit_conf(&mut self) {
        let Some(iface) = self.selected_iface().cloned() else {
            self.set_status("no interface selected");
            return;
        };
        let content = match std::fs::read_to_string(&iface.path) {
            Ok(t) => t,
            Err(e) => {
                self.set_status(format!("can't read {}: {e}", iface.path.display()));
                return;
            }
        };
        match write_temp(&content) {
            Ok(tmp) => {
                self.pending_editor = Some(EditorReq {
                    tmp,
                    original: Some(iface.path.clone()),
                    was_up: iface.up,
                    seed: content,
                })
            }
            Err(e) => self.set_status(format!("can't open an editor buffer: {e}")),
        }
    }

    /// Called by the event loop once `$EDITOR` has exited. Saving nothing (buffer
    /// unchanged from the seed, or emptied) cancels. Otherwise the config is
    /// validated: a valid one goes to the name prompt; an invalid one pops a dialog
    /// naming the problem, so the user decides whether to fix it or throw it away.
    pub(super) fn editor_done(&mut self, req: EditorReq) {
        let content = std::fs::read_to_string(&req.tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&req.tmp);
        if content == req.seed || content.trim().is_empty() {
            self.set_status(if req.original.is_some() {
                "no changes - nothing saved"
            } else {
                "nothing saved"
            });
            return;
        }
        match wg::validate_config(&content) {
            Ok(()) => self.open_conf_name_prompt(content, req.original, req.was_up),
            Err(e) => {
                self.overlay = Some(Overlay::Invalid {
                    reason: e.to_string(),
                    content,
                    original: req.original,
                    was_up: req.was_up,
                })
            }
        }
    }

    /// Reopen `$EDITOR` on `content` (the "correct" choice from the invalid dialog).
    pub(super) fn reopen_editor(
        &mut self,
        content: String,
        original: Option<PathBuf>,
        was_up: bool,
    ) {
        match write_temp(&content) {
            Ok(tmp) => {
                self.pending_editor = Some(EditorReq {
                    tmp,
                    original,
                    was_up,
                    seed: content,
                })
            }
            Err(e) => self.set_status(format!("can't reopen editor: {e}")),
        }
    }

    /// The name prompt shown after the editor: a Name field prefilled with the
    /// existing name (edit) or the next free `wgN` (create), plus a Directory
    /// picker when creating with more than one registered dir.
    pub(super) fn open_conf_name_prompt(
        &mut self,
        content: String,
        original: Option<PathBuf>,
        was_up: bool,
    ) {
        let default_name = original
            .as_deref()
            .map(stem)
            .unwrap_or_else(|| self.next_iface_name());
        let mut name = Field::new("Name (.conf interface name)", "");
        name.value = default_name;
        let mut fields = vec![name];
        if original.is_none() && self.dirs.len() > 1 {
            let opts: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            fields.push(Field::pick("Directory", opts));
        }
        let title = if original.is_some() {
            "Save interface".to_string()
        } else {
            "Name the interface".to_string()
        };
        self.prompt = Some(Prompt {
            title,
            fields,
            idx: 0,
            action: Action::SaveConf {
                content,
                original,
                was_up,
            },
        });
    }

    /// Confirm-gate deleting the selected interface. Refuses while it is up: the
    /// running interface must be brought down (its `.conf` is what downs it) first.
    pub(super) fn confirm_delete_iface(&mut self) {
        let Some(iface) = self.selected_iface() else {
            self.set_status("no interface selected");
            return;
        };
        if iface.up {
            self.set_status(format!(
                "`{}` is up - toggle it down (↵) before deleting",
                iface.name
            ));
            return;
        }
        let name = iface.name.clone();
        let path = iface.path.clone();
        self.overlay = Some(Overlay::Confirm {
            prompt: format!("delete `{name}.conf`? a .bak is kept."),
            action: ConfirmAction::DeleteIface(path),
            yes: false,
        });
    }

    /// Back up then delete an interface `.conf`, and refresh the list.
    pub(super) fn delete_iface(&mut self, path: PathBuf) {
        backup(&path);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                let name = stem(&path);
                self.reload(format!("deleted `{name}.conf` (.bak kept)"));
            }
            Err(e) => self.set_status(format!(
                "can't delete {}: {e} (need sudo ewg?)",
                path.display()
            )),
        }
    }

    /// Toggle whether the selected interface starts on boot (systemd).
    pub(super) fn toggle_boot(&mut self) {
        let Some(iface) = self.selected_iface().cloned() else {
            self.set_status("no interface selected");
            return;
        };
        match iface.enabled {
            None => self.set_status("can't manage boot state - systemd/systemctl not available"),
            Some(enabled) => match wg::set_boot(&iface.name, !enabled) {
                Ok(()) => {
                    let name = iface.name.clone();
                    self.reload(format!(
                        "`{name}` will {} start on boot",
                        if enabled { "no longer" } else { "now" }
                    ));
                    self.select_iface(&name);
                }
                Err(e) => self.set_status(format!("{e} (need sudo ewg?)")),
            },
        }
    }

    /// Inspect the selected interface: its `.conf`, plus a live `wg show` readout
    /// appended when the interface is up (handshakes, transfer, peers).
    pub(super) fn inspect_iface(&mut self) {
        let Some(iface) = self.selected_iface().cloned() else {
            self.set_status("no interface selected");
            return;
        };
        let mut body = match std::fs::read_to_string(&iface.path) {
            Ok(t) => t,
            Err(e) => {
                self.set_status(format!("can't read {}: {e}", iface.path.display()));
                return;
            }
        };
        if iface.up
            && let Ok(live) = wg::show(&iface.name)
            && !live.trim().is_empty()
        {
            body.push_str("\n# ---- live (wg show) ----\n");
            body.push_str(&live);
        }
        self.overlay = Some(Overlay::Text {
            title: format!(" {} ", tilde(&iface.path)),
            body,
            scroll: 0,
        });
    }
}
