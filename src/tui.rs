//! The toolbox - bare `ewg`. Two tabs (Interfaces / Mesh), switched with
//! `h/l ←→` or Tab, navigated with `j/k ↑↓` like a normal list:
//!
//! - **Interfaces** - the `.conf` files across your registered dirs; `↵` toggles one
//!   up/down, `c` creates one in `$EDITOR` (paste a provider config), `e` edits, `d`
//!   deletes it (a `.bak` is kept), `b` toggles start-on-boot, `i` inspects it (with
//!   live `wg show` when up).
//! - **Mesh** - the nodes in a manifest, shown hub-and-spoke (spokes nested under
//!   their hub). `c` creates one (a Spoke/Hub wizard that generates keys and pops a
//!   QR to scan), `↵` shows a node's QR, `i` inspects its generated config, `d`
//!   deletes it.
//!
//! Modeled on `simplessh`'s tabbed layout: a bordered tab bar titled with the app
//! name, `Name (count)` bodies, a status line that fades after `STATUS_TTL`, and
//! centered overlays (a wizard, a QR, a confirm).

use anyhow::Result;
use qrcode::QrCode;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement},
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::keys;
use crate::manifest::{Manifest, Node};
use crate::wg::{self, Iface};

/// How long a status message stays before the hints return.
const STATUS_TTL: Duration = Duration::from_millis(1500);

// Movement key labels, split so each view states exactly the chord it accepts and
// they still read the same everywhere. `Y` = vertical, `X` = horizontal; `VIM_` is
// the letter form, plain is the arrow form. Lists take either (`VIM_Y_MOVE Y_MOVE`
// -> "j/k ↑↓"); the wizard needs the ctrl- letter form, since bare letters are
// typed into its fields, while its arrows work unmodified ("ctrl-j/k ↑↓").
const Y_MOVE: &str = "↑↓";
const X_MOVE: &str = "←→";
const VIM_Y_MOVE: &str = "j/k";
const VIM_X_MOVE: &str = "h/l";
const CTRL_VIM_Y_MOVE: &str = "ctrl-j/k";
const CTRL_VIM_X_MOVE: &str = "ctrl-h/l";

/// The tabs, in left-to-right / Tab-cycle order.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Interfaces,
    Mesh,
}

const VIEWS: [View; 2] = [View::Interfaces, View::Mesh];

impl View {
    fn title(self) -> &'static str {
        match self {
            View::Interfaces => "Interfaces",
            View::Mesh => "Mesh",
        }
    }
    fn hints(self) -> String {
        match self {
            View::Interfaces => format!("{VIM_Y_MOVE} {Y_MOVE} move · {VIM_X_MOVE} {X_MOVE} tab · ↵ toggle · c new · e edit · d del · b boot · i inspect · r refresh · q quit"),
            View::Mesh => format!("{VIM_Y_MOVE} {Y_MOVE} move · {VIM_X_MOVE} {X_MOVE} tab · c create · e edit · R rotate · d del · E export · ↵ QR · i view · g gen · r reload · q quit"),
        }
    }
}

/// A centered modal over the current tab: a QR to scan, or a scrollable text
/// pager (`Text`) showing a `.conf` — an interface's on disk, or a mesh node's
/// generated on the fly (even if it was never written out).
enum Overlay {
    Qr { title: String, width: usize, dark: Vec<bool> },
    Text { title: String, body: String, scroll: u16 },
    Confirm { prompt: String, action: ConfirmAction },
    Menu { title: String, name: String, items: Vec<(String, ExportKind)>, idx: usize },
    /// The saved buffer failed validation. Shows the reason and lets the user
    /// choose: correct (reopen the editor on the same content) or discard it.
    Invalid { reason: String, content: String, original: Option<PathBuf>, was_up: bool },
}

/// What a `y`/Enter on a Confirm overlay carries out.
#[derive(Clone)]
enum ConfirmAction {
    /// Remove a node from the mesh manifest.
    DeleteNode(String),
    /// Delete an interface `.conf` from disk (a `.bak` is kept first).
    DeleteIface(PathBuf),
}

/// Ways to export a node from the Export menu.
#[derive(Clone, Copy)]
enum ExportKind {
    Conf,
    Install,
    Qr,
    Ansible,
}

/// What the wizard is creating. A **Spoke** is a road-warrior ewg generates a key
/// for; it dials the hub(s) you pick. A **Hub** is reachable (has an endpoint) and
/// meshes with every other hub; its key may live elsewhere (referenced by pubkey).
#[derive(Clone, Copy, PartialEq)]
enum NodeKind {
    Spoke,
    Hub,
}

impl NodeKind {
    fn short(self) -> &'static str {
        match self {
            NodeKind::Spoke => "Spoke",
            NodeKind::Hub => "Hub",
        }
    }
    fn toggled(self) -> Self {
        match self {
            NodeKind::Spoke => NodeKind::Hub,
            NodeKind::Hub => NodeKind::Spoke,
        }
    }
}

/// Where a node's key comes from: ewg generates a keypair, or you paste an existing
/// public key (its private lives elsewhere - e.g. a router's, vaulted in Ansible).
#[derive(Clone, Copy, PartialEq)]
enum KeySource {
    Generate,
    Paste,
}

impl KeySource {
    fn short(self) -> &'static str {
        match self {
            KeySource::Generate => "Generate",
            KeySource::Paste => "Paste pubkey",
        }
    }
    fn toggled(self) -> Self {
        match self {
            KeySource::Generate => KeySource::Paste,
            KeySource::Paste => KeySource::Generate,
        }
    }
}

/// A `Text` field is typed into; a `Type`/`Key` toggle or a `Pick` is a choice
/// flipped with ←/→ (nothing to type there). The toggles rebuild the form (their
/// choice changes which fields follow); a `Pick` just steps its index.
enum FieldKind {
    Text,
    Type(NodeKind),
    Key(KeySource),
    Pick { options: Vec<String>, idx: usize },
}

/// One line in a wizard. `default` shows in brackets and is used when the field
/// is left blank on submit.
struct Field {
    label: String,
    default: String,
    value: String,
    kind: FieldKind,
}

impl Field {
    fn new(label: &str, default: &str) -> Self {
        Self { label: label.into(), default: default.into(), value: String::new(), kind: FieldKind::Text }
    }
    fn type_toggle(kind: NodeKind) -> Self {
        Self { label: "Type".into(), default: String::new(), value: String::new(), kind: FieldKind::Type(kind) }
    }
    fn key_toggle(src: KeySource) -> Self {
        Self { label: "Key".into(), default: String::new(), value: String::new(), kind: FieldKind::Key(src) }
    }
    fn pick(label: &str, options: Vec<String>) -> Self {
        Self { label: label.into(), default: String::new(), value: String::new(), kind: FieldKind::Pick { options, idx: 0 } }
    }
    /// A choice field (Type/Key toggle or Pick): ←/→ cycles it, letters don't type.
    fn is_choice(&self) -> bool {
        matches!(self.kind, FieldKind::Type(_) | FieldKind::Key(_) | FieldKind::Pick { .. })
    }
}

