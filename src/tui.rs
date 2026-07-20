//! Interactive interface manager: list `.conf` files across the registered dirs,
//! see up/down, toggle with a keypress. Launched by bare `ewg` (or `ewg tui`).

use anyhow::Result;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::io::stdout;
use std::path::PathBuf;

use crate::wg::{self, Iface};

struct App {
    ifaces: Vec<Iface>,
    sel: usize,
    msg: String,
}

impl App {
    fn load(dirs: &[PathBuf]) -> Self {
        App {
            ifaces: wg::interfaces(dirs).unwrap_or_default(),
            sel: 0,
            msg: String::new(),
        }
    }

    fn selected(&self) -> Option<&Iface> {
        self.ifaces.get(self.sel)
    }

    fn wants_up(&self) -> bool {
        self.ifaces.get(self.sel).map(|i| !i.up).unwrap_or(false)
    }

    /// Move selection with wraparound.
    fn mv(&mut self, delta: isize) {
        if self.ifaces.is_empty() {
            return;
        }
        let n = self.ifaces.len() as isize;
        self.sel = (((self.sel as isize + delta) % n + n) % n) as usize;
    }

    /// Reload from disk, keeping the cursor in range and a status message.
    fn reload(&mut self, dirs: &[PathBuf], msg: String) {
        let sel = self.sel;
        *self = App::load(dirs);
        self.sel = sel.min(self.ifaces.len().saturating_sub(1));
        self.msg = msg;
    }
}

pub fn run(dirs: &[PathBuf]) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = event_loop(&mut terminal, dirs);

    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, dirs: &[PathBuf]) -> Result<()> {
    let mut app = App::load(dirs);
    loop {
        terminal.draw(|f| render(f, &app))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('l') => app.mv(1),
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k') | KeyCode::Char('h') => app.mv(-1),
            KeyCode::Char('u') => {
                let msg = act(app.selected().map(|i| i.path.clone()), true);
                app.reload(dirs, msg);
            }
            KeyCode::Char('d') => {
                let msg = act(app.selected().map(|i| i.path.clone()), false);
                app.reload(dirs, msg);
            }
            KeyCode::Enter => {
                let up = app.wants_up();
                let msg = act(app.selected().map(|i| i.path.clone()), up);
                app.reload(dirs, msg);
            }
            KeyCode::Char('r') => app.reload(dirs, "refreshed".into()),
            _ => {}
        }
    }
    Ok(())
}

/// Bring the interface at `path` up or down, returning a status line.
fn act(path: Option<PathBuf>, up: bool) -> String {
    let Some(path) = path else {
        return "no interface selected".into();
    };
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let result = if up { wg::up(&path) } else { wg::down(&path) };
    match result {
        Ok(()) => format!("{} {name}", if up { "brought up" } else { "took down" }),
        Err(e) => format!("error: {e}"),
    }
}

fn render(f: &mut Frame, app: &App) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(f.area());

    f.render_widget(Paragraph::new(" ewg  wireguard interfaces").bold(), rows[0]);

    if app.ifaces.is_empty() {
        f.render_widget(
            Paragraph::new("no .conf files found (register a dir: ewg dir add <path>)")
                .block(Block::default().borders(Borders::ALL).title("interfaces")),
            rows[1],
        );
    } else {
        let items: Vec<ListItem> = app
            .ifaces
            .iter()
            .map(|i| {
                let mark = if i.up { "● up  " } else { "○ down" };
                ListItem::new(format!("{mark}  {}", i.name))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(app.sel));
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("interfaces"))
            .highlight_symbol("> ");
        f.render_stateful_widget(list, rows[1], &mut state);
    }

    let footer = if app.msg.is_empty() {
        "arrows/hjkl move   u up   d down   enter toggle   r refresh   q quit".to_string()
    } else {
        app.msg.clone()
    };
    f.render_widget(Paragraph::new(footer), rows[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn iface(name: &str, up: bool) -> Iface {
        Iface {
            name: name.into(),
            up,
            path: format!("/etc/wireguard/{name}.conf").into(),
        }
    }

    fn app_with(ifaces: Vec<Iface>) -> App {
        App {
            ifaces,
            sel: 0,
            msg: String::new(),
        }
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut app = app_with(vec![iface("a", false), iface("b", false)]);
        app.mv(-1);
        assert_eq!(app.sel, 1, "up from first wraps to last");
        app.mv(1);
        assert_eq!(app.sel, 0, "down from last wraps to first");
    }

    #[test]
    fn selection_no_op_on_empty() {
        let mut app = app_with(vec![]);
        app.mv(1);
        assert_eq!(app.sel, 0);
        assert!(app.selected().is_none());
    }

    #[test]
    fn wants_up_is_the_opposite_of_current_state() {
        assert!(!app_with(vec![iface("a", true)]).wants_up());
        assert!(app_with(vec![iface("a", false)]).wants_up());
    }

    #[test]
    fn render_shows_names_and_up_down_state() {
        let app = app_with(vec![iface("wg0", true), iface("wg1", false)]);
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("wg0") && text.contains("wg1"));
        assert!(text.contains("up") && text.contains("down"));
    }
}
