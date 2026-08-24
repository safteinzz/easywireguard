//! Pane furniture with no idea what it is drawing: titled blocks, an empty
//! placeholder and the centering maths for an overlay.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::path::Path;

/// A bordered block titled `Name (count)`.
pub(super) fn titled(name: &str, count: usize) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {name} ({count}) "))
}

/// A friendly empty-state message inside the body block.
pub(super) fn empty(f: &mut Frame, area: Rect, name: &str, msg: &str) {
    let para = Paragraph::new(msg)
        .block(titled(name, 0))
        .style(Style::default().add_modifier(Modifier::DIM))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

pub(super) fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// A rect centered in `area`, `pct`% wide and `height` rows tall.
pub(super) fn centered_pct(area: Rect, pct: u16, height: u16) -> Rect {
    let w = area.width * pct / 100;
    centered(area, w, height)
}

/// A path with the home directory collapsed to `~`, for anything that puts a
/// path on screen: `/home/you/wg/mesh.conf` is mostly noise, and the part that
/// identifies the file is the tail.
pub(super) fn tilde(path: &Path) -> String {
    let full = path.display().to_string();
    let Some(home) = dirs::home_dir() else {
        return full;
    };
    let home = home.display().to_string();
    match full.strip_prefix(&home) {
        Some("") => "~".into(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => full,
    }
}
