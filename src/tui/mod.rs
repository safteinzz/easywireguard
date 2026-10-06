//! The toolbox, bare `ewg`. Two tabs (Interfaces and Mesh), switched with
//! `h/l ←→` or Tab, navigated with `j/k ↑↓` like a normal list, `/` filtering
//! either list and `?` listing every key:
//!
//! - **Interfaces**: the `.conf` files across your registered dirs; `↵` turns one
//!   on or off, `c` creates one in `$EDITOR` (paste a provider config), `e` edits,
//!   `d` deletes it (a `.bak` is kept), `b` turns start-on-boot on or off, `i`
//!   inspects it (with live `wg show` when up).
//! - **Mesh**: the nodes in a manifest, shown hub-and-spoke (spokes nested under
//!   their hub). `c` creates one (a Spoke/Hub wizard that generates keys and pops a
//!   QR to scan), `↵` shows a node's QR, `i` inspects its generated config, `d`
//!   deletes it once its name is typed.
//!
//! Modeled on `simplessh`'s tabbed layout: a bordered tab bar titled with the app
//! name, `Name (count)` bodies, a status line that fades after `STATUS_TTL`, and
//! centered overlays (a wizard, a QR, a confirm).

use anyhow::Result;
use ratatui::crossterm::{
    event::{
        self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use ratatui::prelude::*;
use ratatui::widgets::ListState;
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::manifest::{Manifest, Node};
use crate::wg::{self, Iface};

mod clipboard;
mod edit;
mod filter;
mod input;
mod interfaces;
mod line_edit;
mod mesh;
mod overlay;
mod prompt;
mod render;
mod widgets;
mod wizard;

use clipboard::copy_clipboard;
use edit::{Action, EditorReq, act, run_editor};
use overlay::Overlay;
use prompt::{Field, FieldKind, KeySource, NodeKind, Prompt};
use render::render;
use widgets::{
    CREATE, DEL, EDIT, FIND, INSPECT, QUIT, REFRESH, above_status, box_area, box_block,
    box_buttons, box_height, box_hint, box_inner_width, box_width, empty, tilde, titled,
    wrapped_line_count,
};

/// How long a status message stays before the keys return.
const STATUS_TTL: Duration = Duration::from_secs(3);

// The footer under each tab: actions only, in the house order.
const INTERFACES_KEYS: &[&str] = &[
    "↵ on/off",
    "b boot",
    INSPECT,
    CREATE,
    EDIT,
    DEL,
    FIND,
    REFRESH,
    QUIT,
];
const MESH_KEYS: &[&str] = &[
    "↵ QR", INSPECT, "R rotate", "E export", "g gen", CREATE, EDIT, DEL, FIND, REFRESH, QUIT,
];

/// The tabs, in left-to-right / Tab-cycle order.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Interfaces,
    Mesh,
}

const VIEWS: [View; 2] = [View::Interfaces, View::Mesh];

impl View {
    pub(super) fn title(self) -> &'static str {
        match self {
            View::Interfaces => "Interfaces",
            View::Mesh => "Mesh",
        }
    }
    pub(super) fn keys(self) -> &'static [&'static str] {
        match self {
            View::Interfaces => INTERFACES_KEYS,
            View::Mesh => MESH_KEYS,
        }
    }
}

pub(super) struct App {
    view: View,
    should_quit: bool,
    status: String,
    /// Whether the message on screen is a failure, which is all that decides
    /// its colour.
    pub(super) status_failed: bool,
    status_at: Option<Instant>,

    dirs: Vec<PathBuf>,
    ifaces: Vec<Iface>,
    iface_state: ListState,

    manifest_path: PathBuf,
    nodes: Vec<Node>,
    node_state: ListState,

    /// The `/` filter over the current tab's list, dropped when the tab changes.
    query: String,
    /// The cursor in `query`, as characters after it (`line_edit::edit`).
    query_back: usize,
    /// Whether `/` is still being typed, so letters go into `query`.
    searching: bool,
    show_help: bool,
    /// The first help row on screen; `render_help` clamps it to the end.
    help_scroll: usize,