#[derive(Clone)]
enum Action {
    AddNode,
    EditNode { original: String },
    /// Write an edited interface config to `<name>.conf`. `content` is the buffer
    /// the user saved in `$EDITOR`; `original` is the file being edited (None on
    /// create); `was_up` guards a rename of a live interface.
    SaveConf { content: String, original: Option<PathBuf>, was_up: bool },
}

/// A pending request to suspend the TUI, run `$EDITOR` on a temp file, and resume.
/// Built when `c`/`e` is pressed; consumed by the event loop, which owns the
/// terminal, then handed back to `App::editor_done`.
struct EditorReq {
    tmp: PathBuf,
    original: Option<PathBuf>,
    was_up: bool,
    /// Exactly what we wrote to `tmp` before opening the editor. If the buffer
    /// comes back identical, the user saved nothing (`:q!`, or `:wq` on the
    /// untouched seed), so we treat it as a cancel - and it's the guaranteed way
    /// out of the fix-and-resave loop when a config keeps failing validation.
    seed: String,
}

/// A modal wizard: a titled stack of fields plus the action to run on submit.
struct Prompt {
    title: String,
    fields: Vec<Field>,
    idx: usize,
    action: Action,
}

impl Prompt {
    fn cur_mut(&mut self) -> &mut Field {
        &mut self.fields[self.idx]
    }

    /// The node kind (read off the Type toggle row).
    fn kind(&self) -> NodeKind {
        match self.fields.first().map(|f| &f.kind) {
            Some(FieldKind::Type(k)) => *k,
            _ => NodeKind::Spoke,
        }
    }

    /// The key source (read off the Key toggle row, if any; else Generate).
    fn keysrc(&self) -> KeySource {
        self.fields.iter().find_map(|f| if let FieldKind::Key(k) = &f.kind { Some(*k) } else { None }).unwrap_or(KeySource::Generate)
    }

    /// Look up a field's value by label prefix: a Pick's selected option, or a
    /// Text field's typed value (falling back to its default when blank).
    fn value_of(&self, label_starts: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.label.starts_with(label_starts))
            .map(|f| match &f.kind {
                FieldKind::Pick { options, idx } => options.get(*idx).cloned().unwrap_or_default(),
                _ => {
                    let v = f.value.trim();
                    if v.is_empty() { f.default.clone() } else { v.to_string() }
                }
            })
            .unwrap_or_default()
    }

    /// The metadata fields (everything except key material) - shared by create/edit.
    fn base_fields(kind: NodeKind, default_addr: &str, hubs: &[String]) -> Vec<Field> {
        let mut fields = vec![Field::type_toggle(kind), Field::new("Name", ""), Field::new("Address (interface IP/prefix)", default_addr)];
        match kind {
            NodeKind::Spoke => {
                fields.push(Field::new("DNS (optional - e.g. a Pi-hole)", ""));
                fields.push(Field::pick("Hub to dial", hubs.to_vec()));
            }
            NodeKind::Hub => {
                fields.push(Field::new("Endpoint (host:port peers dial)", ""));
                fields.push(Field::new("Allowed-IPs peers route here", "0.0.0.0/0"));
                fields.push(Field::new("Keepalive seconds (optional)", ""));
            }
        }
        fields
    }

    /// The key-source toggle plus its one dependent field: generate -> store/redact
    /// the private; paste -> the existing public key to reference.
    fn key_fields(keysrc: KeySource) -> Vec<Field> {
        let mut fields = vec![Field::key_toggle(keysrc)];
        match keysrc {
            KeySource::Generate => fields.push(Field::pick("Private key", vec!["store in mesh.toml".into(), "redact (QR/file only)".into()])),
            KeySource::Paste => fields.push(Field::new("Public key (paste existing)", "")),
        }
        fields
    }

    fn create_node(kind: NodeKind, keysrc: KeySource, default_addr: &str, hubs: &[String]) -> Self {
        let mut fields = Self::base_fields(kind, default_addr, hubs);
        fields.extend(Self::key_fields(keysrc));
        Self { title: "Create a mesh node".into(), idx: 0, action: Action::AddNode, fields }
    }

    /// Prefill a Text field's value by label prefix (no-op if absent).
    fn set(&mut self, label_starts: &str, value: &str) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.label.starts_with(label_starts)) {
            f.value = value.to_string();
        }
    }

    /// Point a Pick field at `selected` if it's among the options.
    fn set_pick(&mut self, label_starts: &str, selected: &str) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.label.starts_with(label_starts))
            && let FieldKind::Pick { options, idx } = &mut f.kind
            && let Some(pos) = options.iter().position(|o| o == selected)
        {
            *idx = pos;
        }
    }

    /// A wizard prefilled from an existing node. Edit changes metadata only - the
    /// key is untouched (rotate is how you change keys), so no key fields here.
    fn edit_node(node: &Node, hubs: &[String]) -> Self {
        let kind = if node.endpoint.is_some() { NodeKind::Hub } else { NodeKind::Spoke };
        let mut p = Self { title: format!("Edit `{}`", node.name), fields: Self::base_fields(kind, &node.address, hubs), idx: 0, action: Action::EditNode { original: node.name.clone() } };
        p.set("Name", &node.name);
        p.set("Address", &node.address);
        p.set("DNS", node.dns.as_deref().unwrap_or(""));
        p.set("Endpoint", node.endpoint.as_deref().unwrap_or(""));
        p.set("Allowed-IPs", node.allowed_ips.as_deref().unwrap_or(""));
        p.set("Keepalive", &node.keepalive.map(|k| k.to_string()).unwrap_or_default());
        if let Some(h) = node.hubs.first() {
            p.set_pick("Hub to dial", h);
        }
        p
    }

    /// Cycle the current Pick field by `delta` (no-op on other field kinds).
    fn cycle_pick(&mut self, delta: isize) {
        if let FieldKind::Pick { options, idx } = &mut self.fields[self.idx].kind
            && !options.is_empty()
        {
            let n = options.len() as isize;
            *idx = (((*idx as isize + delta) % n + n) % n) as usize;
        }
    }

    /// Rebuild the form for a new kind/keysrc, carrying over what the user typed.
    fn rebuild(&mut self, kind: NodeKind, keysrc: KeySource, default_addr: &str, hubs: &[String]) {
        let old = std::mem::take(&mut self.fields);
        let mut next = if matches!(self.action, Action::EditNode { .. }) {
            Self::base_fields(kind, default_addr, hubs)
        } else {
            let mut f = Self::base_fields(kind, default_addr, hubs);
            f.extend(Self::key_fields(keysrc));
            f
        };
        for nf in next.iter_mut() {
            if let Some(of) = old.iter().find(|f| f.label == nf.label) {
                nf.value = of.value.clone();
                if let (FieldKind::Pick { options, idx }, FieldKind::Pick { options: oo, idx: oi }) = (&mut nf.kind, &of.kind)
                    && let Some(pos) = oo.get(*oi).and_then(|sel| options.iter().position(|o| o == sel))
                {
                    *idx = pos;
                }
            }
        }
        self.fields = next;
        self.idx = self.idx.min(self.fields.len().saturating_sub(1));
    }

    /// Flip Spoke<->Hub and rebuild, landing back on the Type row.
    fn toggle_kind(&mut self, default_addr: &str, hubs: &[String]) {
        self.rebuild(self.kind().toggled(), self.keysrc(), default_addr, hubs);
        self.idx = 0;
    }

    /// Flip Generate<->Paste and rebuild, staying on the Key row.
    fn toggle_keysrc(&mut self, default_addr: &str, hubs: &[String]) {
        let at = self.idx;
        self.rebuild(self.kind(), self.keysrc().toggled(), default_addr, hubs);
        self.idx = at.min(self.fields.len().saturating_sub(1));
    }
}

