//! `command_exists` factories used by `when_command_exists` rule
//! evaluation across every handler.
//!
//! Two variants for the same logic:
//! * [`make_command_exists`] borrows `path_prepend` for `'a` — used
//!   by tests that pin per-call lifetimes and don't want the closure
//!   to take ownership.
//! * [`make_command_exists_owned`] owns its inputs so the closure is
//!   `'static`-bounded, which is what [`crate::AppContext`] needs to
//!   carry it around without lifetime parameters.
//!
//! Both share identical command-resolution semantics; keeping the
//! single source of truth tucked behind two thin signatures avoided
//! a churning rewrite of the existing test suite when AppContext
//! landed in B2.

use std::path::{Path, PathBuf};

/// Build a `command_exists` closure with precache hint layer.
///
/// When `path_prepend` is `Some(dir)`, files inside `dir` are checked
/// first (bare name, and `.exe` on Windows). Falls through to
/// `which::which`.
///
/// Rejects any `cmd` containing `/`, `\`, or `:` because
/// `when_command_exists` values must be bare command names, not
/// filesystem paths. Accepting paths would allow directory traversal
/// and absolute-path probing via `dir.join(cmd)`.
///
/// ## Hint layer (precache)
///
/// If `RUNEX_CMD_CACHE_V1` env var contains a valid cache with
/// matching fingerprint:
/// - `cache[cmd] == true` → return true immediately (skip `which`)
/// - `cache[cmd] == false` → re-check live (avoid stale false
///   negatives after installs)
/// - `cmd` not in cache → live check
///
/// Results are also cached in a `RefCell<HashMap>` per invocation to
/// avoid repeated `which` calls within the same CLI run.
///
/// ## Windows-specific PATH augmentation
///
/// On Windows we feed `which::which_in` the *augmented* search path
/// from [`crate::win_path::effective_search_path`] (process PATH +
/// HKCU + HKLM) instead of relying on the inherited `PATH` env var
/// alone.
///
/// The reason is that some parent processes — most notably the
/// cmd.exe children that clink's Lua `io.popen` spawns — inherit a
/// PATH that's missing the User-scope entries the registry holds.
/// Without augmentation, `runex hook` running under clink would fail
/// to find binaries installed under `~/.cargo/bin`,
/// `~/AppData/Local/Microsoft/WinGet/Links`, or
/// `~/AppData/Local/mise/shims`. `when_command_exists` rules pointing
/// at those binaries would then silently evaluate false and
/// abbreviations would no-op — looking like an integration bug while
/// the real cause is environmental. The regression test
/// `runex/tests/windows_path_isolation.rs` pins this behavior so the
/// failure mode can't return unnoticed.
pub fn make_command_exists<'a>(
    path_prepend: Option<&'a Path>,
    precache_fingerprint: Option<&str>,
) -> impl Fn(&str) -> bool + 'a {
    use runex_core::precache;

    let hint = precache_fingerprint.and_then(precache::load_cache);
    let cache = std::cell::RefCell::new(std::collections::HashMap::<String, bool>::new());
    #[cfg(windows)]
    let effective_path = crate::win_path::effective_search_path();

    move |cmd: &str| -> bool {
        if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
            return false;
        }

        if let Some(&cached) = cache.borrow().get(cmd) {
            return cached;
        }

        if let Some(ref h) = hint {
            if let Some(&cached) = h.commands.get(cmd) {
                if cached {
                    cache.borrow_mut().insert(cmd.to_owned(), true);
                    return true;
                }
            }
        }

        let live_check = |c: &str| -> bool {
            #[cfg(windows)]
            {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                which::which_in(c, Some(&effective_path.combined), &cwd).is_ok()
            }
            #[cfg(not(windows))]
            {
                which::which(c).is_ok()
            }
        };

        let exists = if let Some(dir) = path_prepend {
            if dir.join(cmd).is_file() {
                true
            } else {
                #[cfg(windows)]
                {
                    if dir.join(format!("{cmd}.exe")).is_file() {
                        true
                    } else {
                        live_check(cmd)
                    }
                }
                #[cfg(not(windows))]
                {
                    live_check(cmd)
                }
            }
        } else {
            live_check(cmd)
        };

        cache.borrow_mut().insert(cmd.to_owned(), exists);
        exists
    }
}

/// Owning variant of [`make_command_exists`]. The non-owning version
/// is still used by tests that pin per-call lifetimes; this version
/// exists so [`crate::AppContext`] can hold a `'static`-bounded
/// closure.
pub fn make_command_exists_owned(
    path_prepend: Option<PathBuf>,
    precache_fingerprint: Option<String>,
) -> impl Fn(&str) -> bool + 'static {
    use runex_core::precache;

    let hint = precache_fingerprint
        .as_deref()
        .and_then(precache::load_cache);
    let cache = std::cell::RefCell::new(std::collections::HashMap::<String, bool>::new());
    #[cfg(windows)]
    let effective_path = crate::win_path::effective_search_path();

    move |cmd: &str| -> bool {
        if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
            return false;
        }

        if let Some(&cached) = cache.borrow().get(cmd) {
            return cached;
        }

        if let Some(ref h) = hint {
            if let Some(&cached) = h.commands.get(cmd) {
                if cached {
                    cache.borrow_mut().insert(cmd.to_owned(), true);
                    return true;
                }
            }
        }

        let live_check = |c: &str| -> bool {
            #[cfg(windows)]
            {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                which::which_in(c, Some(&effective_path.combined), &cwd).is_ok()
            }
            #[cfg(not(windows))]
            {
                which::which(c).is_ok()
            }
        };

        let exists = if let Some(dir) = path_prepend.as_deref() {
            #[cfg(windows)]
            let direct = dir.join(cmd).is_file()
                || dir.join(format!("{cmd}.exe")).is_file();
            #[cfg(not(windows))]
            let direct = dir.join(cmd).is_file();
            direct || live_check(cmd)
        } else {
            live_check(cmd)
        };

        cache.borrow_mut().insert(cmd.to_owned(), exists);
        exists
    }
}
