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
}
