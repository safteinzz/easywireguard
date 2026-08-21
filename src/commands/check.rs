//! `ewg check`: validate configs and exit non-zero if any is broken.

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::wg;

/// Validate each config with the same check the TUI's create/edit uses. Valid
/// files report `ok` on stdout; problems (or an unreadable file) go to stderr, and
/// any failure makes the whole run exit non-zero so it drops into a CI pipeline.
pub fn run(paths: &[PathBuf]) -> Result<()> {
    let mut all_ok = true;
    for path in paths {
        let label = path.display();
        match std::fs::read_to_string(path) {
            Ok(text) => match wg::validate_config(&text) {
                Ok(()) => println!("{label}: {}", "ok".green()),
                Err(e) => {
                    all_ok = false;
                    eprintln!("{label}: {}", e.to_string().red());
                }
            },
            Err(e) => {
                all_ok = false;
                eprintln!("{label}: {}", format!("can't read: {e}").red());
            }
        }
    }
    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}
