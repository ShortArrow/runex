//! `runex config <show|type|where>` — locate and inspect the active
//! config file (issue #15).
//!
//! Subcommand naming mirrors the OS commands users already know:
//! `where` prints the resolved path (where.exe / which), `type`
//! streams the file contents to stdout (type / cat), and `show`
//! opens the file with the OS-associated application (Start-Process
//! / open / xdg-open).
//!
//! All three resolve the config path exactly like `add` / `remove`:
//! the global `--config` flag wins, then `RUNEX_CONFIG`, then the
//! default `$XDG_CONFIG_HOME/runex/config.toml`. The resolution
//! happens in `main()`'s dispatch; handlers receive the final path.

use std::path::Path;

use crate::domain::sanitize::{sanitize_for_display, sanitize_multiline_for_display};
use crate::{CmdOutcome, CmdResult};

/// Print the resolved config path. The path is printed even when the
/// file does not exist — seeing where runex *would* look is the
/// diagnostic point — but the exit code flags the absence so scripts
/// can branch on it.
pub(crate) fn handle_where(config_path: &Path, json: bool) -> CmdResult {
    let exists = config_path.is_file();
    let display = sanitize_for_display(&config_path.display().to_string());
    if json {
        #[derive(serde::Serialize)]
        struct WhereJson<'a> {
            path: &'a str,
            exists: bool,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&WhereJson { path: &display, exists })?
        );
    } else {
        println!("{display}");
    }
    Ok(if exists { CmdOutcome::Ok } else { CmdOutcome::ExitCode(1) })
}

/// Stream the raw config file contents to stdout. Reuses the app
/// layer's safe read (symlink and size-cap policy) and strips
/// terminal-unsafe characters while preserving newlines and tabs.
pub(crate) fn handle_type(config_path: &Path) -> CmdResult {
    if !config_path.is_file() {
        eprintln!("{}", missing_config_message(config_path));
        return Ok(CmdOutcome::ExitCode(1));
    }
    let text = crate::app::config::read_config_text(config_path)?;
    print!("{}", sanitize_multiline_for_display(&text));
    Ok(CmdOutcome::Ok)
}

/// Open the config file with the OS-associated application.
pub(crate) fn handle_show(config_path: &Path) -> CmdResult {
    handle_show_with(config_path, open_with_associated_app)
}

/// Testable core of [`handle_show`]: the platform opener is injected
/// so tests can assert the missing-file guard fires before any
/// process is spawned, and that an existing file reaches the opener.
fn handle_show_with(
    config_path: &Path,
    opener: impl FnOnce(&Path) -> std::io::Result<()>,
) -> CmdResult {
    let display = sanitize_for_display(&config_path.display().to_string());
    if !config_path.is_file() {
        eprintln!("{}", missing_config_message(config_path));
        return Ok(CmdOutcome::ExitCode(1));
    }
    if let Err(e) = opener(config_path) {
        eprintln!("error: could not open {display}: {e} (open the file manually)");
        return Ok(CmdOutcome::ExitCode(1));
    }
    println!("Opened {display}");
    Ok(CmdOutcome::Ok)
}

fn missing_config_message(config_path: &Path) -> String {
    format!(
        "error: config file not found at {} (run `runex init` to create it)",
        sanitize_for_display(&config_path.display().to_string())
    )
}

/// Spawn the platform opener detached. Intentionally neither waits
/// for nor captures output from the child: the subprocess output-cap
/// rule applies to captured output, and the associated application
/// (typically an editor) may legitimately run for the whole session.
fn open_with_associated_app(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program).arg(path).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[test]
    fn show_invokes_opener_with_existing_config_path() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "version = 1\n").unwrap();

        let opened: RefCell<Option<PathBuf>> = RefCell::new(None);
        let outcome = handle_show_with(&cfg, |p| {
            *opened.borrow_mut() = Some(p.to_path_buf());
            Ok(())
        })
        .expect("handle_show_with must not Err");

        assert!(matches!(outcome, CmdOutcome::Ok));
        assert_eq!(opened.borrow().as_deref(), Some(cfg.as_path()));
    }

    #[test]
    fn show_missing_config_exits_1_without_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");

        let outcome = handle_show_with(&missing, |_| {
            panic!("opener must not run for a missing config");
        })
        .expect("handle_show_with must not Err");

        assert!(matches!(outcome, CmdOutcome::ExitCode(1)));
    }

    #[test]
    fn show_opener_failure_exits_1() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "version = 1\n").unwrap();

        let outcome = handle_show_with(&cfg, |_| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no opener"))
        })
        .expect("handle_show_with must not Err");

        assert!(matches!(outcome, CmdOutcome::ExitCode(1)));
    }

    #[test]
    fn where_flags_missing_file_via_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        let outcome = handle_where(&missing, false).unwrap();
        assert!(matches!(outcome, CmdOutcome::ExitCode(1)));

        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "version = 1\n").unwrap();
        let outcome = handle_where(&cfg, false).unwrap();
        assert!(matches!(outcome, CmdOutcome::Ok));
    }
}
