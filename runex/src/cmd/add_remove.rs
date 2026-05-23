//! `runex add <key> <expand>` and `runex remove <key>` — direct edits
//! to the config TOML.
//!
//! Phase G additions: after a successful config edit, both handlers
//! call `cmd::init::refresh_existing_caches` so any installed
//! Phase G integration cache files (`<XDG_CACHE_HOME>/runex/
//! integration.<ext>`) immediately reflect the new abbreviation
//! table on the next shell startup. Refresh is silent and best-
//! effort: a transient I/O error on one shell's cache emits a
//! stderr warning but does not affect the success of `add` /
//! `remove`. Shells the user has not run `runex init <shell>` for
//! have no cache to refresh and are skipped.

use std::path::Path;

use crate::domain::sanitize::sanitize_for_display;
use crate::infra::env::HomeDirResolver;

use crate::{CmdOutcome, CmdResult};

pub(crate) fn handle_add(
    config_path: &Path,
    key: &str,
    expand: &str,
    when_command_exists: Option<&[String]>,
    env: &dyn HomeDirResolver,
) -> CmdResult {
    crate::app::config::append_abbr_to_file(config_path, key, expand, when_command_exists)?;
    println!(
        "Added: {} -> {}",
        sanitize_for_display(key),
        sanitize_for_display(expand)
    );
    crate::cmd::init::refresh_existing_caches(config_path, env);
    Ok(CmdOutcome::Ok)
}

pub(crate) fn handle_remove(
    config_path: &Path,
    key: &str,
    env: &dyn HomeDirResolver,
) -> CmdResult {
    let removed = crate::app::config::remove_abbr_from_file(config_path, key)?;
    if removed > 0 {
        println!(
            "Removed {} rule(s) for '{}'",
            removed,
            sanitize_for_display(key)
        );
        // Only refresh caches when something actually changed; a
        // remove that found nothing is a no-op and the caches are
        // unchanged.
        crate::cmd::init::refresh_existing_caches(config_path, env);
    } else {
        println!("No rule found for '{}'", sanitize_for_display(key));
    }
    Ok(CmdOutcome::Ok)
}

#[cfg(test)]
mod tests {
    //! Pin the Phase G silent-cache-refresh contract: after a
    //! successful `add` / `remove`, every cache file the user has
    //! installed (`<XDG_CACHE_HOME>/runex/integration.<ext>`) is
    //! atomically rewritten to reflect the new abbreviation table.
    //! Shells the user has not run `runex init <shell>` for are
    //! skipped (no cache file present, nothing to refresh).

    use super::*;
    use crate::infra::env::EnvHomeDir;
    use crate::infra::integration_cache::cache_path;
    use crate::domain::shell::Shell;
    use std::collections::HashMap;

    fn env_with(home: &std::path::Path) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
        let cache = home.join(".cache");
        let owned: HashMap<String, String> = HashMap::from([
            ("HOME".to_string(), home.to_string_lossy().into_owned()),
            ("XDG_CACHE_HOME".to_string(), cache.to_string_lossy().into_owned()),
        ]);
        EnvHomeDir::new(move |n| owned.get(n).cloned())
    }

    fn write_seed_config(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("config.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    /// `handle_add` followed by an inspection of the bash cache:
    /// the new abbr key must appear in the regenerated body when a
    /// cache was already installed.
    #[test]
    fn handle_add_refreshes_existing_bash_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let env = env_with(home);
        let cfg = write_seed_config(
            home,
            "version = 1\n[keybind.trigger]\ndefault = \"space\"\n",
        );

        // Pre-create the bash cache (= simulate a prior `runex
        // init bash`). The contents don't matter; we only need
        // the file to exist so the refresh path runs.
        let bash_cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
        std::fs::create_dir_all(bash_cache.parent().unwrap()).unwrap();
        std::fs::write(&bash_cache, "stale").unwrap();

        handle_add(&cfg, "ggg", "git get", None, &env).expect("handle_add");

        // After add, the bash cache must have been rewritten with
        // the new abbreviation reachable inside it.
        let refreshed = std::fs::read_to_string(&bash_cache).unwrap();
        assert!(
            refreshed != "stale",
            "bash cache must be regenerated after handle_add"
        );
        assert!(
            refreshed.contains("runex-integration-version:"),
            "regenerated cache must contain the version header"
        );
    }

    /// Shells without an existing cache file must NOT be created
    /// by add/remove. Only `runex init <shell>` opts a shell into
    /// the cache layout.
    #[test]
    fn handle_add_does_not_create_caches_for_uninitialized_shells() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let env = env_with(home);
        let cfg = write_seed_config(
            home,
            "version = 1\n[keybind.trigger]\ndefault = \"space\"\n",
        );

        // No cache files pre-created.
        handle_add(&cfg, "ggg", "git get", None, &env).expect("handle_add");

        // None of the per-shell caches should exist now.
        for shell in [Shell::Bash, Shell::Zsh, Shell::Pwsh, Shell::Nu] {
            let p = cache_path(shell, &env).unwrap().unwrap();
            assert!(
                !p.exists(),
                "{:?} cache must not be auto-created by handle_add: {}",
                shell,
                p.display()
            );
        }
    }

    /// `handle_remove` returning zero hits must not touch caches:
    /// nothing changed in the config so refreshing would be wasted
    /// I/O (and might race with concurrent shells).
    #[test]
    fn handle_remove_zero_match_does_not_touch_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let env = env_with(home);
        let cfg = write_seed_config(
            home,
            "version = 1\n[keybind.trigger]\ndefault = \"space\"\n",
        );

        // Pre-existing bash cache with a known sentinel content.
        let bash_cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
        std::fs::create_dir_all(bash_cache.parent().unwrap()).unwrap();
        let sentinel = "this-is-the-original-cache-content";
        std::fs::write(&bash_cache, sentinel).unwrap();

        handle_remove(&cfg, "nonexistent-key", &env).expect("handle_remove");

        let after = std::fs::read_to_string(&bash_cache).unwrap();
        assert_eq!(
            after, sentinel,
            "no-op remove must leave the cache untouched"
        );
    }
}
