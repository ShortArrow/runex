//! Test-injectable environment façade.
//!
//! Several modules in `runex-core` need to know "where is the user's
//! home directory" or "what is `$XDG_CONFIG_HOME`" to compute paths
//! like `~/.bashrc`, `~/.config/runex/config.toml`, or
//! `~/AppData/Local/clink/runex.lua`. The natural answers come from
//! [`dirs::home_dir`] and [`std::env::var`] respectively. Both of
//! those are *process-global*: tests cannot give a single function
//! call a different `$HOME` without mutating the env (which races
//! against parallel tests) or relying on `dirs`'s platform-specific
//! fallback chain (which on Windows ignores `$HOME` entirely and
//! consults the Known Folders API).
//!
//! `HomeDirResolver` factors those two lookups into a trait so
//! tests can pass an in-memory implementation without touching
//! process state. Production code uses [`SystemHomeDir`], a
//! zero-sized adapter over `dirs` + `std::env`. Tests use
//! [`EnvHomeDir`] which closes over a `Fn(&str) -> Option<String>`
//! and resolves `home_dir` from `$HOME` (or `$USERPROFILE` on
//! Windows) inside the closure.
//!
//! ## Public API stability
//!
//! This trait is `pub` because it has to cross the `runex-core` →
//! `runex` boundary so the binary can build a context for handlers
//! to use. It is *not* part of any external semver guarantee — see
//! the crate-level docstring on [`crate`]. Phase C (which absorbs
//! this whole module into `runex/src/infra/env.rs`) will move the
//! trait without renaming it.

use std::path::PathBuf;

/// Look up the user's home directory and arbitrary environment
/// variables. The two lookups are bundled in one trait because every
/// caller in `runex-core` that wants one wants the other (config-
/// home resolution starts from `$XDG_CONFIG_HOME` and falls back to
/// `home_dir().join(".config")`, etc.).
pub trait HomeDirResolver: Send + Sync {
    /// The user's home directory. `None` when the platform cannot
    /// determine one (extremely rare in practice — empty `$HOME`
    /// with no `dirs` fallback). Callers that can't recover from
    /// `None` should fail rather than guess.
    fn home_dir(&self) -> Option<PathBuf>;

    /// Read an environment variable. Returns `None` for both
    /// "unset" and "set but empty" because every caller in this
    /// crate treats them the same way (the `xdg_config_home` chain
    /// rejects empty strings explicitly).
    fn env_var(&self, name: &str) -> Option<String>;
}

/// Production resolver. Defers to [`dirs::home_dir`] and
/// [`std::env::var`].
///
/// Zero-sized; `&SystemHomeDir` is a fine default for any function
/// that takes `&dyn HomeDirResolver`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemHomeDir;

impl HomeDirResolver for SystemHomeDir {
    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        match std::env::var(name) {
            Ok(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    }
}

/// Test resolver. Delegates every lookup to a closure.
///
/// `home_dir()` returns the result of `env("HOME")` first, falling
/// back to `env("USERPROFILE")` for Windows-style overrides. If
/// neither is set the closure decides — most tests preset both.
///
/// Constructed by `EnvHomeDir::new(|name| match name { ... })`. The
/// closure is `Send + Sync` so `&dyn HomeDirResolver` can cross
/// thread boundaries (none of the runex-core callers spawn threads
/// today, but the trait bound future-proofs the public API).
#[allow(dead_code)]
pub struct EnvHomeDir<F>
where
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    lookup: F,
}

impl<F> EnvHomeDir<F>
where
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    /// `#[allow(dead_code)]` because the only callers today are
    /// inside `cfg(test)` modules under `runex/src/{app,infra}/`.
    /// The struct + ctor stay on the API surface so future cmd-side
    /// callers (e.g. a test that drives `cmd::init` with a hermetic
    /// home dir) can pick them up without re-adding the type.
    #[allow(dead_code)]
    pub fn new(lookup: F) -> Self {
        Self { lookup }
    }
}

