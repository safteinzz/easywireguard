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

/// The red `*` beside a required field's label, empty for the rest. Its colour
/// does not follow the focus: whether a field can be left blank is a property of
/// the field, not of where the cursor is.
fn required_star(field: &Field) -> Span<'static> {
    Span::styled(
        if field.required { "*" } else { "" },
        Style::default().fg(Color::Red),
    )
}

/// The column a wizard's values start in, measured from the label's first
/// character. Fixed rather than measured off whatever is on screen: a toggle
/// rebuilds the form, and a measured column would slide every value sideways
/// each time the longest label changed.
const LABEL_COL: usize = 14;
/// How far a wide form may push that column before the labels are the ones that
/// give way, so a single long label cannot shove every value off the box.
const LABEL_COL_MAX: usize = 22;

/// The columns a label cell eats: the label, its star, and the colon.
fn label_width(field: &Field) -> usize {
    field.label.chars().count() + usize::from(field.required) + 1
}

/// Where this form's values start.
fn value_column(fields: &[Field]) -> usize {
    let widest = fields.iter().map(label_width).max().unwrap_or(0) + 2;
    widest.clamp(LABEL_COL, LABEL_COL_MAX)
}

/// One option of a toggle, drawn like the house Yes/No buttons because it is
/// the same thing: the filled one is what the form will use, the other is dim.
fn chip(label: &str, picked: bool) -> Span<'static> {
    let style = if picked {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Span::styled(format!(" {label} "), style)
}

/// A form: labels in one column, values in another, and nothing that appears or
/// disappears with the cursor. Per-row key hints used to sit on whichever row
/// was focused, which made every row grow and shrink as you moved through the
/// form for the sake of repeating what the key line at the bottom already says.
pub(super) fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    let width = box_width(area.width);
    let mut hint = format!("{Y_MOVE} move · {X_MOVE} choose · ↵ next/submit · esc cancel");
    if p.fields.iter().any(|f| f.required) {
        hint.push_str(" · * required");
    }
    // The field block (a fixed height, so a toggle never resizes the box), then
    // a blank and the key line - which can wrap, so it is measured, not counted.
    let rows = p.body_rows() as u16 + 1 + wrapped_height(&hint, box_inner_width(width)) as u16;
    let rect = centered(area, width, box_height(rows, area.height));
    f.render_widget(Clear, rect);

    let dim = Style::default().add_modifier(Modifier::DIM);
    let col = value_column(&p.fields);
    // No leading blank: the box's own top padding is that row.
    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in p.fields.iter().enumerate() {
        // The key material is the second half of a create wizard; one blank row
        // is all the separation it needs.
        if i > 0 && matches!(field.kind, FieldKind::Key(_)) {
            lines.push(Line::raw(""));
        }
        let active = i == p.idx;
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let pad = col.saturating_sub(label_width(field)).max(1);
        let mut spans = vec![
            Span::raw(if active { "▸ " } else { "  " }),
            Span::styled(field.label.clone(), label_style),
            required_star(field),
            Span::styled(format!(":{}", " ".repeat(pad)), label_style),
        ];
        match &field.kind {
            FieldKind::Type(kind) => {
                spans.push(chip(NodeKind::Spoke.short(), *kind == NodeKind::Spoke));
                spans.push(Span::raw("  "));
                spans.push(chip(NodeKind::Hub.short(), *kind == NodeKind::Hub));
            }
            FieldKind::Key(src) => {
                spans.push(chip(
                    KeySource::Generate.short(),
                    *src == KeySource::Generate,
                ));
                spans.push(Span::raw("  "));
                spans.push(chip(KeySource::Paste.short(), *src == KeySource::Paste));
            }
            // A pick is one value out of a list, not a button: it is drawn as
            // the value it holds, with guillemets saying it can be stepped.
            FieldKind::Pick { options, idx } => match options.get(*idx) {
                None => spans.push(Span::styled(field.hint.clone(), dim)),
                Some(chosen) => {
                    let steps = options.len() > 1;
                    let mut style = Style::default().fg(Color::Cyan);
                    if active {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    spans.push(Span::styled(if steps { "‹ " } else { "" }, dim));
                    spans.push(Span::styled(chosen.clone(), style));
                    spans.push(Span::styled(if steps { " ›" } else { "" }, dim));
                }
            },
            FieldKind::Text => {
                if field.value.is_empty() {
                    spans.push(Span::raw(if active { "█ " } else { "" }));
                    spans.push(Span::styled(field.placeholder(), dim));
                } else {
                    spans.push(Span::raw(field.value.clone()));
                    spans.push(Span::raw(if active { "█" } else { "" }));
                }
            }
        }
        lines.push(Line::from(spans));
    }
    // Hold the block open to its reserved height, so flipping Spoke<->Hub moves
    // nothing but the fields themselves.
    while lines.len() < p.body_rows() {
        lines.push(Line::raw(""));
    }
    lines.push(Line::raw(""));
    lines.push(box_hint(&hint));
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
