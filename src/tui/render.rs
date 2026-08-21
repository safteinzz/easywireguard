//! Drawing the frame: the tab bar, the list body and the status line.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};

use super::overlay::{render_overlay, render_prompt};
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

    if let Some(p) = &app.prompt {
        render_prompt(f, area, p);
    }
    if let Some(ov) = &app.overlay {
        render_overlay(f, ov);
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
            let list = List::new(items)
                .block(titled("Interfaces", app.ifaces.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.iface_state);
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
                    ListItem::new(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(tag, tag_style),
                        Span::raw(" "),
                        Span::styled(format!("{:w$}", n.name), bold),
                        Span::raw("  "),
                        Span::styled(
                            format!("{:16}", n.mesh_ip()),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw("  "),
                        Span::styled(tail, dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Mesh", app.nodes.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.node_state);
        }
    }
}

pub(super) fn render_status(f: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.live_status() {
        Some(msg) => (msg.to_string(), Style::default().fg(Color::Green)),
        None => (
            app.view.hints(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))),
        area,
    );
}