impl<F> HomeDirResolver for EnvHomeDir<F>
where
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    fn home_dir(&self) -> Option<PathBuf> {
        // Prefer $HOME (Unix and Windows-with-Git-Bash). Fall back
        // to $USERPROFILE for Windows-native callers. Both treated
        // as "unset" if empty (matches SystemHomeDir).
        self.env_var("HOME")
            .or_else(|| self.env_var("USERPROFILE"))
            .map(PathBuf::from)
    }

    fn env_var(&self, name: &str) -> Option<String> {
        match (self.lookup)(name) {
            Some(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    }
}

// ─── filesystem-shape resolvers ─────────────────────────────────────
//
// These three live here (not in `app::init` / `app::config`) because
// they are pure path arithmetic over the `HomeDirResolver` trait,
// with no Config or other app-layer concerns. Putting them in `infra`
// breaks the `app::doctor → infra::integration_check → app::init`
// cycle that the older Phase C layout had: integration_check needs
// to resolve rcfile paths, and reaching back into `app::init` for
// that did not actually express a real dependency on init logic.

use crate::domain::shell::Shell;

/// `XDG_CONFIG_HOME` if set, else `~/.config`.
///
/// `xdg_config_home_with` is the resolver-injectable variant; the
/// non-`_with` shorthand at the call site is `infra::env::
/// xdg_config_home_with(&SystemHomeDir)`.
pub(crate) fn xdg_config_home_with(env: &dyn HomeDirResolver) -> Option<PathBuf> {
    if let Some(p) = env.env_var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(p));
    }
    env.home_dir().map(|h| h.join(".config"))
}

