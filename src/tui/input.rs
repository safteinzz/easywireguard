//! Key dispatch: the tab-wide keys, then whichever view owns the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

use crate::manifest::Manifest;

use super::overlay::ConfirmAction;
use super::overlay::ExportKind;
use super::*;

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        if self.overlay.is_some() {
            // Text pager: j/k scroll, else close. QR: any key closes. Confirm: y deletes.
            let mut close = false;
            let mut confirm: Option<ConfirmAction> = None;
            let mut do_export: Option<(String, ExportKind)> = None;
            let mut yank: Option<String> = None;
            let mut reopen: Option<(String, Option<PathBuf>, bool)> = None;
            let mut discarded = false;
            match self.overlay.as_mut().unwrap() {
                Overlay::Text { scroll, body, .. } => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::Char('y') => yank = Some(body.clone()),
                    _ => close = true,
                },
                Overlay::Qr { .. } => close = true,
                Overlay::Confirm { action, .. } => {
                    if matches!(
                        key.code,
                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
                    ) {
                        confirm = Some(action.clone());
                    }
                    close = true; // any key dismisses the confirm
                }
                Overlay::Menu {
                    name, items, idx, ..
                } => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => *idx = (*idx + 1) % items.len(),
                    KeyCode::Up | KeyCode::Char('k') => {
                        *idx = (*idx + items.len() - 1) % items.len()
                    }
                    KeyCode::Enter => {
                        do_export = Some((name.clone(), items[*idx].1));
                        close = true;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => close = true,
                    _ => {}
                },
                Overlay::Invalid {
                    content,
                    original,
                    was_up,
                    ..
                } => {
                    match key.code {
                        KeyCode::Char('e') | KeyCode::Char('c') | KeyCode::Enter => {
                            reopen = Some((content.clone(), original.clone(), *was_up))
                        }
                        _ => discarded = true, // d / esc / any other key throws it away
                    }
                    close = true;
                }
            }
            if discarded {
                self.set_status("discarded");
            }
            if let Some((content, original, was_up)) = reopen {
                self.overlay = None;
                self.reopen_editor(content, original, was_up);
                return;
            }
            if let Some(text) = yank {
                // keep the box open so "copied" shows while it's still on screen
                match copy_clipboard(&text) {
                    Some(tool) => self.set_status(format!("copied to clipboard ({tool})")),
                    None => self.set_status("no clipboard tool - install wl-clipboard or xclip"),
                }
            } else if let Some(action) = confirm {
                self.overlay = None;
                match action {
                    ConfirmAction::DeleteNode(name) => self.delete_node(&name),
                    ConfirmAction::DeleteIface(path) => self.delete_iface(path),
                }
            } else if let Some((name, kind)) = do_export {
                self.overlay = None;
                self.export(&name, kind);
            } else if close {
                self.overlay = None;
            }
            return;
        }
        if self.prompt.is_some() {
            self.prompt_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.cycle_view(1),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.cycle_view(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            _ => match self.view {
                View::Interfaces => self.interfaces_key(key),
                View::Mesh => self.mesh_key(key),
            },
        }
    }

    pub(super) fn interfaces_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let sel = self.selected_iface();
                let (path, up) = match sel {
                    Some(i) => (Some(i.path.clone()), !i.up),
                    None => (None, false),
                };
                let m = act(path, up);
                self.reload(m);
            }
            KeyCode::Char('c') => self.start_create_conf(),
            KeyCode::Char('e') => self.start_edit_conf(),
            KeyCode::Char('d') => self.confirm_delete_iface(),
            KeyCode::Char('b') => self.toggle_boot(),
            KeyCode::Char('i') => self.inspect_iface(),
            KeyCode::Char('r') => self.reload("refreshed"),
            _ => {}
        }
    }

    pub(super) fn mesh_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') => {
                self.prompt = Some(Prompt::create_node(
                    NodeKind::Spoke,
                    KeySource::Generate,
                    &self.next_address(),
                    &self.hub_names(),
                ))
            }
            KeyCode::Char('e') => {
                let hubs = self.hub_names();
                let Some(node) = self.selected_mesh_node() else {
                    self.set_status("no node selected");
                    return;
                };
                self.prompt = Some(Prompt::edit_node(node, &hubs));
            }
            KeyCode::Char('R') => self.rotate_selected(),
            KeyCode::Char('E') => self.open_export(),
            KeyCode::Char('i') => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_status("no node selected");
                    return;
                };
                match Manifest::load_or_empty(&self.manifest_path) {
                    Ok(m) => match m.nodes.iter().find(|n| n.name == name) {
                        Some(node) => {
                            self.overlay = Some(Overlay::Text {
                                title: format!(" {name}.conf (generated) "),
                                body: m.node_config(node),
                                scroll: 0,
                            })
                        }
                        None => self.set_status("node vanished"),
                    },
                    Err(e) => self.set_status(format!("couldn't load manifest: {e}")),
                }
            }
            KeyCode::Char('d') => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_status("no node selected");
                    return;
                };
                self.overlay = Some(Overlay::Confirm {
                    prompt: format!(" delete `{name}` from mesh.toml?  y / n "),
                    action: ConfirmAction::DeleteNode(name),
                });
            }
            KeyCode::Enter => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_status("no node selected");
                    return;
                };
                match Manifest::load_or_empty(&self.manifest_path) {
                    Ok(m) => self.show_qr(&m, name),
                    Err(e) => self.set_status(format!("couldn't load manifest: {e}")),
                }
            }
            KeyCode::Char('g') => match self.gen_all() {
                Ok(n) => self.set_status(format!("wrote {n} configs to ./out")),
                Err(e) => self.set_status(format!("gen failed: {e}")),
            },
            KeyCode::Char('r') => self.reload("reloaded manifest"),
            _ => {}
        }
    }

    pub(super) fn prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.prompt.as_ref().map(|p| p.fields.len()).unwrap_or(0);
        if len == 0 {
            return;
        }
        // Choice rows (Type / Key / hub picker) are *cycled*, never typed into, so
        // both ←/→ and h/l work there - with or without ctrl, since ctrl-h/l is the
        // chord for "choose" (fields move with ctrl-j/k, so h/l stays free).
        let on_choice = self
            .prompt
            .as_ref()
            .map(|p| p.fields[p.idx].is_choice())
            .unwrap_or(false);
        if on_choice {
            let delta = match key.code {
                KeyCode::Right | KeyCode::Char('l') => 1,
                KeyCode::Left | KeyCode::Char('h') => -1,
                _ => 0,
            };
            if delta != 0 {
                let p = self.prompt.as_ref().unwrap();
                let cur_kind = matches!(p.fields[p.idx].kind, FieldKind::Type(_));
                let cur_key = matches!(p.fields[p.idx].kind, FieldKind::Key(_));
                let (addr, hubs) = (self.next_address(), self.hub_names());
                let p = self.prompt.as_mut().unwrap();
                if cur_kind {
                    p.toggle_kind(&addr, &hubs);
                } else if cur_key {
                    p.toggle_keysrc(&addr, &hubs);
                } else {
                    p.cycle_pick(delta);
                }
                return;
            }
        }
        // Field navigation: ↑↓/Tab, or ctrl-j/k (plain letters are typed into text
        // fields, so the vertical chord needs ctrl; ctrl-h/l is "choose", above).
        let next = matches!(key.code, KeyCode::Tab | KeyCode::Down)
            || (ctrl && key.code == KeyCode::Char('j'));
        let prev = matches!(key.code, KeyCode::BackTab | KeyCode::Up)
            || (ctrl && key.code == KeyCode::Char('k'));
        if next {
            let p = self.prompt.as_mut().unwrap();
            p.idx = (p.idx + 1) % len;
            return;
        }
        if prev {
            let p = self.prompt.as_mut().unwrap();
            p.idx = (p.idx + len - 1) % len;
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.set_status("cancelled");
            }
            KeyCode::Char('c') if ctrl => {
                self.prompt = None;
                self.set_status("cancelled");
            }
            KeyCode::Enter => {
                let idx = self.prompt.as_ref().unwrap().idx;
                if idx + 1 < len {
                    self.prompt.as_mut().unwrap().idx += 1;
                } else {
                    self.submit_prompt();
                }
            }
            KeyCode::Backspace => {
                self.prompt.as_mut().unwrap().cur_mut().value.pop();
            }
            KeyCode::Char(c) if !ctrl => {
                self.prompt.as_mut().unwrap().cur_mut().value.push(c);
            }
            _ => {}
        }
    }
}