struct App {
    view: View,
    should_quit: bool,
    status: String,
    status_at: Option<Instant>,

    dirs: Vec<PathBuf>,
    ifaces: Vec<Iface>,
    iface_state: ListState,

    manifest_path: PathBuf,
    nodes: Vec<Node>,
    node_state: ListState,

    prompt: Option<Prompt>,
    overlay: Option<Overlay>,

    /// Set when a create/edit needs `$EDITOR`; the event loop drains it.
    pending_editor: Option<EditorReq>,
}

impl App {
    fn load(dirs: &[PathBuf]) -> Self {
        let manifest_path = PathBuf::from("mesh.toml");
        let nodes = Manifest::load_or_empty(&manifest_path).map(|m| m.nodes).unwrap_or_default();
        let mut app = App {
            view: View::Interfaces,
            should_quit: false,
            status: String::new(),
            status_at: None,
            dirs: dirs.to_vec(),
            ifaces: wg::interfaces(dirs).unwrap_or_default(),
            iface_state: ListState::default(),
            manifest_path,
            nodes,
            node_state: ListState::default(),
            prompt: None,
            overlay: None,
            pending_editor: None,
        };
        app.clamp_all();
        app
    }

    /// Set the transient status line; it fades on its own after `STATUS_TTL`.
    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
    }

    /// The status message while still fresh; `None` once it has expired.
    fn live_status(&self) -> Option<&str> {
        let at = self.status_at?;
        (at.elapsed() < STATUS_TTL && !self.status.is_empty()).then_some(self.status.as_str())
    }

    fn clamp_all(&mut self) {
        Self::clamp(&mut self.iface_state, self.ifaces.len());
        Self::clamp(&mut self.node_state, self.nodes.len());
    }

    fn clamp(state: &mut ListState, len: usize) {
        if len == 0 {
            state.select(None);
        } else {
            state.select(Some(state.selected().unwrap_or(0).min(len - 1)));
        }
    }

    fn active_list(&mut self) -> (&mut ListState, usize) {
        match self.view {
            View::Interfaces => (&mut self.iface_state, self.ifaces.len()),
            View::Mesh => (&mut self.node_state, self.nodes.len()),
        }
    }

    fn cycle_view(&mut self, delta: isize) {
        let cur = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let n = VIEWS.len() as isize;
        self.view = VIEWS[(((cur + delta) % n + n) % n) as usize];
    }

    fn move_sel(&mut self, delta: isize) {
        let (state, len) = self.active_list();
        if len == 0 {
            return;
        }
        let n = len as isize;
        let cur = state.selected().unwrap_or(0) as isize;
        state.select(Some((((cur + delta) % n + n) % n) as usize));
    }

    fn reload(&mut self, msg: impl Into<String>) {
        let (view, ifs, nds) = (self.view, self.iface_state.selected(), self.node_state.selected());
        let dirs = self.dirs.clone();
        *self = App::load(&dirs);
        self.view = view;
        if let Some(i) = ifs {
            self.iface_state.select(Some(i.min(self.ifaces.len().saturating_sub(1))));
        }
        if let Some(i) = nds {
            self.node_state.select(Some(i.min(self.nodes.len().saturating_sub(1))));
        }
        self.set_status(msg);
    }

    /// Suggest the next free `10.10.1.x/24` from the nodes already in the manifest.
    fn next_address(&self) -> String {
        let used: Vec<u8> = self
            .nodes
            .iter()
            .filter_map(|n| n.mesh_ip().strip_prefix("10.10.1.").and_then(|s| s.parse().ok()))
            .collect();
        let next = (1..=254).find(|c| !used.contains(c)).unwrap_or(1);
        format!("10.10.1.{next}/24")
    }

    /// Names of the hubs (nodes with an endpoint) — the pickable targets for a spoke.
    fn hub_names(&self) -> Vec<String> {
        self.nodes.iter().filter(|n| n.endpoint.is_some()).map(|n| n.name.clone()).collect()
    }

    /// Display order for the Mesh list: each hub, then its spokes indented under
    /// it; spokes with no (resolvable) hub trail at the end. Returns indices into
    /// `self.nodes`; every node appears exactly once.
    fn mesh_rows(&self) -> Vec<usize> {
        let is_hub = |i: usize| self.nodes[i].endpoint.is_some();
        let hubs: Vec<usize> = (0..self.nodes.len()).filter(|&i| is_hub(i)).collect();
        let under = |i: usize, hub: usize| self.nodes[i].hubs.first().is_some_and(|h| *h == self.nodes[hub].name);
        let mut rows = Vec::with_capacity(self.nodes.len());
        for &h in &hubs {
            rows.push(h);
            for i in 0..self.nodes.len() {
                if !is_hub(i) && under(i, h) {
                    rows.push(i);
                }
            }
        }
        for i in 0..self.nodes.len() {
            let placed = is_hub(i) || hubs.iter().any(|&h| under(i, h));
            if !placed {
                rows.push(i);
            }
        }
        rows
    }

    /// The node currently selected in the Mesh list (via the display order).
    fn selected_mesh_node(&self) -> Option<&Node> {
        let rows = self.mesh_rows();
        self.node_state.selected().and_then(|i| rows.get(i)).and_then(|&n| self.nodes.get(n))
    }

    // --- input ------------------------------------------------------------

    fn on_key(&mut self, key: KeyEvent) {
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
                    if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter) {
                        confirm = Some(action.clone());
                    }
                    close = true; // any key dismisses the confirm
                }
                Overlay::Menu { name, items, idx, .. } => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => *idx = (*idx + 1) % items.len(),
                    KeyCode::Up | KeyCode::Char('k') => *idx = (*idx + items.len() - 1) % items.len(),
                    KeyCode::Enter => {
                        do_export = Some((name.clone(), items[*idx].1));
                        close = true;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => close = true,
                    _ => {}
                },
                Overlay::Invalid { content, original, was_up, .. } => {
                    match key.code {
                        KeyCode::Char('e') | KeyCode::Char('c') | KeyCode::Enter => reopen = Some((content.clone(), original.clone(), *was_up)),
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

    fn interfaces_key(&mut self, key: KeyEvent) {
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

    /// The interface selected in the list, if any.
    fn selected_iface(&self) -> Option<&Iface> {
        self.iface_state.selected().and_then(|i| self.ifaces.get(i))
    }

    /// Re-select the interface named `name` after a reload (best effort).
    fn select_iface(&mut self, name: &str) {
        if let Some(i) = self.ifaces.iter().position(|f| f.name == name) {
            self.iface_state.select(Some(i));
        }
    }

    /// Suggest the next free `wgN` interface name from those already present.
    fn next_iface_name(&self) -> String {
        let used: std::collections::BTreeSet<&str> = self.ifaces.iter().map(|i| i.name.as_str()).collect();
        (0..=254).map(|n| format!("wg{n}")).find(|c| !used.contains(c.as_str())).unwrap_or_else(|| "wg0".into())
    }

    /// Start creating an interface: seed a temp file with a skeleton and ask the
    /// event loop to open `$EDITOR` on it. On save we prompt for the name.
    fn start_create_conf(&mut self) {
        const SKELETON: &str = "[Interface]\n# Paste your provider's config over this, or fill it in.\nPrivateKey = \nAddress = \n# DNS = 1.1.1.1\n\n[Peer]\nPublicKey = \n# PresharedKey = \nEndpoint = \nAllowedIPs = 0.0.0.0/0\n";
        match write_temp(SKELETON) {
            Ok(tmp) => self.pending_editor = Some(EditorReq { tmp, original: None, was_up: false, seed: SKELETON.to_string() }),
            Err(e) => self.set_status(format!("can't open an editor buffer: {e}")),
        }
    }

    /// Start editing the selected interface: seed the temp file with its current
    /// contents and open `$EDITOR`. On save we prompt for the name (rename-aware).
    fn start_edit_conf(&mut self) {
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
            Ok(tmp) => self.pending_editor = Some(EditorReq { tmp, original: Some(iface.path.clone()), was_up: iface.up, seed: content }),
            Err(e) => self.set_status(format!("can't open an editor buffer: {e}")),
        }
    }

    /// Called by the event loop once `$EDITOR` has exited. Saving nothing (buffer
    /// unchanged from the seed, or emptied) cancels. Otherwise the config is
    /// validated: a valid one goes to the name prompt; an invalid one pops a dialog
    /// naming the problem, so the user decides whether to fix it or throw it away.
    fn editor_done(&mut self, req: EditorReq) {
        let content = std::fs::read_to_string(&req.tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&req.tmp);
        if content == req.seed || content.trim().is_empty() {
            self.set_status(if req.original.is_some() { "no changes - nothing saved" } else { "nothing saved" });
            return;
        }
        match wg::validate_config(&content) {
            Ok(()) => self.open_conf_name_prompt(content, req.original, req.was_up),
            Err(e) => {
                self.overlay = Some(Overlay::Invalid { reason: e.to_string(), content, original: req.original, was_up: req.was_up })
            }
        }
    }

    /// Reopen `$EDITOR` on `content` (the "correct" choice from the invalid dialog).
    fn reopen_editor(&mut self, content: String, original: Option<PathBuf>, was_up: bool) {
        match write_temp(&content) {
            Ok(tmp) => self.pending_editor = Some(EditorReq { tmp, original, was_up, seed: content }),
            Err(e) => self.set_status(format!("can't reopen editor: {e}")),
        }
    }

    /// The name prompt shown after the editor: a Name field prefilled with the
    /// existing name (edit) or the next free `wgN` (create), plus a Directory
    /// picker when creating with more than one registered dir.
    fn open_conf_name_prompt(&mut self, content: String, original: Option<PathBuf>, was_up: bool) {
        let default_name = original.as_deref().map(stem).unwrap_or_else(|| self.next_iface_name());
        let mut name = Field::new("Name (.conf interface name)", "");
        name.value = default_name;
        let mut fields = vec![name];
        if original.is_none() && self.dirs.len() > 1 {
            let opts: Vec<String> = self.dirs.iter().map(|d| d.display().to_string()).collect();
            fields.push(Field::pick("Directory", opts));
        }
        let title = if original.is_some() { "Save interface".to_string() } else { "Name the interface".to_string() };
        self.prompt = Some(Prompt { title, fields, idx: 0, action: Action::SaveConf { content, original, was_up } });
    }

    /// Confirm-gate deleting the selected interface. Refuses while it is up: the
    /// running interface must be brought down (its `.conf` is what downs it) first.
    fn confirm_delete_iface(&mut self) {
        let Some(iface) = self.selected_iface() else {
            self.set_status("no interface selected");
            return;
        };
        if iface.up {
            self.set_status(format!("`{}` is up - toggle it down (↵) before deleting", iface.name));
            return;
        }
        let name = iface.name.clone();
        let path = iface.path.clone();
        self.overlay = Some(Overlay::Confirm { prompt: format!(" delete `{name}.conf`?  (a .bak is kept)  y / n "), action: ConfirmAction::DeleteIface(path) });
    }

    /// Back up then delete an interface `.conf`, and refresh the list.
    fn delete_iface(&mut self, path: PathBuf) {
        backup(&path);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                let name = stem(&path);
                self.reload(format!("deleted `{name}.conf` (.bak kept)"));
            }
            Err(e) => self.set_status(format!("can't delete {}: {e} (need sudo ewg?)", path.display())),
        }
    }

    /// Toggle whether the selected interface starts on boot (systemd).
    fn toggle_boot(&mut self) {
        let Some(iface) = self.selected_iface().cloned() else {
            self.set_status("no interface selected");
            return;
        };
        match iface.enabled {
            None => self.set_status("can't manage boot state - systemd/systemctl not available"),
            Some(enabled) => match wg::set_boot(&iface.name, !enabled) {
                Ok(()) => {
                    let name = iface.name.clone();
                    self.reload(format!("`{name}` will {} start on boot", if enabled { "no longer" } else { "now" }));
                    self.select_iface(&name);
                }
                Err(e) => self.set_status(format!("{e} (need sudo ewg?)")),
            },
        }
    }

    /// Inspect the selected interface: its `.conf`, plus a live `wg show` readout
    /// appended when the interface is up (handshakes, transfer, peers).
    fn inspect_iface(&mut self) {
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
        self.overlay = Some(Overlay::Text { title: format!(" {} ", iface.path.display()), body, scroll: 0 });
    }

    fn mesh_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') => {
                self.prompt = Some(Prompt::create_node(NodeKind::Spoke, KeySource::Generate, &self.next_address(), &self.hub_names()))
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
                            self.overlay = Some(Overlay::Text { title: format!(" {name}.conf (generated) "), body: m.node_config(node), scroll: 0 })
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
                self.overlay = Some(Overlay::Confirm { prompt: format!(" delete `{name}` from mesh.toml?  y / n "), action: ConfirmAction::DeleteNode(name) });
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

    /// Delete `name` from the manifest and refresh the in-memory list.
    fn delete_node(&mut self, name: &str) {
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
    fn show_qr(&mut self, m: &Manifest, node_name: String) {
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
    fn show_qr_config(&mut self, node_name: &str, config: &str) {
        if config.contains("<PASTE PRIVATE KEY>") {
            self.set_status(format!("`{node_name}` has no private key - rotate (R) to re-onboard"));
            return;
        }
        // Compact + lowest error-correction = fewest modules = smallest QR.
        let compact = compact_for_qr(config);
        match QrCode::with_error_correction_level(compact.as_bytes(), qrcode::EcLevel::L) {
            Ok(code) => {
                let width = code.width();
                let dark = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
                self.overlay = Some(Overlay::Qr { title: format!(" scan `{node_name}` "), width, dark });
            }
            Err(e) => self.set_status(format!("QR failed: {e}")),
        }
    }

    /// Config for a fresh QR: the node's public-only config with `private` spliced
    /// into the `[Interface]` (so the scan imports a working tunnel).
    fn working_config(m: &Manifest, node: &Node, private: &str) -> String {
        m.node_config(node).replace("<PASTE PRIVATE KEY>", private)
    }

    fn gen_all(&self) -> Result<usize> {
        let m = Manifest::load(&self.manifest_path)?;
        std::fs::create_dir_all("out")?;
        for node in &m.nodes {
            std::fs::write(format!("out/{}.conf", node.name), m.node_config(node))?;
        }
        Ok(m.nodes.len())
    }

    // --- wizard -----------------------------------------------------------

    fn prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.prompt.as_ref().map(|p| p.fields.len()).unwrap_or(0);
        if len == 0 {
            return;
        }
        // Choice rows (Type / Key / hub picker) are *cycled*, never typed into, so
        // both ←/→ and h/l work there - with or without ctrl, since ctrl-h/l is the
        // chord for "choose" (fields move with ctrl-j/k, so h/l stays free).
        let on_choice = self.prompt.as_ref().map(|p| p.fields[p.idx].is_choice()).unwrap_or(false);
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
        let next = matches!(key.code, KeyCode::Tab | KeyCode::Down) || (ctrl && key.code == KeyCode::Char('j'));
        let prev = matches!(key.code, KeyCode::BackTab | KeyCode::Up) || (ctrl && key.code == KeyCode::Char('k'));
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

    /// Consume the wizard and carry out its action. On a validation error it puts
    /// the prompt back so the field can be fixed.
    fn submit_prompt(&mut self) {
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
    fn submit_conf(&mut self, prompt: Prompt) {
        let (content, original, was_up) = match &prompt.action {
            Action::SaveConf { content, original, was_up } => (content.clone(), original.clone(), *was_up),
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
        let dir: PathBuf = if prompt.fields.iter().any(|f| f.label.starts_with("Directory")) {
            PathBuf::from(prompt.value_of("Directory"))
        } else if let Some(orig) = &original {
            orig.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
        } else {
            self.dirs.first().cloned().unwrap_or_else(|| PathBuf::from("."))
        };
        let target = dir.join(format!("{name}.conf"));

        let renaming = original.as_ref().is_some_and(|o| *o != target);
        if renaming && was_up {
            self.set_status(format!("take `{}` down (↵) before renaming it", stem(original.as_ref().unwrap())));
            self.prompt = Some(prompt);
            return;
        }
        backup(&target); // keep any file we're about to clobber
        if let Err(e) = std::fs::write(&target, &content) {
            self.set_status(format!("can't write {}: {e} (need sudo ewg?)", target.display()));
            self.prompt = Some(prompt);
            return;
        }
        if renaming && let Some(orig) = &original {
            backup(orig);
            let _ = std::fs::remove_file(orig);
        }
        let verb = if original.is_some() { "saved" } else { "created" };
        let tail = if was_up && !renaming { " - toggle (↵) to apply" } else { "" };
        self.reload(format!("{verb} {}{tail}", target.display()));
        self.select_iface(&name);
    }

    /// Turn a completed create/edit wizard into a manifest entry. The manifest is
    /// PUBLIC-ONLY (never stores a private key); a freshly generated key is handed
    /// out once via the QR + an out/<name>.conf, then discarded. On a validation
    /// error the prompt is put back so the field can be fixed.
    fn submit_node(&mut self, prompt: Prompt) {
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
        let existing = original.as_ref().and_then(|o| m.nodes.iter().find(|n| &n.name == o));
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
            keepalive: (kind == NodeKind::Hub).then(|| keepalive.parse().ok()).flatten(),
            hubs: if kind == NodeKind::Spoke && !hub.is_empty() { vec![hub] } else { Vec::new() },
            private_key: stored_private,
            post_up: None,
            post_down: None,
        };

        // On edit, drop the original first so the re-add doesn't collide with itself.
        if let Some(o) = &original {
            let _ = m.remove(o);
        }
        if let Err(e) = m.add(node) {
            self.set_status(format!("{} failed: {e}", if editing { "edit" } else { "add" }));
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
            let how = if redact { "redacted, key in QR/file only" } else { "key stored" };
            self.set_status(format!("added {} `{name}` ({how}) - QR or {where_}", kind.short()));
        } else if editing {
            self.set_status(format!("edited `{name}` - rotate (R) to re-onboard if its config changed"));
        } else {
            self.show_qr(&m, name.clone());
            self.set_status(format!("added {} `{name}`", kind.short()));
        }
        self.nodes = m.nodes; // move (Node isn't Clone); m is done being borrowed
        Self::clamp(&mut self.node_state, self.nodes.len());
    }

    /// Regenerate the selected node's keypair (public-only): update its pubkey,
    /// hand out the new key via QR + out/<name>.conf. The pubkey CHANGES, so its
    /// hub/Ansible must be updated to keep accepting it.
    fn rotate_selected(&mut self) {
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
        self.set_status(format!("rotated `{name}` - NEW pubkey, update its hub/Ansible; QR or {where_}"));
    }

    /// Open the Export menu for the selected node.
    fn open_export(&mut self) {
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
        self.overlay = Some(Overlay::Menu { title: format!(" export `{name}` "), name, items, idx: 0 });
    }

    /// Carry out an export for `name`. `.conf`/install/QR need the node's config;
    /// it carries the private key only if the node stores one (else placeholder).
    fn export(&mut self, name: &str, kind: ExportKind) {
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
                let line = format!("  - {{ name: {}, ip: {}, public_key: \"{}\" }}", node.name, ip, node.public_key);
                self.overlay = Some(Overlay::Text { title: " Ansible peer entry ".into(), body: line, scroll: 0 });
            }
        }
    }
}