/// The rc file path for a given shell (best-effort; may not exist
/// yet). For PowerShell, `$PROFILE` is a runtime variable and cannot
/// be resolved statically, so the conventional filesystem path is
/// used instead.
///
/// Production callers want the system home directory — pass
/// `&SystemHomeDir`. Tests should pass an [`EnvHomeDir`] resolver
/// for hermetic runs (no process-env mutation, no platform fallback
/// chain).
pub(crate) fn rc_file_for(shell: Shell, env: &dyn HomeDirResolver) -> Option<PathBuf> {
    let home = env.home_dir()?;
    match shell {
        Shell::Bash => Some(home.join(".bashrc")),
        Shell::Zsh => Some(home.join(".zshrc")),
        Shell::Pwsh => {
            let base = if cfg!(windows) {
                home.join("Documents").join("PowerShell")
            } else {
                home.join(".config").join("powershell")
            };
            Some(base.join("Microsoft.PowerShell_profile.ps1"))
        }
        Shell::Nu => {
            // Nu's `env.nu` lives under XDG_CONFIG_HOME (or
            // ~/.config). Both knobs come from the same resolver so
            // tests can drive them from one closure.
            let cfg = xdg_config_home_with(env).unwrap_or_else(|| home.join(".config"));
            Some(cfg.join("nushell").join("env.nu"))
        }
        Shell::Clink => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn map_resolver(map: HashMap<&'static str, &'static str>) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
        let owned: HashMap<String, String> = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        EnvHomeDir::new(move |name| owned.get(name).cloned())
    }

    #[test]
    fn env_home_dir_prefers_home_over_userprofile() {
        let r = map_resolver(HashMap::from([
            ("HOME", "/home/test"),
            ("USERPROFILE", r"C:\Users\test"),
        ]));
        assert_eq!(r.home_dir(), Some(PathBuf::from("/home/test")));
    }

    #[test]
    fn env_home_dir_falls_back_to_userprofile_when_home_unset() {
        let r = map_resolver(HashMap::from([
            ("USERPROFILE", r"C:\Users\test"),
        ]));
        assert_eq!(r.home_dir(), Some(PathBuf::from(r"C:\Users\test")));
    }

    #[test]
    fn env_home_dir_returns_none_when_neither_set() {
        let r = map_resolver(HashMap::new());
        assert_eq!(r.home_dir(), None);
    }

    #[test]
    fn env_home_dir_treats_empty_string_as_unset() {
        let r = map_resolver(HashMap::from([("HOME", ""), ("USERPROFILE", "")]));
        assert_eq!(r.home_dir(), None);
        assert_eq!(r.env_var("HOME"), None);
    }

    #[test]
    fn env_home_dir_env_var_returns_value_when_set() {
        let r = map_resolver(HashMap::from([("XDG_CONFIG_HOME", "/etc/xdg")]));
        assert_eq!(r.env_var("XDG_CONFIG_HOME"), Some("/etc/xdg".to_string()));
    }

    /// `xdg_config_home_with` was moved here from `app::config` in
    /// Phase D D3b along with the function itself. The three tests
    /// pin resolver-injectable behaviour without touching process
    /// env state.
    mod xdg_config_home_with_tests {
        use super::*;
        use std::collections::HashMap;

        #[test]
        fn honours_env_var() {
            let owned: HashMap<String, String> = HashMap::from([
                ("XDG_CONFIG_HOME".to_string(), "/test/xdg".to_string()),
            ]);
            let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
            assert_eq!(xdg_config_home_with(&env), Some(PathBuf::from("/test/xdg")));
        }

        #[test]
        fn falls_back_to_home_when_xdg_unset() {
            let owned: HashMap<String, String> = HashMap::from([
                ("HOME".to_string(), "/test/home".to_string()),
            ]);
            let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
            assert_eq!(
                xdg_config_home_with(&env),
                Some(PathBuf::from("/test/home/.config"))
            );
        }

        #[test]
        fn returns_none_when_neither_set() {
            let env = EnvHomeDir::new(|_| -> Option<String> { None });
            assert_eq!(xdg_config_home_with(&env), None);
        }
    }

    /// `rc_file_for` (formerly `app::init::rc_file_for_with`) was
    /// moved here in Phase D D1b. The tests came along.
    mod rc_file_for_tests {
        use super::*;
        use std::collections::HashMap;

        fn map_env(map: HashMap<&'static str, &'static str>) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
            let owned: HashMap<String, String> = map
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            EnvHomeDir::new(move |name| owned.get(name).cloned())
        }

        #[test]
        fn bash_uses_home_from_resolver() {
            let env = map_env(HashMap::from([("HOME", "/test/home")]));
            let p = rc_file_for(Shell::Bash, &env).expect("bash rcfile must resolve");
            assert_eq!(p, PathBuf::from("/test/home/.bashrc"));
        }

        #[test]
        fn zsh_uses_home_from_resolver() {
            let env = map_env(HashMap::from([("HOME", "/test/home")]));
            let p = rc_file_for(Shell::Zsh, &env).expect("zsh rcfile must resolve");
            assert_eq!(p, PathBuf::from("/test/home/.zshrc"));
        }

        #[test]
        fn nu_honours_xdg_config_home_from_resolver() {
            // When XDG_CONFIG_HOME is explicitly set, nu's env.nu
            // sits under it — not under $HOME/.config. This is the
            // codex-flagged failure mode: the production fallback
            // path silently ignored the env var on Windows.
            let env = map_env(HashMap::from([
                ("HOME", "/test/home"),
                ("XDG_CONFIG_HOME", "/test/xdg"),
            ]));
            let p = rc_file_for(Shell::Nu, &env).expect("nu rcfile must resolve");
            assert_eq!(p, PathBuf::from("/test/xdg/nushell/env.nu"));
        }

        #[test]
        fn nu_falls_back_to_home_config_when_xdg_unset() {
            let env = map_env(HashMap::from([("HOME", "/test/home")]));
            let p = rc_file_for(Shell::Nu, &env).expect("nu rcfile must resolve");
            assert_eq!(p, PathBuf::from("/test/home/.config/nushell/env.nu"));
        }

        #[test]
        fn clink_returns_none_regardless_of_resolver() {
            let env = map_env(HashMap::from([("HOME", "/test/home")]));
            assert_eq!(rc_file_for(Shell::Clink, &env), None);
        }

        #[test]
        fn returns_none_when_resolver_has_no_home() {
            let env = map_env(HashMap::new());
            assert_eq!(rc_file_for(Shell::Bash, &env), None);
            assert_eq!(rc_file_for(Shell::Zsh, &env), None);
        }

        /// Production-resolver smoke test: on any platform, the bash
        /// rcfile name must be `.bashrc`. We don't assert the parent
        /// directory (it's whatever `dirs::home_dir()` returns —
        /// platform-specific) but the leaf name is invariant.
        #[test]
        fn bash_rc_path_ends_with_bashrc_under_system_resolver() {
            if let Some(path) = rc_file_for(Shell::Bash, &SystemHomeDir) {
                assert!(path.to_str().unwrap().ends_with(".bashrc"));
            }
        }
    }
}
