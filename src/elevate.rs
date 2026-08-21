//! Re-executing under sudo when the config directories are not readable.

use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;

/// Re-exec under sudo when a config dir isn't readable, so `ewg` "just works"
/// without `sudo $(which ewg)`. If every dir reads fine (root, or readable) we
/// proceed as-is; set `EWG_NO_SUDO=1` to never auto-elevate.
#[cfg(unix)]
pub(crate) fn elevate_for(dirs: &[PathBuf]) -> Result<()> {
    use std::io::ErrorKind;
    use std::os::unix::process::CommandExt;

    if std::env::var_os("EWG_NO_SUDO").is_some() {
        return Ok(());
    }
    let blocked = dirs.iter().find(
        |d| matches!(std::fs::read_dir(d), Err(e) if e.kind() == ErrorKind::PermissionDenied),
    );
    let Some(dir) = blocked else {
        return Ok(());
    };

    eprintln!(
        "easywireguard cannot access the config files in {} without root.\n\
         elevating with sudo (set EWG_NO_SUDO=1 to disable)...",
        dir.display()
    );
    let exe = std::env::current_exe().context("finding own executable path")?;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let err = std::process::Command::new("sudo")
        .arg("--")
        .arg(&exe)
        .args(args)
        .exec(); // replaces this process; only returns on failure
    anyhow::bail!(
        "could not elevate via sudo: {err} (try: sudo {})",
        exe.display()
    );
}

#[cfg(not(unix))]
pub(crate) fn elevate_for(_dirs: &[PathBuf]) -> Result<()> {
    Ok(())
}
