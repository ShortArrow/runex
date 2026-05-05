//! `runex init [shell] [--yes]` — seed the config file (idempotent)
//! and append the integration line to the user's shell rcfile (or
//! write the clink lua, for cmd.exe).
//!
//! The two integration installers live here because every safety
//! property (`OpenOptions::append`, `O_NOFOLLOW`, the symlink reject
//! on the clink path, the sibling-temp + rename atomic write, the
//! per-write user confirmation) is init-specific. None of it makes
//! sense in `util/` — and putting it there would mean other commands
//! could accidentally inherit the policy.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app::init as runex_init;
use crate::domain::sanitize::sanitize_for_display;
use crate::domain::shell::Shell;

use crate::resolve_config_opt;
use crate::util::prompt::{prompt_confirm, read_rc_content};
use crate::util::shell::detect_shell;
use crate::{CmdOutcome, CmdResult};

pub fn handle(config_path: PathBuf, shell_override: Option<&str>, yes: bool) -> CmdResult {
    let msg = format!("Create config at {}?", sanitize_for_display(&config_path.display().to_string()));
    if yes || prompt_confirm(&msg) {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)
        {
            Ok(mut f) => {
                f.write_all(runex_init::default_config_content().as_bytes())?;
                println!("Created: {}", sanitize_for_display(&config_path.display().to_string()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("Config already exists: {}", sanitize_for_display(&config_path.display().to_string()));
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        println!("Skipped config creation.");
    }

    let shell = if let Some(s) = shell_override {
        s.parse::<Shell>().map_err(|e: crate::domain::shell::ShellParseError| {
            Box::<dyn std::error::Error>::from(e.to_string())
        })?
    } else {
        detect_shell().unwrap_or_else(|| {
            eprintln!(
                "Could not detect shell. Defaulting to bash. \
                 Use `runex init <shell>` (e.g. `runex init pwsh`) to target a specific shell."
            );
            Shell::Bash
        })
    };

    let rc_path_for_next_steps = match shell {
        Shell::Clink => {
            install_clink_lua(yes, &config_path)?;
            None
        }
        _ => install_rcfile_integration(shell, yes)?,
    };

    println!();
    println!("{}", runex_init::next_steps_message(shell, rc_path_for_next_steps.as_deref()));
    Ok(CmdOutcome::Ok)
}

/// Append the integration block to the rcfile for `shell`. Returns the
/// rcfile path so the caller can show it in the Next-steps blurb.
///
/// Safety properties (documented in `docs/setup.md` for users):
///
/// - `OpenOptions::append` so existing rcfile content is **never**
///   overwritten — every byte we write goes after the file's current
///   end.
/// - `O_NOFOLLOW` on Unix so a symlink at `rc_path` doesn't redirect
///   the write to a different file.
/// - Idempotent: if `RUNEX_INIT_MARKER` is already present in the
///   file, we skip the append entirely (no duplicate blocks).
/// - User confirmation per write unless `--yes`.
fn install_rcfile_integration(shell: Shell, yes: bool) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let Some(rc_path) = crate::infra::env::rc_file_for(shell, &crate::infra::env::SystemHomeDir) else {
        println!(
            "Shell integration for {:?} must be added manually. \
             Run `runex export {:?}` for the script.",
            shell, shell
        );
        return Ok(None);
    };
    let existing = read_rc_content(&rc_path);
    if existing.contains(crate::infra::integration_check::RUNEX_INIT_MARKER) {
        println!(
            "Shell integration already present in {}",
            sanitize_for_display(&rc_path.display().to_string())
        );
        return Ok(Some(rc_path));
    }
    let msg = format!(
        "Append shell integration to {}?",
        sanitize_for_display(&rc_path.display().to_string())
    );
    if !(yes || prompt_confirm(&msg)) {
        println!("Skipped shell integration.");
        return Ok(Some(rc_path));
    }
    let line = runex_init::integration_line(shell, "runex");
    let block = format!("\n{}\n{}\n", crate::infra::integration_check::RUNEX_INIT_MARKER, line);
    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut open_opts = std::fs::OpenOptions::new();
    open_opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = open_opts.open(&rc_path)?;
    file.write_all(block.as_bytes())?;
    println!("Appended integration to {}", sanitize_for_display(&rc_path.display().to_string()));
    Ok(Some(rc_path))
}

/// Write the clink lua integration to the resolved install path.
///
/// Unlike the rcfile flow, clink's lua file is a *static copy* of the
/// `runex export clink` output. There's no marker block to detect, so
/// we compare full file content against what would be emitted now and
/// only ask before overwriting if the on-disk content has actually
/// drifted. Identical content is a no-op (silent OK).
///
/// `config_path` is consulted so the export reflects the user's
/// keybind / abbr config (clink's lua bakes a `RUNEX_BIN` reference,
/// not abbreviation tables, so the dependency is light — but still
/// correct to thread through).
fn install_clink_lua(yes: bool, config_path: &Path) -> CmdResult {
    use crate::infra::integration_check::{check_clink_lua_freshness, IntegrationCheck};

    // Compute the canonical export content for *this* runex binary.
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "runex".to_string());
    let (_path, config, _err) = resolve_config_opt(Some(config_path));
    let new_content = crate::domain::shell::export_script(Shell::Clink, &bin, config.as_ref());

    let install_path = runex_init::default_clink_lua_install_path();

    // Decide what to do based on what's already on disk at any of the
    // probe paths. We only write to `install_path`; the freshness check
    // is purely informational ("would this PR-style overwrite be a no-op?").
    let probe = check_clink_lua_freshness(
        &new_content,
        &crate::infra::integration_check::default_clink_lua_paths(),
    );
    match probe {
        IntegrationCheck::Ok { detail, .. } => {
            println!("clink integration already up-to-date ({detail}).");
            return Ok(CmdOutcome::Ok);
        }
        IntegrationCheck::Outdated { path, .. } => {
            let msg = format!(
                "clink lua at {} is out of date. Overwrite with the current export?",
                sanitize_for_display(&path.display().to_string())
            );
            if !(yes || prompt_confirm(&msg)) {
                println!("Skipped clink integration update.");
                return Ok(CmdOutcome::Ok);
            }
        }
        IntegrationCheck::Skipped { .. } | IntegrationCheck::Missing { .. } => {
            // No clink lua on disk yet; ask before creating it.
            let msg = format!(
                "Write clink integration to {}?",
                sanitize_for_display(&install_path.display().to_string())
            );
            if !(yes || prompt_confirm(&msg)) {
                println!("Skipped clink integration.");
                return Ok(CmdOutcome::Ok);
            }
        }
    }

    write_clink_lua_safely(&install_path, &new_content)?;
    println!(
        "Wrote clink integration to {}",
        sanitize_for_display(&install_path.display().to_string())
    );
    Ok(CmdOutcome::Ok)
}

/// Write `contents` to `install_path` with two safety properties the
/// previous `std::fs::write` call did not give us:
///
///   1. **Refuse to follow a symlink at `install_path`.** An attacker
///      who can place a symlink in the user's clink scripts directory
///      could otherwise redirect the write to any file the runex
///      process can write (same threat model as the rcfile path). The
///      check uses `symlink_metadata`, which on Windows also catches
///      directory junctions and other reparse points.
///   2. **Atomic replace via sibling temp + rename.** A crash partway
///      through `std::fs::write` would leave a half-written lua file
///      that clink would then load and fail to parse on the next cmd
///      window. Writing to a sibling temp first and renaming on
///      success gives the user either the old content or the new
///      content, never something between.
fn write_clink_lua_safely(install_path: &Path, contents: &str) -> CmdResult {
    let parent = install_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "clink lua install path has no parent directory: {}",
                sanitize_for_display(&install_path.display().to_string())
            ))
        })?;
    std::fs::create_dir_all(parent)?;

    if let Ok(meta) = std::fs::symlink_metadata(install_path) {
        if meta.file_type().is_symlink() {
            return Err(Box::<dyn std::error::Error>::from(format!(
                "refusing to write through a symlink at {}",
                sanitize_for_display(&install_path.display().to_string())
            )));
        }
    }

    let file_name = install_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "clink lua install path has no file name: {}",
                sanitize_for_display(&install_path.display().to_string())
            ))
        })?;
    let tmp_path = parent.join(format!(".{file_name}.runex.tmp"));
    // Best-effort cleanup of a stale temp from a previous crash.
    let _ = std::fs::remove_file(&tmp_path);

    let mut tmp_opts = std::fs::OpenOptions::new();
    tmp_opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        tmp_opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut tmp_file = tmp_opts.open(&tmp_path)?;
    tmp_file.write_all(contents.as_bytes())?;
    tmp_file.sync_all()?;
    drop(tmp_file);

    if let Err(e) = std::fs::rename(&tmp_path, install_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Box::new(e));
    }
    Ok(CmdOutcome::Ok)
}
