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

/// How long a status message stays before the hints return.
mod clipboard;
mod edit;
mod input;
mod interfaces;
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
use widgets::{centered, centered_pct, empty, tilde, titled};

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
    pub(super) fn hints(self) -> String {
        match self {
            View::Interfaces => format!(
                "{VIM_Y_MOVE} {Y_MOVE} move · {VIM_X_MOVE} {X_MOVE} tab · ↵ toggle · c new · e edit · d del · b boot · i inspect · r refresh · q quit"
            ),
            View::Mesh => format!(
                "{VIM_Y_MOVE} {Y_MOVE} move · {VIM_X_MOVE} {X_MOVE} tab · c create · e edit · R rotate · d del · E export · ↵ QR · i view · g gen · r reload · q quit"
            ),
        }
    }
}

pub(super) struct App {
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
    pub(super) fn load(dirs: &[PathBuf]) -> Self {
        let manifest_path = PathBuf::from("mesh.toml");
        let nodes = Manifest::load_or_empty(&manifest_path)
            .map(|m| m.nodes)
            .unwrap_or_default();
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
    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
    }

    /// The status message while still fresh; `None` once it has expired.
    pub(super) fn live_status(&self) -> Option<&str> {
        let at = self.status_at?;
        (at.elapsed() < STATUS_TTL && !self.status.is_empty()).then_some(self.status.as_str())
    }

    pub(super) fn clamp_all(&mut self) {
        Self::clamp(&mut self.iface_state, self.ifaces.len());
        Self::clamp(&mut self.node_state, self.nodes.len());
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
            View::Interfaces => (&mut self.iface_state, self.ifaces.len()),
            View::Mesh => (&mut self.node_state, self.nodes.len()),
        }
    }

    pub(super) fn cycle_view(&mut self, delta: isize) {
        let cur = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let n = VIEWS.len() as isize;
        self.view = VIEWS[(((cur + delta) % n + n) % n) as usize];
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
        *self = App::load(&dirs);
        self.view = view;
        if let Some(i) = ifs {
            self.iface_state
                .select(Some(i.min(self.ifaces.len().saturating_sub(1))));
        }
        if let Some(i) = nds {
            self.node_state
                .select(Some(i.min(self.nodes.len().saturating_sub(1))));
        }
        self.set_status(msg);
    }

    /// Suggest the next free `10.10.1.x/24` from the nodes already in the manifest.
    pub(super) fn next_address(&self) -> String {
        let used: Vec<u8> = self
            .nodes
            .iter()
            .filter_map(|n| {
                n.mesh_ip()
                    .strip_prefix("10.10.1.")
                    .and_then(|s| s.parse().ok())
            })
            .collect();
        let next = (1..=254).find(|c| !used.contains(c)).unwrap_or(1);
        format!("10.10.1.{next}/24")
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
    /// `self.nodes`; every node appears exactly once.
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

    // --- input ------------------------------------------------------------
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
        // Isolate tests from any real ./mesh.toml in the dev tree: start empty and
        // point the manifest at a path that doesn't exist.
        let mut a = App::load(&[]);
        a.manifest_path = std::env::temp_dir().join("ewg-tests-no-such-mesh.toml");
        a.nodes = Vec::new();
        a.node_state.select(None);
        a
    }

    pub(super) fn node(name: &str, ip: u8) -> Node {
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

    /// Address allocation: handing out an in-use mesh IP would collide silently.
    #[test]
    pub(super) fn next_address_picks_the_first_free_ip() {
        let mut a = app();
        assert_eq!(a.next_address(), "10.10.1.1/24", "empty manifest -> .1");
        a.nodes = vec![node("hub", 1), node("phone", 2), node("laptop", 4)];
        assert_eq!(
            a.next_address(),
            "10.10.1.3/24",
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