    prompt: Option<Prompt>,
    overlay: Option<Overlay>,

    /// Set when a create/edit needs `$EDITOR`; the event loop drains it.
    pending_editor: Option<EditorReq>,
}

impl App {
    pub(super) fn load(dirs: &[PathBuf]) -> Self {
        let manifest_path = PathBuf::from("mesh.toml");
        let nodes = Manifest::load_or_empty(&manifest_path)
            .map(|m| m.nodes)
            .unwrap_or_default();
        let ifaces = wg::interfaces(dirs).unwrap_or_default();
        Self::new(dirs, ifaces, manifest_path, nodes)
    }

    fn new(dirs: &[PathBuf], ifaces: Vec<Iface>, manifest_path: PathBuf, nodes: Vec<Node>) -> Self {
        let mut app = App {
            view: View::Interfaces,
            should_quit: false,
            status: String::new(),
            status_failed: false,
            status_at: None,
            dirs: dirs.to_vec(),
            ifaces,
            iface_state: ListState::default(),
            manifest_path,
            nodes,
            node_state: ListState::default(),
            query: String::new(),
            query_back: 0,
            searching: false,
            show_help: false,
            help_scroll: 0,
            prompt: None,
            overlay: None,
            pending_editor: None,
        };
        app.clamp_all();
        app
    }

