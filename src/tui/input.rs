//! Key dispatch: help and the boxes first, then the form, the `/` filter, the
//! app-wide keys, and whichever tab owns the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

use crate::manifest::Manifest;

use super::overlay::ConfirmAction;
use super::overlay::ExportKind;
use super::*;

/// Ctrl-C, which does what Esc does under a box or in a form and quits from a view.
pub(super) fn is_ctrl_c(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        if self.show_help {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let half = 10;
            match key.code {
                _ if is_ctrl_c(key) => self.show_help = false,
                KeyCode::Char('?' | 'q') | KeyCode::Esc => self.show_help = false,
                KeyCode::Char('d') if ctrl => {
                    self.help_scroll = self.help_scroll.saturating_add(half)
                }
                KeyCode::Char('u') if ctrl => {
                    self.help_scroll = self.help_scroll.saturating_sub(half)
                }
                KeyCode::PageDown => self.help_scroll = self.help_scroll.saturating_add(half),
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(half),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1)
                }
                KeyCode::Char('g') | KeyCode::Home => self.help_scroll = 0,
                // `render_help` clamps this to the last screenful.
                KeyCode::Char('G') | KeyCode::End => self.help_scroll = usize::MAX,
                _ => {}
            }
            return;
        }
        if self.overlay.is_some() {
            // Under a box Ctrl-C is Esc, so a reflex Ctrl-C steps out one box.
            let key = if is_ctrl_c(key) {
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
            } else {
                key
            };
            self.overlay_key(key);
            return;
        }
        if self.prompt.is_some() {
            self.prompt_key(key);
            return;
        }
        if self.searching {
            self.search_key(key);
            return;
        }
        self.nav_key(key);
    }

    /// Keys while a box is up. A key that means nothing to the box is
    /// swallowed, so a stray press can neither dismiss nor answer it.
    fn overlay_key(&mut self, key: KeyEvent) {
        let mut close = false;
        let mut confirm: Option<ConfirmAction> = None;
        let mut do_export: Option<(String, ExportKind)> = None;
        let mut yank: Option<String> = None;
        let mut reopen: Option<(String, Option<PathBuf>, bool)> = None;
        let mut discarded = false;
        let mut cancelled = false;
        match self.overlay.as_mut().unwrap() {
            Overlay::Text { scroll, body, .. } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('y') => yank = Some(body.clone()),
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | ' ') => close = true,
                _ => {}
            },
            Overlay::Qr { .. } => {
                close = matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | ' ')
                )
            }
            Overlay::Confirm { action, yes, .. } => match key.code {
                KeyCode::Char('y' | 'Y') => {
                    confirm = Some(action.clone());
                    close = true;
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    cancelled = true;
                    close = true;
                }
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Char('h' | 'l')
                | KeyCode::Tab
                | KeyCode::BackTab => *yes = !*yes,
                KeyCode::Enter => {
                    if *yes {
                        confirm = Some(action.clone());
                    } else {
                        cancelled = true;
                    }
                    close = true;
                }
                _ => {}
            },
            Overlay::Typed {
                name,
                input,
                back,
                action,
                ..
            } => match key.code {
                KeyCode::Esc => {
                    cancelled = true;
                    close = true;
                }
                KeyCode::Enter if input == name => {
                    confirm = Some(action.clone());
                    close = true;
                }
                KeyCode::Enter => {}
                _ => {
                    line_edit::edit(input, back, key);
                }
            },
            Overlay::Menu {
                name, items, idx, ..
            } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => *idx = (*idx + 1) % items.len(),
                KeyCode::Up | KeyCode::Char('k') => *idx = (*idx + items.len() - 1) % items.len(),
                KeyCode::Enter => {
                    do_export = Some((name.clone(), items[*idx].1));
                    close = true;
                }
                KeyCode::Esc => {
                    cancelled = true;
                    close = true;
                }
                _ => {}
            },
            Overlay::Invalid {
                content,
                original,
                was_up,
                yes,
                ..
            } => {
                let correct = match key.code {
                    KeyCode::Char('y' | 'Y') => Some(true),
                    KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(false),
                    KeyCode::Enter => Some(*yes),
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Char('h' | 'l')
                    | KeyCode::Tab
                    | KeyCode::BackTab => {
                        *yes = !*yes;
                        None
                    }
                    _ => None,
                };
                match correct {
                    Some(true) => reopen = Some((content.clone(), original.clone(), *was_up)),
                    Some(false) => discarded = true,
                    None => {}
                }
                close = correct.is_some();
            }
        }
        if discarded {
            self.set_status("discarded");
        }
        if cancelled {
            self.set_status("cancelled");
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
                None => self.set_failed("no clipboard tool: install wl-clipboard or xclip"),
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
    }

    /// Typing a `/` filter. Everything printable goes into the query, and the
    /// list keeps updating under it while the arrows still move, so you can
    /// type, then act on what is left.
    fn search_key(&mut self, key: KeyEvent) {
        // Esc drops the filter, and Ctrl-C with it, so a reflex Ctrl-C steps
        // out of the query before it can quit; Enter keeps it.
        if key.code == KeyCode::Esc || is_ctrl_c(key) {
            self.query.clear();
            self.query_back = 0;
            self.searching = false;
            self.requery();
            return;
        }
        match key.code {
            KeyCode::Enter => self.searching = false,
            KeyCode::Down => self.move_sel(1),
            KeyCode::Up => self.move_sel(-1),
            _ => {
                if line_edit::edit(&mut self.query, &mut self.query_back, key) {
                    self.requery();
                }
            }
        }
    }

    /// Keys on a tab: the app-wide ones, then whichever tab owns the rest.
    fn nav_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if is_ctrl_c(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') if !ctrl => self.should_quit = true,
            KeyCode::Char('?') if !ctrl => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('/') if !ctrl => {
                self.query.clear();
                self.query_back = 0;
                self.searching = true;
                self.requery();
            }
            // Outside a search, Esc's only job is to undo one.
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.query_back = 0;
                self.requery();
                self.set_status("filter cleared");
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.cycle_view(1),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.cycle_view(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            // A leftover Ctrl-chord is never an action, so Ctrl-d cannot delete.
            _ if ctrl => {}
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
                    self.set_failed("no node selected");
                    return;
                };
                self.prompt = Some(Prompt::edit_node(node, &hubs));
            }
            KeyCode::Char('R') => self.rotate_selected(),
            KeyCode::Char('E') => self.open_export(),
            KeyCode::Char('i') => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_failed("no node selected");
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
                    Err(e) => self.set_failed(format!("couldn't load manifest: {e}")),
                }
            }
            KeyCode::Char('d') => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_failed("no node selected");
                    return;
                };
                // mesh.toml keeps no backup, and a stored private key is in
                // no other file, so the delete takes the node's typed name.
                self.overlay = Some(Overlay::Typed {
                    title: "delete node".into(),
                    message: format!(
                        "delete `{name}` from mesh.toml? No backup is kept, and a private key stored there exists nowhere else."
                    ),
                    name: name.clone(),
                    input: String::new(),
                    back: 0,
                    action: ConfirmAction::DeleteNode(name),
                });
            }
            KeyCode::Enter => {
                let Some(name) = self.selected_mesh_node().map(|n| n.name.clone()) else {
                    self.set_failed("no node selected");
                    return;
                };
                match Manifest::load_or_empty(&self.manifest_path) {
                    Ok(m) => self.show_qr(&m, name),
                    Err(e) => self.set_failed(format!("couldn't load manifest: {e}")),
                }
            }
            KeyCode::Char('g') => match self.gen_all() {
                Ok(n) => self.set_status(format!("wrote {n} configs to ./out")),
                Err(e) => self.set_failed(format!("gen failed: {e}")),
            },
            KeyCode::Char('r') => self.reload("refreshed"),
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
            _ if key.code == KeyCode::Esc || is_ctrl_c(key) => {
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
            _ if !on_choice => {
                let field = self.prompt.as_mut().unwrap().cur_mut();
                line_edit::edit(&mut field.value, &mut field.back, key);
            }
            _ => {}
        }
    }
}