/// Copy `text` to the clipboard, returning the method used. Prefers a system tool
/// (wl-copy/xclip/... - survives after ewg exits); falls back to an OSC 52 escape
/// so it still works with no tool installed and over SSH, if the terminal allows it.
fn copy_clipboard(text: &str) -> Option<&'static str> {
    let tools: [(&str, &[&str]); 5] = [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
        ("clip.exe", &[]),
    ];
    for (bin, args) in tools {
        let Ok(mut child) = Command::new(bin).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Some(bin);
        }
    }
    // Fallback: ask the terminal to copy via OSC 52 (base64 payload).
    let mut out = stdout();
    if write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes())).and_then(|_| out.flush()).is_ok() {
        return Some("osc52");
    }
    None
}

/// Minimal standard base64 (no deps) for the OSC 52 payload.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        s.push(T[(n >> 18 & 63) as usize] as char);
        s.push(T[(n >> 12 & 63) as usize] as char);
        s.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        s.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    s
}

/// Write a working `.conf` (private included) to `out/<name>.conf`, returning the
/// path for the status line. out/ is gitignored - the deliberate secret-file spot.
fn write_conf(name: &str, cfg: &str) -> String {
    let path = format!("out/{name}.conf");
    match std::fs::create_dir_all("out").and_then(|_| std::fs::write(&path, cfg)) {
        Ok(()) => path,
        Err(e) => format!("(couldn't write out/: {e})"),
    }
}

