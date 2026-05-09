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
use crate::infra::env::HomeDirResolver;

use crate::resolve_config_opt;
use crate::util::prompt::{prompt_confirm, read_rc_content};
use crate::util::shell::detect_shell;
use crate::{CmdOutcome, CmdResult};

pub(crate) fn handle(
    config_path: PathBuf,
    shell_override: Option<&str>,
    yes: bool,
    env: &dyn HomeDirResolver,
) -> CmdResult {
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
            install_clink_lua(yes, &config_path, env)?;
            None
        }
        _ => install_rcfile_integration(shell, yes, env)?,
    };

    println!();
    println!("{}", runex_init::next_steps_message(shell, rc_path_for_next_steps.as_deref()));
    Ok(CmdOutcome::Ok)
}

/// Phase G shell integration installer for bash/zsh/pwsh/nu.
///
/// Two-phase install:
///
/// 1. **Write the static cache file** at
///    `<XDG_CACHE_HOME>/runex/integration.<ext>`. The cache file
///    contains the absolute `current_exe()` path baked into the hook
///    function so per-keystroke invocations (`bind -x` etc.) skip
///    PATH lookup entirely. Atomic write via sibling-temp + rename
///    in `infra::integration_cache::write_cache_file`.
///
/// 2. **Append a source line** to the user's rcfile pointing at the
///    cache. Idempotent via `RUNEX_INIT_MARKER`; the rcfile-write
///    safety properties (append-only, `O_NOFOLLOW`, user
///    confirmation) are unchanged from 0.1.14.
///
/// Returns the rcfile path for the Next-steps blurb. Cache write
/// happens unconditionally on `--yes` or after user confirmation
/// (the rcfile prompt covers both writes since they're
/// inseparable in the install flow).
fn install_rcfile_integration(
    shell: Shell,
    yes: bool,
    env: &dyn HomeDirResolver,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let Some(rc_path) = crate::infra::env::rc_file_for(shell, env) else {
        println!(
            "Shell integration for {:?} must be added manually. \
             Run `runex export {:?}` for the script.",
            shell, shell
        );
        return Ok(None);
    };

    // Resolve the cache file location for this shell. clink lands
    // here too via the outer `match`, but `cache_path` returns
    // `Ok(None)` for clink — we only reach this function for
    // bash/zsh/pwsh/nu, so unwrap is safe.
    let cache_path = crate::infra::integration_cache::cache_path(shell, env)?
        .expect("cache_path must return Some for non-clink shells");
    let cache_str = cache_path
        .to_str()
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "cache path is not valid UTF-8: {}",
                sanitize_for_display(&cache_path.display().to_string())
            ))
        })?
        .to_string();

    let existing = read_rc_content(&rc_path);
    let marker_present = existing.contains(crate::infra::integration_check::RUNEX_INIT_MARKER);

    let msg = if marker_present {
        format!(
            "Refresh shell integration cache at {}?",
            sanitize_for_display(&cache_path.display().to_string())
        )
    } else {
        format!(
            "Install shell integration (cache at {}, source line in {})?",
            sanitize_for_display(&cache_path.display().to_string()),
            sanitize_for_display(&rc_path.display().to_string())
        )
    };
    if !(yes || prompt_confirm(&msg)) {
        println!("Skipped shell integration.");
        return Ok(Some(rc_path));
    }

    // Write the cache file (idempotent: atomic replace, regenerates
    // even if marker is present so re-running picks up new templates
    // / config / runex binary location).
    let bin = crate::util::path::current_exe_or_default("runex");
    let (_path, config, _err) = resolve_config_opt(None);
    let comment_prefix = crate::infra::integration_cache::comment_prefix_for(shell);
    let header = crate::infra::integration_cache::cache_header(comment_prefix, &bin);
    let body = crate::app::shell_export::export_script(shell, &bin, config.as_ref());
    let cache_contents = format!("{header}{body}");
    crate::infra::integration_cache::write_cache_file(&cache_path, &cache_contents)?;
    println!(
        "Wrote integration cache to {}",
        sanitize_for_display(&cache_path.display().to_string())
    );

    // Append the source line to the rcfile (skipped if marker
    // already present — the cache refresh above is enough).
    if !marker_present {
        let line = runex_init::integration_line(shell, &cache_str);
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
        println!("Appended source line to {}", sanitize_for_display(&rc_path.display().to_string()));
    } else {
        println!(
            "Source line already present in {}",
            sanitize_for_display(&rc_path.display().to_string())
        );
    }

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
fn install_clink_lua(yes: bool, config_path: &Path, env: &dyn HomeDirResolver) -> CmdResult {
    use crate::infra::integration_check::{check_clink_lua_freshness, IntegrationCheck};

    // Compute the canonical export content for *this* runex binary.
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "runex".to_string());
    let (_path, config, _err) = resolve_config_opt(Some(config_path));
    let new_content = crate::app::shell_export::export_script(Shell::Clink, &bin, config.as_ref());

    let install_path = runex_init::clink_lua_install_path_with_resolver(env);

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

#[cfg(test)]
mod tests {
    //! Inline integration of `cmd::init::handle` with an
    //! [`EnvHomeDir`] resolver. Exercises the production code path
    //! end-to-end without touching the real filesystem outside the
    //! test's tempdir, and without mutating process env vars.
    //!
    //! Phase D D5 added these tests to prove the resolver is wired
    //! through *production* `handle`, not just the underlying app
    //! helpers. Before D5 the resolver only entered through `_with`
    //! suffix functions that the binary itself never called, so the
    //! cmd-level handler was effectively un-injected.

    use super::*;
    use crate::infra::env::EnvHomeDir;
    use std::collections::HashMap;

    fn env_with(home: &std::path::Path) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
        let owned: HashMap<String, String> = HashMap::from([(
            "HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )]);
        EnvHomeDir::new(move |n| owned.get(n).cloned())
    }

    #[test]
    fn handle_writes_bashrc_under_env_home_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let env = env_with(home);
        let cfg_path = home.join("config.toml");

        let outcome = handle(cfg_path.clone(), Some("bash"), true, &env)
            .expect("handle must succeed");
        assert!(matches!(outcome, CmdOutcome::Ok));

        assert!(cfg_path.is_file(), "config file must be created at {:?}", cfg_path);
        let bashrc = home.join(".bashrc");
        assert!(bashrc.is_file(), "bashrc must be created at {:?}", bashrc);
        let body = std::fs::read_to_string(&bashrc).unwrap();
        assert!(
            body.contains(crate::infra::integration_check::RUNEX_INIT_MARKER),
            "bashrc must contain the runex marker: {body}"
        );
    }

    #[test]
    fn handle_is_idempotent_for_rcfile_integration() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let env = env_with(home);
        let cfg_path = home.join("config.toml");

        handle(cfg_path.clone(), Some("zsh"), true, &env).expect("first handle");
        let zshrc = home.join(".zshrc");
        let first = std::fs::read_to_string(&zshrc).unwrap();

        handle(cfg_path.clone(), Some("zsh"), true, &env).expect("second handle");
        let second = std::fs::read_to_string(&zshrc).unwrap();

        assert_eq!(
            first, second,
            "rerunning handle must not duplicate the integration block"
        );
    }
}
