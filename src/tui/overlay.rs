//! Centered overlays: the wizard, a QR, an inspect pane and the confirms.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::path::PathBuf;

use super::*;

/// A centered modal over the current tab: a QR to scan, or a scrollable text
/// pager (`Text`) showing a `.conf` - an interface's on disk, or a mesh node's
/// generated on the fly (even if it was never written out).
pub(super) enum Overlay {
    Qr {
        title: String,
        width: usize,
        dark: Vec<bool>,
    },
    Text {
        title: String,
        body: String,
        scroll: u16,
    },
    Confirm {
        prompt: String,
        action: ConfirmAction,
    },
    Menu {
        title: String,
        name: String,
        items: Vec<(String, ExportKind)>,
        idx: usize,
    },
    /// The saved buffer failed validation. Shows the reason and lets the user
    /// choose: correct (reopen the editor on the same content) or discard it.
    Invalid {
        reason: String,
        content: String,
        original: Option<PathBuf>,
        was_up: bool,
    },
}

/// What a `y`/Enter on a Confirm overlay carries out.
#[derive(Clone)]
pub(super) enum ConfirmAction {
    /// Remove a node from the mesh manifest.
    DeleteNode(String),
    /// Delete an interface `.conf` from disk (a `.bak` is kept first).
    DeleteIface(PathBuf),
}

/// Ways to export a node from the Export menu.
#[derive(Clone, Copy)]
pub(super) enum ExportKind {
    Conf,
    Install,
    Qr,
    Ansible,
}

pub(super) fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    let height = p.fields.len() as u16 + 6;
    let rect = centered_pct(area, 72, height);
    f.render_widget(Clear, rect);
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, field) in p.fields.iter().enumerate() {
        let active = i == p.idx;
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let arrow = Span::raw(if active { "▸ " } else { "  " });
        match &field.kind {
            FieldKind::Type(kind) => {
                let dim = Style::default().add_modifier(Modifier::DIM);
                let opt = |k: NodeKind| {
                    let st = if k == *kind {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
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
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
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
                    Span::styled(
                        format!(" {} ", options[*idx]),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                    )
                };
                let counter = if options.len() > 1 {
                    format!("  ({}/{})", idx + 1, options.len())
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    arrow,
                    Span::styled(format!("{}: ", field.label), label_style),
                    chosen,
                    Span::styled(counter, dim),
                    Span::styled(
                        if active && options.len() > 1 {
                            "   ←/→ choose"
                        } else {
                            ""
                        },
                        dim,
                    ),
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", p.title))
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}

pub(super) fn render_overlay(f: &mut Frame, ov: &Overlay) {
    match ov {
        Overlay::Qr { title, width, dark } => {
            const QZ: usize = 2; // light quiet zone around the code
            let n = width + 2 * QZ;
            let is_dark = |x: usize, y: usize| -> bool {
                x >= QZ
                    && y >= QZ
                    && x < width + QZ
                    && y < width + QZ
                    && dark[(y - QZ) * width + (x - QZ)]
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
                Paragraph::new(lines)
                    .block(Block::default().borders(Borders::ALL).title(title.clone())),
                area,
            );
        }
        Overlay::Text {
            title,
            body,
            scroll,
        } => {
            let full = f.area();
            let w = (body.lines().map(|l| l.chars().count()).max().unwrap_or(40) as u16 + 4)
                .min(full.width.saturating_sub(4))
                .max(24);
            let h = (body.lines().count() as u16 + 3)
                .min(full.height.saturating_sub(2))
                .max(6);
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
                Paragraph::new(prompt.clone()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                ),
                area,
            );
        }
        Overlay::Invalid { reason, .. } => {
            let full = f.area();
            let body = format!(
                "This config is not valid:\n\n{reason}\n\ne / ↵  correct (reopen editor)\nd / esc  discard"
            );
            let w = 64.min(full.width.saturating_sub(4)).max(28);
            let h = (body.lines().count() as u16 + 4)
                .min(full.height.saturating_sub(2))
                .max(7);
            let area = centered(full, w, h);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(body)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" invalid config ")
                            .border_style(Style::default().fg(Color::Yellow)),
                    )
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Overlay::Menu {
            title, items, idx, ..
        } => {
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, (label, _))| {
                    let active = i == *idx;
                    Line::from(vec![
                        Span::raw(if active { "▸ " } else { "  " }),
                        Span::styled(
                            label.clone(),
                            if active {
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().add_modifier(Modifier::DIM)
                            },
                        ),
                    ])
                })
                .collect();
            let w = items
                .iter()
                .map(|(l, _)| l.chars().count())
                .max()
                .unwrap_or(20) as u16
                + 6;
            let area = centered(
                f.area(),
                w.max(title.chars().count() as u16 + 2),
                items.len() as u16 + 2,
            );
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title.clone())
                        .title_bottom(
                            Line::from(format!("  {VIM_Y_MOVE} {Y_MOVE} · ↵ select · esc"))
                                .right_aligned(),
                        )
                        .border_style(Style::default().fg(Color::Cyan)),
                ),
                area,
            );
        }
    }
}

/// A rect centered in `area`, exactly `w`x`h` (capped to the screen).
/// Shrink a `.conf` for QR encoding without changing meaning: drop the alignment
/// padding around `=`, the client-irrelevant `ListenPort`, `[Peer]` name comments,
/// and blank lines. WireGuard parses `Key=Value` fine, so fewer bytes = smaller QR.
pub(super) fn compact_for_qr(config: &str) -> String {
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
pub(super) fn qr_half(n: usize, is_dark: &dyn Fn(usize, usize) -> bool) -> Vec<Line<'static>> {
    (0..n)
        .step_by(2)
        .map(|y| {
            Line::from(
                (0..n)
                    .map(|x| {
                        let fg = if is_dark(x, y) {
                            Color::Black
                        } else {
                            Color::White
                        };
                        let bg = if y + 1 < n && is_dark(x, y + 1) {
                            Color::Black
                        } else {
                            Color::White
                        };
                        Span::styled("▀", Style::default().fg(fg).bg(bg))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// QR as quadrant blocks: each cell packs a 2x2 module block (black on white), so
/// it's half the width of the half-block form - denser, at the cost of scan margin.
pub(super) fn qr_quad(n: usize, is_dark: &dyn Fn(usize, usize) -> bool) -> Vec<Line<'static>> {
    // Glyph per 2x2 pattern, bit order TL=1, TR=2, BL=4, BR=8.
    const G: [char; 16] = [
        ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
    ];
    let cell = Style::default().fg(Color::Black).bg(Color::White);
    (0..n)
        .step_by(2)
        .map(|y| {
            Line::from(
                (0..n)
                    .step_by(2)
                    .map(|x| {
                        let d = |dx: usize, dy: usize| {
                            x + dx < n && y + dy < n && is_dark(x + dx, y + dy)
                        };
                        let bits = d(0, 0) as usize
                            | (d(1, 0) as usize) << 1
                            | (d(0, 1) as usize) << 2
                            | (d(1, 1) as usize) << 3;
                        Span::styled(G[bits].to_string(), cell)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}