/// The file stem of `path` (the interface name for a `<name>.conf`).
fn stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
}

/// A temp file seeded with `content` for `$EDITOR` to open, with a `.conf`
/// extension so the editor highlights it. One per process is enough (edits are
/// sequential); the caller removes it once read back.
fn write_temp(content: &str) -> std::io::Result<PathBuf> {
    let mut p = std::env::temp_dir();
    p.push(format!("ewg-edit-{}.conf", std::process::id()));
    std::fs::write(&p, content)?;
    Ok(p)
}

/// Back up `path` to `<path>.bak.<epoch>` if it exists, so a clobber or delete is
/// recoverable. Returns the backup path, or None when there was nothing to copy.
fn backup(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut bak = path.as_os_str().to_os_string();
    bak.push(format!(".bak.{epoch}"));
    let bak = PathBuf::from(bak);
    std::fs::copy(path, &bak).ok().map(|_| bak)
}

/// Bring the interface at `path` up or down, returning a status line.
fn act(path: Option<PathBuf>, up: bool) -> String {
    let Some(path) = path else {
        return "no interface selected".into();
    };
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    match if up { wg::up(&path) } else { wg::down(&path) } {
        Ok(()) => format!("{} {name}", if up { "brought up" } else { "took down" }),
        Err(e) => format!("error: {e}"),
    }
}

