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
        /// Which button is selected. Starts on No: a gate stands in front of
        /// something irreversible, so a reflex Enter must never fire it.
        yes: bool,
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
    // One row per field, then a blank and the key line.
    let rows = p.fields.len() as u16 + 2;
    let rect = centered(area, box_width(area.width), box_height(rows, area.height));
    f.render_widget(Clear, rect);
    // No leading blank: the box's own top padding is that row.
    let mut lines: Vec<Line> = Vec::new();
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
    lines.push(box_hint(&format!(
        "{CTRL_VIM_Y_MOVE} {Y_MOVE} move · {CTRL_VIM_X_MOVE} {X_MOVE} choose · enter next/submit · esc cancel"
    )));
    let para = Paragraph::new(lines)
        .block(box_block(Color::Cyan, &p.title))
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
                // scannable QR; point at the file/PNG instead. An alert: it has
                // already failed, and reading it costs nothing.
                let body = format!(
                    "a QR this size needs a {}x{} terminal.\n\nMaximise the window, or press E to write out/<name>.conf or its PNG instead.",
                    quad.0, quad.1
                );
                let width = box_width(full.width);
                let rows = wrapped_height(&body, box_inner_width(width)) as u16;
                let area = centered(full, width, box_height(rows + 2, full.height));
                f.render_widget(Clear, area);
                let mut lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
                lines.push(Line::raw(""));
                lines.push(box_hint("esc dismiss"));
                f.render_widget(
                    Paragraph::new(lines)
                        .block(box_block(Color::Yellow, "qr does not fit"))
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
            let width = box_width(full.width);
            let rows = wrapped_height(body, box_inner_width(width)) as u16;
            let area = centered(full, width, box_height(rows + 2, full.height));
            f.render_widget(Clear, area);
            let mut lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
            lines.push(Line::raw(""));
            lines.push(box_hint(&format!(
                "{VIM_Y_MOVE} {Y_MOVE} scroll · y copy · esc close"
            )));
            f.render_widget(
                Paragraph::new(lines)
                    .block(box_block(Color::Cyan, title))
                    .scroll((*scroll, 0)),
                area,
            );
        }
        Overlay::Confirm { prompt, yes, .. } => {
            // A gate: red, and it starts on No, so a reflex Enter is never the
            // key that deletes something.
            let full = f.area();
            let width = box_width(full.width);
            // The prompt, a blank, the buttons, a blank, the keys.
            let rows = wrapped_height(prompt, box_inner_width(width)) as u16 + 4;
            let area = centered(full, width, box_height(rows, full.height));
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(vec![
                    Line::raw(prompt.clone()),
                    Line::raw(""),
                    box_buttons(Color::Red, *yes),
                    Line::raw(""),
                    box_hint("h/l ←/→ move · enter select · y/n"),
                ])
                .wrap(Wrap { trim: false })
                .block(box_block(Color::Red, "are you sure?")),
                area,
            );
        }
        Overlay::Invalid { reason, .. } => {
            // An alert: it already happened, reading it costs nothing, so
            // yellow rather than the red that means something is at stake.
            let full = f.area();
            let body = format!("this config is not valid:\n\n{reason}");
            let width = box_width(full.width);
            let rows = wrapped_height(&body, box_inner_width(width)) as u16;
            let area = centered(full, width, box_height(rows + 2, full.height));
            f.render_widget(Clear, area);
            let mut lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
            lines.push(Line::raw(""));
            lines.push(box_hint("e ↵ correct · d esc discard"));
            f.render_widget(
                Paragraph::new(lines)
                    .block(box_block(Color::Yellow, "invalid config"))
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
            let full = f.area();
            let area = centered(
                full,
                box_width(full.width),
                box_height(items.len() as u16 + 2, full.height),
            );
            f.render_widget(Clear, area);
            let mut lines = lines;
            lines.push(Line::raw(""));
            lines.push(box_hint(&format!(
                "{VIM_Y_MOVE} {Y_MOVE} move · ↵ select · esc cancel"
            )));
            f.render_widget(
                Paragraph::new(lines).block(box_block(Color::Cyan, title)),
                area,
            );
        }
    }
}

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
