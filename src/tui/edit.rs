//! Handing a config to `$EDITOR` and writing what comes back, with a `.bak`
//! kept whenever an existing file is replaced.

use anyhow::Result;
use ratatui::prelude::*;
use std::path::{Path, PathBuf};

use super::*;
use std::process::Command;

#[derive(Clone)]
pub(super) enum Action {
    AddNode,
    EditNode {
        original: String,
    },
    /// Write an edited interface config to `<name>.conf`. `content` is the buffer
    /// the user saved in `$EDITOR`; `original` is the file being edited (None on
    /// create); `was_up` guards a rename of a live interface.
    SaveConf {
        content: String,
        original: Option<PathBuf>,
        was_up: bool,
    },
}

/// A pending request to suspend the TUI, run `$EDITOR` on a temp file, and resume.
/// Built when `c`/`e` is pressed; consumed by the event loop, which owns the
/// terminal, then handed back to `App::editor_done`.
pub(super) struct EditorReq {
    pub(super) tmp: PathBuf,
    pub(super) original: Option<PathBuf>,
    pub(super) was_up: bool,
    /// Exactly what we wrote to `tmp` before opening the editor. If the buffer
    /// comes back identical, the user saved nothing (`:q!`, or `:wq` on the
    /// untouched seed), so we treat it as a cancel - and it's the guaranteed way
    /// out of the fix-and-resave loop when a config keeps failing validation.
    pub(super) seed: String,
}

/// Write a working `.conf` (private included) to `out/<name>.conf`, returning the
/// path for the status line. out/ is gitignored - the deliberate secret-file spot.
pub(super) fn write_conf(name: &str, cfg: &str) -> String {
    let path = format!("out/{name}.conf");
    match std::fs::create_dir_all("out").and_then(|_| std::fs::write(&path, cfg)) {
        Ok(()) => path,
        Err(e) => format!("(couldn't write out/: {e})"),
    }
}

/// The file stem of `path` (the interface name for a `<name>.conf`).
pub(super) fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// A temp file seeded with `content` for `$EDITOR` to open, with a `.conf`
/// extension so the editor highlights it. One per process is enough (edits are
/// sequential); the caller removes it once read back.
pub(super) fn write_temp(content: &str) -> std::io::Result<PathBuf> {
    let mut p = std::env::temp_dir();
    p.push(format!("ewg-edit-{}.conf", std::process::id()));
    std::fs::write(&p, content)?;
    Ok(p)
}

/// Back up `path` to `<path>.bak.<epoch>` if it exists, so a clobber or delete is
/// recoverable. Returns the backup path, or None when there was nothing to copy.
pub(super) fn backup(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut bak = path.as_os_str().to_os_string();
    bak.push(format!(".bak.{epoch}"));
    let bak = PathBuf::from(bak);
    std::fs::copy(path, &bak).ok().map(|_| bak)
}

/// Bring the interface at `path` up or down, returning a status line.
pub(super) fn act(path: Option<PathBuf>, up: bool) -> String {
    let Some(path) = path else {
        return "no interface selected".into();
    };
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    match if up { wg::up(&path) } else { wg::down(&path) } {
        Ok(()) => format!("{} {name}", if up { "brought up" } else { "took down" }),
        Err(e) => format!("error: {e}"),
    }
}

/// Suspend the TUI (restore the terminal so the editor owns it), run `$EDITOR`
/// (`$VISUAL`, then `$EDITOR`, then `vi`) on the temp file, then resume the TUI
/// and hand the result back to the app. Under sudo, `$EDITOR` may not survive the
/// re-exec, so `vi` is the floor; `sudo -E ewg` carries it through.
pub(super) fn run_editor<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    req: EditorReq,
) -> Result<()> {
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let status = Command::new(&editor).arg(&req.tmp).status();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    terminal.clear()?; // the editor scribbled over our screen; force a full redraw

    match status {
        Ok(_) => app.editor_done(req),
        Err(e) => {
            let _ = std::fs::remove_file(&req.tmp);
            app.set_status(format!(
                "couldn't launch editor `{}`: {e}",
                editor.to_string_lossy()
            ));
        }
    }
    Ok(())
}