pub fn run(dirs: &[PathBuf]) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    // Disambiguate Ctrl+letter (esp. Ctrl+h, which is otherwise byte-identical to
    // Backspace) so Ctrl-hjkl field nav works without releasing Ctrl. No-op on
    // terminals without the kitty keyboard protocol.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let result = event_loop(&mut terminal, dirs);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, dirs: &[PathBuf]) -> Result<()> {
    let mut app = App::load(dirs);
    while !app.should_quit {
        terminal.draw(|f| render(f, &mut app))?;
        // Poll so the status line can fade even with no keypresses.
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key);
        }
        // A create/edit asked for $EDITOR: suspend the TUI, run it, resume.
        if let Some(req) = app.pending_editor.take() {
            run_editor(terminal, &mut app, req)?;
        }
    }
    Ok(())
}

/// Suspend the TUI (restore the terminal so the editor owns it), run `$EDITOR`
/// (`$VISUAL`, then `$EDITOR`, then `vi`) on the temp file, then resume the TUI
/// and hand the result back to the app. Under sudo, `$EDITOR` may not survive the
/// re-exec, so `vi` is the floor; `sudo -E ewg` carries it through.
fn run_editor<B: Backend>(terminal: &mut Terminal<B>, app: &mut App, req: EditorReq) -> Result<()> {
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    let editor = std::env::var_os("VISUAL").or_else(|| std::env::var_os("EDITOR")).unwrap_or_else(|| "vi".into());
    let status = Command::new(&editor).arg(&req.tmp).status();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    if enhanced {
        let _ = execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    terminal.clear()?; // the editor scribbled over our screen; force a full redraw

    match status {
        Ok(_) => app.editor_done(req),
        Err(e) => {
            let _ = std::fs::remove_file(&req.tmp);
            app.set_status(format!("couldn't launch editor `{}`: {e}", editor.to_string_lossy()));
        }
    }
    Ok(())
}

fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    render_tabs(f, chunks[0], app);
    render_body(f, chunks[1], app);
    render_status(f, chunks[2], app);

    if let Some(p) = &app.prompt {
        render_prompt(f, area, p);
    }
    if let Some(ov) = &app.overlay {
        render_overlay(f, ov);
    }
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let idx = VIEWS.iter().position(|v| *v == app.view).unwrap_or(0);
    let tabs = Tabs::new(VIEWS.iter().map(|v| v.title()).collect::<Vec<_>>())
        .select(idx)
        .block(Block::default().borders(Borders::ALL).title(" easywireguard · ewg "))
        .divider("│")
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn render_body(f: &mut Frame, area: Rect, app: &mut App) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let sel = Style::default().add_modifier(Modifier::REVERSED);

    match app.view {
        View::Interfaces => {
            if app.ifaces.is_empty() {
                empty(f, area, "Interfaces", "No .conf files found.\nRegister a dir: ewg dir add <path>");
                return;
            }
            let w = app.ifaces.iter().map(|i| i.name.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = app
                .ifaces
                .iter()
                .map(|i| {
                    let (mark, mstyle) = if i.up {
                        ("● up  ", Style::default().fg(Color::Green))
                    } else {
                        ("○ down", dim)
                    };
                    let boot = if i.enabled == Some(true) {
                        Span::styled("  ⏻ boot", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, mstyle),
                        Span::raw("  "),
                        Span::styled(format!("{:w$}", i.name), bold),
                        boot,
                    ]))
                })
                .collect();
            let list = List::new(items).block(titled("Interfaces", app.ifaces.len())).highlight_style(sel).highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.iface_state);
        }

        View::Mesh => {
            if app.nodes.is_empty() {
                empty(f, area, "Mesh", "No nodes in mesh.toml here.\nPress `c` to create one (run ewg where mesh.toml lives).");
                return;
            }
            let rows = app.mesh_rows();
            let w = app.nodes.iter().map(|n| n.name.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| {
                    let n = &app.nodes[i];
                    let is_hub = n.endpoint.is_some();
                    let (indent, tag, tag_style, tail) = if is_hub {
                        ("", "hub  ", Style::default().fg(Color::Cyan), n.endpoint.clone().unwrap_or_default())
                    } else {
                        let h = n.hubs.first().cloned().unwrap_or_else(|| "all hubs".into());
                        ("  ", "spoke", dim, format!("→ {h}"))
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(tag, tag_style),
                        Span::raw(" "),
                        Span::styled(format!("{:w$}", n.name), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:16}", n.mesh_ip()), Style::default().fg(Color::Cyan)),
                        Span::raw("  "),
                        Span::styled(tail, dim),
                    ]))
                })
                .collect();
            let list = List::new(items).block(titled("Mesh", app.nodes.len())).highlight_style(sel).highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.node_state);
        }
    }
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.live_status() {
        Some(msg) => (msg.to_string(), Style::default().fg(Color::Green)),
        None => (app.view.hints(), Style::default().add_modifier(Modifier::DIM)),
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))), area);
}

fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    let height = p.fields.len() as u16 + 6;
    let rect = centered_pct(area, 72, height);
    f.render_widget(Clear, rect);
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, field) in p.fields.iter().enumerate() {
        let active = i == p.idx;
        let label_style = if active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let arrow = Span::raw(if active { "▸ " } else { "  " });
        match &field.kind {
            FieldKind::Type(kind) => {
                let dim = Style::default().add_modifier(Modifier::DIM);
                let opt = |k: NodeKind| {
                    let st = if k == *kind {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                    } else {
                        dim
                    };
                    Span::styled(format!(" {} ", k.short()), st)
                };
                lines.push(Line::from(vec![
                    arrow,
                    Span::styled("Type: ", label_style),
                    opt(NodeKind::Spoke),
                    Span::styled("│", dim),
                    opt(NodeKind::Hub),
                    Span::styled(if active { "   ←/→ toggle" } else { "" }, dim),
                ]));
            }
            FieldKind::Key(src) => {
                let dim = Style::default().add_modifier(Modifier::DIM);
                let opt = |k: KeySource| {
                    let st = if k == *src {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                    } else {
                        dim
                    };
                    Span::styled(format!(" {} ", k.short()), st)
                };
                lines.push(Line::from(vec![
                    arrow,
                    Span::styled("Key:  ", label_style),
                    opt(KeySource::Generate),
                    Span::styled("│", dim),
                    opt(KeySource::Paste),
                    Span::styled(if active { "   ←/→ toggle" } else { "" }, dim),
                ]));
            }
            FieldKind::Pick { options, idx } => {
                let dim = Style::default().add_modifier(Modifier::DIM);
                let chosen = if options.is_empty() {
                    Span::styled(" (no hubs yet - create one first) ", dim)
                } else {
                    Span::styled(format!(" {} ", options[*idx]), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED))
                };
                let counter = if options.len() > 1 { format!("  ({}/{})", idx + 1, options.len()) } else { String::new() };
                lines.push(Line::from(vec![
                    arrow,
                    Span::styled(format!("{}: ", field.label), label_style),
                    chosen,
                    Span::styled(counter, dim),
                    Span::styled(if active && options.len() > 1 { "   ←/→ choose" } else { "" }, dim),
                ]));
            }
            FieldKind::Text => {
                let head = if field.default.is_empty() {
                    format!("{}: ", field.label)
                } else {
                    format!("{} [{}]: ", field.label, field.default)
                };
                let cursor = if active { "█" } else { "" };
                lines.push(Line::from(vec![
                    arrow,
                    Span::styled(head, label_style),
                    Span::raw(format!("{}{}", field.value, cursor)),
                ]));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("  {CTRL_VIM_Y_MOVE} {Y_MOVE} move · {CTRL_VIM_X_MOVE} {X_MOVE} choose · enter next/submit · esc cancel"),
        Style::default().add_modifier(Modifier::DIM),
    )));
    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(format!(" {} ", p.title)).border_style(Style::default().fg(Color::Cyan)))
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}

fn render_overlay(f: &mut Frame, ov: &Overlay) {
    match ov {
        Overlay::Qr { title, width, dark } => {
            const QZ: usize = 2; // light quiet zone around the code
            let n = width + 2 * QZ;
            let is_dark = |x: usize, y: usize| -> bool {
                x >= QZ && y >= QZ && x < width + QZ && y < width + QZ && dark[(y - QZ) * width + (x - QZ)]
            };
            let full = f.area();
            // Half-block packs 2 modules/row (best scan). Quadrant packs 2x2/cell -
            // half the width - as a fallback when half-block is too wide to fit.
            let half = (n as u16 + 2, n.div_ceil(2) as u16 + 2);
            let quad = (n.div_ceil(2) as u16 + 2, n.div_ceil(2) as u16 + 2);
            let fits = |wh: (u16, u16)| wh.0 <= full.width && wh.1 <= full.height;
            let (lines, wh) = if fits(half) {
                (qr_half(n, &is_dark), half)
            } else if fits(quad) {
                (qr_quad(n, &is_dark), quad)
            } else {
                // Even the densest rendering won't fit - don't show a clipped, un-
                // scannable QR; point at the file/PNG instead.
                let area = centered(full, 50.min(full.width), 4);
                f.render_widget(Clear, area);
                f.render_widget(
                    Paragraph::new(format!("QR needs a {}x{} terminal.\nMaximize the window, or use E -> out/<name>.conf / PNG.", quad.0, quad.1))
                        .block(Block::default().borders(Borders::ALL).title(title.clone()).border_style(Style::default().fg(Color::Yellow)))
                        .wrap(Wrap { trim: false }),
                    area,
                );
                return;
            };
            let area = centered(full, wh.0, wh.1);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title.clone())),
                area,
            );
        }
        Overlay::Text { title, body, scroll } => {
            let full = f.area();
            let w = (body.lines().map(|l| l.chars().count()).max().unwrap_or(40) as u16 + 4).min(full.width.saturating_sub(4)).max(24);
            let h = (body.lines().count() as u16 + 3).min(full.height.saturating_sub(2)).max(6);
            let area = centered(full, w, h);
            f.render_widget(Clear, area);
            let foot = format!("  {VIM_Y_MOVE} {Y_MOVE} scroll · y copy · other key close");
            f.render_widget(
                Paragraph::new(body.clone())
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title.clone())
                            .title_bottom(Line::from(foot).right_aligned())
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
                    .scroll((*scroll, 0)),
                area,
            );
        }
        Overlay::Confirm { prompt, .. } => {
            let w = prompt.chars().count() as u16 + 2;
            let area = centered(f.area(), w, 3);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(prompt.clone())
                    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Red))),
                area,
            );
        }
        Overlay::Invalid { reason, .. } => {
            let full = f.area();
            let body = format!("This config is not valid:\n\n{reason}\n\ne / ↵  correct (reopen editor)\nd / esc  discard");
            let w = 64.min(full.width.saturating_sub(4)).max(28);
            let h = (body.lines().count() as u16 + 4).min(full.height.saturating_sub(2)).max(7);
            let area = centered(full, w, h);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(body)
                    .block(Block::default().borders(Borders::ALL).title(" invalid config ").border_style(Style::default().fg(Color::Yellow)))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Overlay::Menu { title, items, idx, .. } => {
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, (label, _))| {
                    let active = i == *idx;
                    Line::from(vec![
                        Span::raw(if active { "▸ " } else { "  " }),
                        Span::styled(label.clone(), if active { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().add_modifier(Modifier::DIM) }),
                    ])
                })
                .collect();
            let w = items.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(20) as u16 + 6;
            let area = centered(f.area(), w.max(title.chars().count() as u16 + 2), items.len() as u16 + 2);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title.clone())
                        .title_bottom(Line::from(format!("  {VIM_Y_MOVE} {Y_MOVE} · ↵ select · esc")).right_aligned())
                        .border_style(Style::default().fg(Color::Cyan)),
                ),
                area,
            );
        }
    }
}

