//! Registry of directories that hold `.conf` files, so `ewg` can find, list,
//! and toggle configs wherever they live - not just `/etc/wireguard`.
//!
//! Stored as TOML at `$EWG_REGISTRY`, else `$XDG_CONFIG_HOME/ewg/dirs.toml`,
//! else `~/.config/ewg/dirs.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub dirs: Vec<PathBuf>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing `{}`", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(e).with_context(|| format!("reading `{}`", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let text = toml::to_string(self).context("serializing registry")?;
        std::fs::write(path, text).with_context(|| format!("writing `{}`", path.display()))
    }

    /// Add a dir; returns false if it was already registered.
    pub fn add(&mut self, dir: PathBuf) -> bool {
        if self.dirs.contains(&dir) {
            false
        } else {
            self.dirs.push(dir);
            true
        }
    }

    /// Remove a dir; returns false if it wasn't registered.
    pub fn remove(&mut self, dir: &Path) -> bool {
        let before = self.dirs.len();
        self.dirs.retain(|d| d != dir);
        self.dirs.len() != before
    }

    /// Dirs to actually scan: the registry, or `[/etc/wireguard]` when empty so
    /// the tool works out of the box.
    pub fn effective(&self) -> Vec<PathBuf> {
        if self.dirs.is_empty() {
            vec![PathBuf::from(crate::wg::DEFAULT_DIR)]
        } else {
            self.dirs.clone()
        }
    }
}

/// Where the registry file lives.
pub fn default_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("EWG_REGISTRY") {
        return Ok(PathBuf::from(p));
    }
    let base = dirs::config_dir().context("cannot locate config dir: set $EWG_REGISTRY")?;
    Ok(base.join("ewg").join("dirs.toml"))
}

/// The dirs to operate on: a `--dir` override, else the registry (which falls
/// back to `/etc/wireguard` when empty).
pub(crate) fn resolve_dirs(cli_dir: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(dir) = cli_dir {
        return Ok(vec![dir]);
    }
    Ok(Registry::load(&default_path()?)?.effective())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_is_idempotent() {
        let mut r = Registry::default();
        assert!(r.add(PathBuf::from("/a")));
        assert!(!r.add(PathBuf::from("/a")), "second add is a no-op");
        assert_eq!(r.dirs.len(), 1);
    }

    #[test]
    fn remove_reports_whether_it_existed() {
        let mut r = Registry::default();
        r.add(PathBuf::from("/a"));
        assert!(r.remove(Path::new("/a")));
        assert!(!r.remove(Path::new("/a")), "removing again is false");
    }

    #[test]
    fn effective_falls_back_to_default_dir_when_empty() {
        assert_eq!(
            Registry::default().effective(),
            vec![PathBuf::from(crate::wg::DEFAULT_DIR)]
        );
    }

    #[test]
    fn save_then_load_roundtrips() {
        let f = tempfile::tempdir().unwrap();
        let path = f.path().join("dirs.toml");
        let mut r = Registry::default();
        r.add(PathBuf::from("/etc/wireguard"));
        r.add(PathBuf::from("/home/x/wg"));
        r.save(&path).unwrap();
        let back = Registry::load(&path).unwrap();
        assert_eq!(back.dirs, r.dirs);
    }

    #[test]
    fn load_missing_file_is_empty_not_error() {
        let r = Registry::load(Path::new("/no/such/file.toml")).unwrap();
        assert!(r.dirs.is_empty());
    }
}
