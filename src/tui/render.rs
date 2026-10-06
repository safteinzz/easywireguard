//! Drawing the frame: the tab bar, the list body, the status line and help.

use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs};

use super::overlay::{clamp_text_scroll, render_overlay, render_prompt};
use super::widgets::{BACK, READER_KEYS, SEP, key_footer, vscrollbar};
use super::*;

pub(super) fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(f, chunks[0], app);
    render_body(f, chunks[1], app);
    render_status(f, chunks[2], app);

    let boxes = above_status(area);
    if app.show_help {
        render_help(f, boxes, app);
    }
    if let Some(p) = &app.prompt {
        render_prompt(f, boxes, p);
    }
    if let Some(ov) = app.overlay.as_mut() {
        clamp_text_scroll(boxes, ov);
        render_overlay(f, boxes, ov);
    }
}

pub(super) fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let idx = VIEWS.iter().position(|v| *v == app.view).unwrap_or(0);
    let tabs = Tabs::new(VIEWS.iter().map(|v| v.title()).collect::<Vec<_>>())
        .select(idx)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" easywireguard · ewg "),
        )
        .divider("│")
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

pub(super) fn render_body(f: &mut Frame, area: Rect, app: &mut App) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let sel = Style::default().add_modifier(Modifier::REVERSED);

    match app.view {
        View::Interfaces => {
            if app.ifaces.is_empty() {
                empty(
                    f,
                    area,
                    "Interfaces",
                    "No .conf files found.\nRegister a dir: ewg dir add <path>",
                );
                return;
            }
            let rows = app.iface_rows();
            if rows.is_empty() {
                no_match(f, area, "Interfaces", &app.query);
                return;
            }
            let w = app.ifaces.iter().map(|i| i.name.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| {
                    let i = &app.ifaces[i];
                    // Only the words are coloured, never their padding: the
                    // selected row is drawn reversed, and coloured spaces would
                    // turn into a solid block.
                    let (mark, mstyle) = if i.up {
                        ("● up", Style::default().fg(Color::Green))
                    } else {
                        ("○ down", dim)
                    };
                    let boot = if i.enabled == Some(true) {
                        Span::styled("⏻ boot", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, mstyle),
                        Span::raw(" ".repeat(8 - mark.chars().count())),
                        Span::styled(format!("{:w$}", i.name), bold),
                        Span::raw("  "),
                        boot,
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Interfaces", rows.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.iface_state);
            list_bar(f, area, rows.len(), app.iface_state.offset());
        }

        View::Mesh => {
            if app.nodes.is_empty() {
                empty(
                    f,
                    area,
                    "Mesh",
                    "No nodes in mesh.toml here.\nPress `c` to create one (run ewg where mesh.toml lives).",
                );
                return;
            }
            let rows = app.mesh_rows();
            if rows.is_empty() {
                no_match(f, area, "Mesh", &app.query);
                return;
            }
            let w = app.nodes.iter().map(|n| n.name.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| {
                    let n = &app.nodes[i];
                    let is_hub = n.endpoint.is_some();
                    let (indent, tag, tag_style, tail) = if is_hub {
                        (
                            "",
                            "hub  ",
                            Style::default().fg(Color::Cyan),
                            n.endpoint.clone().unwrap_or_default(),
                        )
                    } else {
                        let h = n.hubs.first().cloned().unwrap_or_else(|| "all hubs".into());
                        ("  ", "spoke", dim, format!("→ {h}"))
                    };
                    let ip = n.mesh_ip();
                    ListItem::new(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(tag.trim_end(), tag_style),
                        Span::raw(" ".repeat(6 - tag.trim_end().chars().count())),
                        Span::styled(format!("{:w$}", n.name), bold),
                        Span::raw("  "),
                        Span::styled(ip.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(" ".repeat(18usize.saturating_sub(ip.chars().count()))),
                        Span::styled(tail, dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Mesh", rows.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.node_state);
            list_bar(f, area, rows.len(), app.node_state.offset());
        }
    }
}

/// The scrollbar of a bordered list pane, after the list is drawn so its
/// `offset` is the one on screen.
fn list_bar(f: &mut Frame, area: Rect, total: usize, offset: usize) {
    vscrollbar(
        f,
        area,
        total,
        offset,
        area.height.saturating_sub(2) as usize,
    );
}

/// The body when a filter hides every row, so an empty list never reads as
/// an empty config.
fn no_match(f: &mut Frame, area: Rect, name: &str, query: &str) {
    empty(
        f,
        area,
        name,
        &format!("Nothing matches `/{query}`.\nPress esc to see everything again."),
    );
}

pub(super) fn render_status(f: &mut Frame, area: Rect, app: &App) {
    // While `/` is being typed the line belongs to the query: it is the only
    // place what you typed is visible.
    if app.searching {
        let mut spans = vec![Span::styled(
            " /",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(line_edit::with_cursor(
            &app.query,
            app.query_back,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        // Every letter goes into the query here, so only keys that are not
        // letters are offered.
        spans.push(Span::styled(
            format!("   {} match   ↵ keep{SEP}{BACK}", app.row_count()),
            Style::default().add_modifier(Modifier::DIM),
        ));
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }
    let line = match app.live_status() {
        // Green for what worked, yellow for what did not, and never red: red
        // means a gate in front of something you are about to lose.
        Some(msg) => Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(if app.status_failed {
                Color::Yellow
            } else {
                Color::Green
            }),
        )),
        None => {
            // A committed filter stays visible in front of the keys: rows are
            // hidden, and nothing else on screen would say why.
            let lead = match app.query.is_empty() {
                true => Vec::new(),
                false => vec![format!("/{}", app.query), BACK.to_string()],
            };
            key_footer(&lead, app.view.keys(), area.width)
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/// One group of the help panel: a heading, then `(keys, what they do)` rows,
/// where a row with no keys is a note about the group.
type HelpSection = (&'static str, &'static [(&'static str, &'static str)]);

/// Every key the app answers to, grouped by where it works. The panel scrolls,
/// so a new row costs nothing but its line.
const HELP: &[HelpSection] = &[
    (
        "moving",
        &[
            ("j/k ↑↓", "move in the list"),
            ("h/l ←→", "the previous, next tab"),
            ("tab shift-tab", "the next, previous tab"),
            ("ctrl-j/k/h/l", "the same"),
        ],
    ),
    (
        "every tab",
        &[
            ("/", "find in the list, esc drops it"),
            ("r", "refresh what the tab shows"),
            ("?", "this help"),
            ("q ctrl-c", "quit (ctrl-c is esc in a box)"),
        ],
    ),
    (
        "interfaces",
        &[
            ("↵", "on/off (wg-quick up, down)"),
            ("c", "create one in $EDITOR"),
            ("e", "edit it"),
            ("d", "delete it (a .bak is kept)"),
            ("i", "inspect it (wg show)"),
            ("b", "on/off: start on boot (systemctl, wg-quick@)"),
            ("", "● up · ○ down · ⏻ boot starts with the machine"),
        ],
    ),
    (
        "mesh",
        &[
            ("↵", "a QR to scan"),
            ("c", "create a node"),
            ("e", "edit it"),
            ("d", "delete it"),
            ("i", "inspect its .conf"),
            ("R", "rotate its key"),
            ("E", "export it"),
            ("g", "gen every .conf to ./out"),
        ],
    ),
    (
        "in a form",
        &[
            ("type", "fill the field"),
            ("ctrl-j/k ↑↓", "the next, previous field"),
            ("tab shift-tab", "the next, previous field"),
            ("h/l ←→", "step a choice"),
            ("↵", "the next field, and submit on the last"),
            ("esc", "cancel"),
        ],
    ),
    (
        "in a box",
        &[
            ("y n", "answer"),
            ("h/l ←→", "move between the buttons"),
            ("tab shift-tab", "the same"),
            ("↵", "select, or pick from a list"),
            ("j/k ↑↓", "move in a list"),
            ("esc", "cancel"),
            ("", "a name to type: type it, then ↵ deletes"),
        ],
    ),
    (
        "in a reader",
        &[
            ("j/k ↑↓", "scroll"),
            ("y", "yank what it shows"),
            ("esc", "close"),
        ],
    ),
    (
        "in this help",
        &[
            ("j/k ↑↓", "scroll"),
            ("ctrl-d ctrl-u", "half a page down, up"),
            ("g G", "the top, the bottom"),
            ("esc q ?", "close"),
        ],
    ),
];

/// The width of the key column, so every description starts in one place.
const HELP_KEYS: usize = 16;

pub(super) fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (section, entries) in HELP {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            *section,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        for (keys, what) in *entries {
            if keys.is_empty() {
                lines.push(Line::styled(
                    format!("  {what}"),
                    Style::default().add_modifier(Modifier::DIM),
                ));
                continue;
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {keys:<HELP_KEYS$}"),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(*what),
            ]));
        }
    }
    lines
}

/// The help reader: the body scrolls under a key row that never moves, with a
/// scrollbar on the right border once it is taller than the box.
pub(super) fn render_help(f: &mut Frame, area: Rect, app: &mut App) {
    let lines = help_lines();
    let width = box_width(area.width);
    // The body, then a blank and the key row.
    let rect = box_area(area, width, box_height(lines.len() as u16 + 2, area.height));
    f.render_widget(Clear, rect);
    let block = box_block(Color::Cyan, "help");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let shown = inner.height.saturating_sub(2) as usize;
    // Clamped here, where the height is known, so scrolling past the end never
    // piles up presses that then take as many to undo.
    app.help_scroll = app.help_scroll.min(lines.len().saturating_sub(shown));
    let top = app.help_scroll;
    let body = Rect {
        height: shown as u16,
        ..inner
    };
    f.render_widget(Paragraph::new(lines[top..].to_vec()), body);
    let keys = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: 1,
        ..inner
    };
    f.render_widget(Paragraph::new(box_hint(READER_KEYS)), keys);
    if lines.len() > shown {
        vscrollbar(f, rect, lines.len(), top, shown);
    }
}