/// A bordered block titled `Name (count)`.
fn titled(name: &str, count: usize) -> Block<'static> {
    Block::default().borders(Borders::ALL).title(format!(" {name} ({count}) "))
}

/// A friendly empty-state message inside the body block.
fn empty(f: &mut Frame, area: Rect, name: &str, msg: &str) {
    let para = Paragraph::new(msg)
        .block(titled(name, 0))
        .style(Style::default().add_modifier(Modifier::DIM))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

/// A rect centered in `area`, exactly `w`x`h` (capped to the screen).
/// Shrink a `.conf` for QR encoding without changing meaning: drop the alignment
/// padding around `=`, the client-irrelevant `ListenPort`, `[Peer]` name comments,
/// and blank lines. WireGuard parses `Key=Value` fine, so fewer bytes = smaller QR.
fn compact_for_qr(config: &str) -> String {
    let mut out = String::new();
    for line in config.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("ListenPort") {
            continue;
        }
        if let Some(h) = line.find('#') {
            let head = line[..h].trim(); // e.g. "[Peer]  # name" -> "[Peer]"
            if !head.is_empty() {
                out.push_str(head);
                out.push('\n');
            }
            continue;
        }
        match line.split_once('=') {
            Some((k, v)) => out.push_str(&format!("{}={}\n", k.trim(), v.trim())),
            None => out.push_str(&format!("{line}\n")),
        }
    }
    out
}

/// QR as upper-half blocks: 1 module/col, 2 modules/row (fg=top, bg=bottom).
/// Square-ish modules, best for scanning. `n` = side incl. quiet zone.
fn qr_half(n: usize, is_dark: &dyn Fn(usize, usize) -> bool) -> Vec<Line<'static>> {
    (0..n)
        .step_by(2)
        .map(|y| {
            Line::from(
                (0..n)
                    .map(|x| {
                        let fg = if is_dark(x, y) { Color::Black } else { Color::White };
                        let bg = if y + 1 < n && is_dark(x, y + 1) { Color::Black } else { Color::White };
                        Span::styled("▀", Style::default().fg(fg).bg(bg))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// QR as quadrant blocks: each cell packs a 2x2 module block (black on white), so
/// it's half the width of the half-block form - denser, at the cost of scan margin.
fn qr_quad(n: usize, is_dark: &dyn Fn(usize, usize) -> bool) -> Vec<Line<'static>> {
    // Glyph per 2x2 pattern, bit order TL=1, TR=2, BL=4, BR=8.
    const G: [char; 16] = [' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█'];
    let cell = Style::default().fg(Color::Black).bg(Color::White);
    (0..n)
        .step_by(2)
        .map(|y| {
            Line::from(
                (0..n)
                    .step_by(2)
                    .map(|x| {
                        let d = |dx: usize, dy: usize| x + dx < n && y + dy < n && is_dark(x + dx, y + dy);
                        let bits = d(0, 0) as usize | (d(1, 0) as usize) << 1 | (d(0, 1) as usize) << 2 | (d(1, 1) as usize) << 3;
                        Span::styled(G[bits].to_string(), cell)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// A rect centered in `area`, `pct`% wide and `height` rows tall.
fn centered_pct(area: Rect, pct: u16, height: u16) -> Rect {
    let w = area.width * pct / 100;
    centered(area, w, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        // Isolate tests from any real ./mesh.toml in the dev tree: start empty and
        // point the manifest at a path that doesn't exist.
        let mut a = App::load(&[]);
        a.manifest_path = std::env::temp_dir().join("ewg-tests-no-such-mesh.toml");
        a.nodes = Vec::new();
        a.node_state.select(None);
        a
    }

    fn node(name: &str, ip: u8) -> Node {
        Node {
            name: name.into(),
            address: format!("10.10.1.{ip}/24"),
            public_key: "PUB".into(),
            endpoint: None,
            allowed_ips: None,
            dns: None,
            keepalive: None,
            hubs: Vec::new(),
            private_key: None,
            post_up: None,
            post_down: None,
        }
    }

    /// Hand-rolled base64 (no dep) feeding the OSC 52 clipboard escape - a silent
    /// wrong-padding bug here would just produce a paste that looks fine and isn't.
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"WireGuard"), "V2lyZUd1YXJk");
    }

    /// Address allocation: handing out an in-use mesh IP would collide silently.
    #[test]
    fn next_address_picks_the_first_free_ip() {
        let mut a = app();
        assert_eq!(a.next_address(), "10.10.1.1/24", "empty manifest -> .1");
        a.nodes = vec![node("hub", 1), node("phone", 2), node("laptop", 4)];
        assert_eq!(a.next_address(), "10.10.1.3/24", "skips .1/.2/.4, takes the gap");
    }

    /// Status lines must stop showing once STATUS_TTL has passed.
    #[test]
    fn live_status_expires() {
        let mut a = app();
        a.set_status("hi");
        assert_eq!(a.live_status(), Some("hi"));
        a.status_at = Some(Instant::now() - STATUS_TTL - Duration::from_millis(1));
        assert_eq!(a.live_status(), None, "a stale status stops showing");
    }
}