    /// Set the transient status line for something that worked; it fades on its
    /// own after `STATUS_TTL`.
    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
        self.status_failed = false;
    }

    /// The same line for something that did not. Yellow, the same yellow an
    /// alert uses: red is reserved for a gate in front of something about to be
    /// lost, and this has already happened.
    pub(super) fn set_failed(&mut self, msg: impl Into<String>) {
        self.set_status(msg);
        self.status_failed = true;
    }

    /// The status message while still fresh; `None` once it has expired.
    pub(super) fn live_status(&self) -> Option<&str> {
        let at = self.status_at?;
        (at.elapsed() < STATUS_TTL && !self.status.is_empty()).then_some(self.status.as_str())
    }

    pub(super) fn clamp_all(&mut self) {
        let n = self.iface_rows().len();
        Self::clamp(&mut self.iface_state, n);
        let n = self.mesh_rows().len();
        Self::clamp(&mut self.node_state, n);
    }

    pub(super) fn clamp(state: &mut ListState, len: usize) {
        if len == 0 {
            state.select(None);
        } else {
            state.select(Some(state.selected().unwrap_or(0).min(len - 1)));
        }
    }

    pub(super) fn active_list(&mut self) -> (&mut ListState, usize) {
        match self.view {
            View::Interfaces => {
                let n = self.iface_rows().len();
                (&mut self.iface_state, n)
            }
            View::Mesh => {
                let n = self.mesh_rows().len();
                (&mut self.node_state, n)
            }
        }
    }

    /// The rows the current tab shows, after the filter.
    pub(super) fn row_count(&self) -> usize {
        match self.view {
            View::Interfaces => self.iface_rows().len(),
            View::Mesh => self.mesh_rows().len(),
        }
    }

    /// The interfaces the filter keeps, as indices into `self.ifaces`. Every
    /// selection goes through here, since the list state indexes these rows.
    pub(super) fn iface_rows(&self) -> Vec<usize> {
        (0..self.ifaces.len())
            .filter(|&i| filter::matches(&self.query, &[&self.ifaces[i].name]))
            .collect()
    }

    /// The query changed: start each list from its first match.
    pub(super) fn requery(&mut self) {
        self.iface_state.select(Some(0));
        self.node_state.select(Some(0));
        self.clamp_all();
    }

    pub(super) fn cycle_view(&mut self, delta: isize) {
        let cur = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let n = VIEWS.len() as isize;
        self.view = VIEWS[(((cur + delta) % n + n) % n) as usize];
        // A filter belongs to the list it was typed over; carried into the
        // other tab it would hide rows nobody searched for.
        if !self.query.is_empty() || self.searching {
            self.query.clear();
            self.query_back = 0;
            self.searching = false;
            self.requery();
        }
    }

    pub(super) fn move_sel(&mut self, delta: isize) {
        let (state, len) = self.active_list();
        if len == 0 {
            return;
        }
        let n = len as isize;
        let cur = state.selected().unwrap_or(0) as isize;
        state.select(Some((((cur + delta) % n + n) % n) as usize));
    }

    pub(super) fn reload(&mut self, msg: impl Into<String>) {
        let (view, ifs, nds) = (
            self.view,
            self.iface_state.selected(),
            self.node_state.selected(),
        );
        let dirs = self.dirs.clone();
        let (query, query_back) = (std::mem::take(&mut self.query), self.query_back);
        *self = App::load(&dirs);
        self.view = view;
        self.query = query;
        self.query_back = query_back;
        if let Some(i) = ifs {
            self.iface_state.select(Some(i));
        }
        if let Some(i) = nds {
            self.node_state.select(Some(i));
        }
        self.clamp_all();
        self.set_status(msg);
    }

    /// Suggest the next free `10.99.0.x/24` from the nodes already in the manifest.
    pub(super) fn next_address(&self) -> String {
        let used: Vec<u8> = self
            .nodes
            .iter()
            .filter_map(|n| {
                n.mesh_ip()
                    .strip_prefix("10.99.0.")
                    .and_then(|s| s.parse().ok())
            })
            .collect();
        let next = (1..=254).find(|c| !used.contains(c)).unwrap_or(1);
        format!("10.99.0.{next}/24")
    }

    /// Names of the hubs (nodes with an endpoint) - the pickable targets for a spoke.
    pub(super) fn hub_names(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| n.endpoint.is_some())
            .map(|n| n.name.clone())
            .collect()
    }

    /// Display order for the Mesh list: each hub, then its spokes indented under
    /// it; spokes with no (resolvable) hub trail at the end. Returns indices into
    /// `self.nodes` the filter keeps, each at most once, and every selection
    /// goes through here.
    pub(super) fn mesh_rows(&self) -> Vec<usize> {
        let is_hub = |i: usize| self.nodes[i].endpoint.is_some();
        let hubs: Vec<usize> = (0..self.nodes.len()).filter(|&i| is_hub(i)).collect();
        let under = |i: usize, hub: usize| {
            self.nodes[i]
                .hubs
                .first()
                .is_some_and(|h| *h == self.nodes[hub].name)
        };
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
        rows.retain(|&i| {
            let n = &self.nodes[i];
            let hub = n.hubs.first().map(String::as_str).unwrap_or("");
            let endpoint = n.endpoint.as_deref().unwrap_or("");
            filter::matches(&self.query, &[&n.name, &n.address, endpoint, hub])
        });
        rows
    }

    /// The node currently selected in the Mesh list (via the display order).
    pub(super) fn selected_mesh_node(&self) -> Option<&Node> {
        let rows = self.mesh_rows();
        self.node_state
            .selected()
            .and_then(|i| rows.get(i))
            .and_then(|&n| self.nodes.get(n))
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
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn app() -> App {
        App::new(&[], Vec::new(), PathBuf::new(), Vec::new())
    }

    pub(super) fn node(name: &str, ip: u8) -> Node {
        Node {
            name: name.into(),
            address: format!("10.99.0.{ip}/24"),
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

    /// Address allocation: handing out an in-use mesh IP would collide silently.
    #[test]
    pub(super) fn next_address_picks_the_first_free_ip() {
        let mut a = app();
        assert_eq!(a.next_address(), "10.99.0.1/24", "empty manifest -> .1");
        a.nodes = vec![node("hub", 1), node("phone", 2), node("laptop", 4)];
        assert_eq!(
            a.next_address(),
            "10.99.0.3/24",
            "skips .1/.2/.4, takes the gap"
        );
    }

    /// Status lines must stop showing once STATUS_TTL has passed.
    #[test]
    pub(super) fn live_status_expires() {
        let mut a = app();
        a.set_status("hi");
        assert_eq!(a.live_status(), Some("hi"));
        a.status_at = Some(Instant::now() - STATUS_TTL - Duration::from_millis(1));
        assert_eq!(a.live_status(), None, "a stale status stops showing");
    }
}
