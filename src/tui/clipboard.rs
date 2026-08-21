//! Copying to the clipboard through OSC 52, which works over ssh and tmux
//! where a clipboard binary is not reachable.

use std::io::Write;
use std::process::{Command, Stdio};

use super::*;

/// Copy `text` to the clipboard, returning the method used. Prefers a system tool
/// (wl-copy/xclip/... - survives after ewg exits); falls back to an OSC 52 escape
/// so it still works with no tool installed and over SSH, if the terminal allows it.
pub(super) fn copy_clipboard(text: &str) -> Option<&'static str> {
    let tools: [(&str, &[&str]); 5] = [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
        ("clip.exe", &[]),
    ];
    for (bin, args) in tools {
        let Ok(mut child) = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Some(bin);
        }
    }
    // Fallback: ask the terminal to copy via OSC 52 (base64 payload).
    let mut out = stdout();
    if write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))
        .and_then(|_| out.flush())
        .is_ok()
    {
        return Some("osc52");
    }
    None
}

/// Minimal standard base64 (no deps) for the OSC 52 payload.
pub(super) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        s.push(T[(n >> 18 & 63) as usize] as char);
        s.push(T[(n >> 12 & 63) as usize] as char);
        s.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-rolled base64 (no dep) feeding the OSC 52 clipboard escape - a silent
    /// wrong-padding bug here would just produce a paste that looks fine and isn't.
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"WireGuard"), "V2lyZUd1YXJk");
    }
}
