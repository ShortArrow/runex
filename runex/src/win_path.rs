//! Windows-specific PATH augmentation for command resolution.
//!
//! ## Why this module exists
//!
//! On Windows, child processes inherit the parent process's PATH at spawn
//! time; the OS does not re-derive PATH from the registry on each spawn.
//! Most of the time this is fine because the parent's PATH already includes
//! both Machine and User scopes (cmd.exe and Explorer compose them at
//! login).
//!
//! **Clink** breaks this assumption. Clink injects a DLL into a host
//! cmd.exe process, and lua scripts run inside that injected process call
//! `io.popen()` which spawns *another* cmd.exe with the host's PATH. If
//! the host process happens to have only the Machine PATH (for instance
//! when started by certain task hosts or terminal launchers), the
//! User-scope PATH never reaches the `runex hook` child — and binaries
//! installed under `~/.cargo/bin`, `~/AppData/Local/Microsoft/WinGet/Links`
//! or `~/AppData/Local/mise/shims` become invisible to `which::which`.
//!
//! When that happens, `when_command_exists = ["lsd"]` rules silently
//! evaluate false, `expand` returns `PassThrough`, and the user's
//! abbreviation never expands — looking for all the world like an
//! integration bug while the real cause is environmental.
//!
//! `effective_search_path()` papers over this by composing PATH from:
//!
//!   1. The process's own `PATH` env var (whatever was inherited).
//!   2. HKCU `Environment\Path` — the User-scope PATH the registry holds.
//!   3. HKLM `...\Session Manager\Environment\Path` — System-scope PATH.
//!
//! …deduplicated case-insensitively, with `%VAR%` references expanded
//! against the live process environment so entries like
//! `%LocalAppData%\Microsoft\WinGet\Links` resolve correctly.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// Result of merging process PATH with registry-scoped PATH entries.
///
/// `combined` is the joined `;`-separated string suitable for passing to
/// `which::which_in`. The breakdown counts let `runex doctor` report
/// "process=N, +user=M, +system=K" so a degraded PATH stands out.
#[derive(Debug, Clone)]
pub(crate) struct EffectiveSearchPath {
    pub combined: OsString,
    /// Number of unique entries originating from the process PATH.
    pub from_process: usize,
    /// Number of *additional* unique entries pulled in from HKCU.
    pub from_user_registry: usize,
    /// Number of *additional* unique entries pulled in from HKLM.
    pub from_system_registry: usize,
}

impl EffectiveSearchPath {
    /// Total count of unique entries in `combined`. Used by tests; the
    /// production `doctor` path goes through
    /// [`doctor::EffectiveSearchPathSummary::total`] which mirrors the
    /// same arithmetic on a Serialize-friendly summary type.
    #[cfg(test)]
    pub(crate) fn total(&self) -> usize {
        self.from_process + self.from_user_registry + self.from_system_registry
    }
}

/// Compose the effective search PATH from process + registry sources.
///
/// See module docs for the rationale. Always succeeds: if registry reads
/// fail (unlikely outside of locked-down environments), the corresponding
/// counts simply stay zero.
pub(crate) fn effective_search_path() -> EffectiveSearchPath {
    let mut combined = OsString::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();

    let mut from_process = 0usize;
    if let Some(p) = std::env::var_os("PATH") {
        for seg in split_path_env(&p) {
            if push_dedup(&seg, &mut combined, &mut seen) {
                from_process += 1;
            }
        }
    }

    let from_user_registry = absorb_registry_path(
        winreg::enums::HKEY_CURRENT_USER,
        "Environment",
        &mut combined,
        &mut seen,
    );
    let from_system_registry = absorb_registry_path(
        winreg::enums::HKEY_LOCAL_MACHINE,
        r"System\CurrentControlSet\Control\Session Manager\Environment",
        &mut combined,
        &mut seen,
    );

    EffectiveSearchPath {
        combined,
        from_process,
        from_user_registry,
        from_system_registry,
    }
}

/// Hard limit on the number of bytes read from a single registry
/// `Path` value. Anything beyond this is truncated at the last
/// `;`-separator that still fits, which means the trailing entry is
/// dropped rather than split mid-string. The cap exists to bound
/// `runex hook`'s per-keystroke cost when an attacker (or a runaway
/// installer) has stuffed an absurdly long PATH into HKCU/HKLM.
///
/// 64 KiB comfortably exceeds any realistic developer PATH (the
/// author's own ~7 KiB Windows PATH fits in well under 1 KiB after
/// dedup) while keeping the worst case linear-bounded.
const MAX_REGISTRY_PATH_BYTES: usize = 64 * 1024;

/// Hard limit on the number of `;`-separated entries pulled from a
/// single registry `Path` value. With both HKCU and HKLM contributing
/// 256 entries each, a `which::which_in` walk in the hot path still
/// stays microsecond-class. Past 256 the user has bigger problems
/// than runex.
const MAX_PATH_ENTRIES: usize = 256;

/// Read `Path` from the given registry hive, expand `%VAR%` references,
/// and append every novel segment to `combined`. Returns the count of
/// segments that were newly added (segments already in `seen` are
/// skipped, so the count reflects only this hive's contribution).
///
/// Bounded by [`MAX_REGISTRY_PATH_BYTES`] (read-side cap) and
/// [`MAX_PATH_ENTRIES`] (segment-count cap) so an oversized registry
/// value cannot turn `runex hook` into a per-keystroke CPU hog.
fn absorb_registry_path(
    hive: winreg::HKEY,
    subkey: &str,
    combined: &mut OsString,
    seen: &mut HashSet<Vec<u16>>,
) -> usize {
    let raw = match read_registry_path(hive, subkey) {
        Some(v) => v,
        None => return 0,
    };
    absorb_registry_path_str(&raw, combined, seen)
}

/// Pure variant of [`absorb_registry_path`] that accepts the raw
/// registry value as a string. Exposed for tests.
pub(crate) fn absorb_registry_path_str(
    raw: &str,
    combined: &mut OsString,
    seen: &mut HashSet<Vec<u16>>,
) -> usize {
    let mut added = 0usize;
    let mut entries_taken = 0usize;
    for seg in raw.split(';') {
        if entries_taken >= MAX_PATH_ENTRIES {
            break;
        }
        entries_taken += 1;
        let expanded = expand_env_vars(seg);
        if push_dedup(&expanded, combined, seen) {
            added += 1;
        }
    }
    added
}

fn read_registry_path(hive: winreg::HKEY, subkey: &str) -> Option<String> {
    let key = winreg::RegKey::predef(hive).open_subkey(subkey).ok()?;
    let v: String = key.get_value("Path").ok()?;
    Some(cap_registry_value(v))
}

/// Truncate an oversized registry `Path` value to the last `;`-
/// separator that still fits within [`MAX_REGISTRY_PATH_BYTES`].
/// Returning `None` for empty input keeps callers' option-chaining
/// idiomatic.
pub(crate) fn cap_registry_value(v: String) -> String {
    if v.is_empty() {
        return v;
    }
    if v.len() <= MAX_REGISTRY_PATH_BYTES {
        return v;
    }
    // Truncate at the last ';' boundary that fits. If there isn't
    // one within the cap, drop everything (the value is one giant
    // entry, which we can't safely include).
    match v[..MAX_REGISTRY_PATH_BYTES].rfind(';') {
        Some(end) => v[..end].to_string(),
        None => String::new(),
    }
}

/// Append `seg` to `dst` (using `;` as a separator) if its case-insensitive
/// form has not been seen yet. Returns `true` when the segment was actually
/// inserted (so callers can count contributions).
fn push_dedup(seg: &OsStr, dst: &mut OsString, seen: &mut HashSet<Vec<u16>>) -> bool {
    if seg.is_empty() {
        return false;
    }
    // Case-insensitive dedup key. ASCII-lowercase is sufficient for path
    // comparisons on Windows: the OS itself does not Unicode-case-fold paths
    // when comparing, so two registry-style entries differing only by
    // letter case point at the same directory.
    let key: Vec<u16> = seg
        .encode_wide()
        .map(|c| if c < 128 { (c as u8).to_ascii_lowercase() as u16 } else { c })
        .collect();
    if seen.insert(key) {
        if !dst.is_empty() {
            dst.push(";");
        }
        dst.push(seg);
        true
    } else {
        false
    }
}

/// Split a `PATH` value (`OsStr`) on the Windows `;` separator without
/// going through UTF-8 (which can lose data for non-ASCII filenames).
fn split_path_env(p: &OsStr) -> Vec<OsString> {
    let wide: Vec<u16> = p.encode_wide().collect();
    let mut out = Vec::new();
    let mut start = 0;
    for (i, w) in wide.iter().enumerate() {
        if *w == b';' as u16 {
            out.push(OsString::from_wide(&wide[start..i]));
            start = i + 1;
        }
    }
    out.push(OsString::from_wide(&wide[start..]));
    out
}

/// Best-effort `%VAR%` expansion against the live process environment.
///
/// We do this manually rather than calling `ExpandEnvironmentStringsW`
/// to avoid taking on a `windows-sys` dependency for one function. The
/// substitution is deliberately conservative: `%FOO%` only expands when
/// `FOO` is actually set, otherwise the literal `%FOO%` is preserved so
/// the entry is at least visible in `doctor` output even if it can't be
/// resolved.
fn expand_env_vars(s: &str) -> OsString {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let var = &after[..end];
            if let Ok(val) = std::env::var(var) {
                out.push_str(&val);
            } else {
                out.push('%');
                out.push_str(var);
                out.push('%');
            }
            rest = &after[end + 1..];
        } else {
            out.push('%');
            rest = after;
            break;
        }
    }
    out.push_str(rest);
    OsString::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty segments (`;;` or trailing `;`) must not be added.
    #[test]
    fn push_dedup_skips_empty() {
        let mut combined = OsString::new();
        let mut seen = HashSet::new();
        assert!(!push_dedup(OsStr::new(""), &mut combined, &mut seen));
        assert!(combined.is_empty());
    }

    /// First insertion succeeds; second insertion of an
    /// case-insensitively-equal segment is skipped.
    #[test]
    fn push_dedup_is_case_insensitive() {
        let mut combined = OsString::new();
        let mut seen = HashSet::new();
        assert!(push_dedup(OsStr::new(r"C:\Foo\Bar"), &mut combined, &mut seen));
        assert!(!push_dedup(OsStr::new(r"c:\foo\bar"), &mut combined, &mut seen));
        assert_eq!(combined, OsString::from(r"C:\Foo\Bar"));
    }

    /// `%VAR%` references that resolve must expand; unknown vars must
    /// stay as-is so they're visible in diagnostic output.
    #[test]
    fn expand_env_vars_expands_known_and_preserves_unknown() {
        // SAFETY: tests run in serial within this module's test binary
        // and we restore the env var.
        unsafe { std::env::set_var("__RUNEX_TEST_VAR", "REPLACED"); }
        let r = expand_env_vars(r"%__RUNEX_TEST_VAR%\sub");
        unsafe { std::env::remove_var("__RUNEX_TEST_VAR"); }
        assert_eq!(r, OsString::from(r"REPLACED\sub"));

        let r2 = expand_env_vars(r"%__RUNEX_DEFINITELY_NOT_SET_VAR%\x");
        assert_eq!(r2, OsString::from(r"%__RUNEX_DEFINITELY_NOT_SET_VAR%\x"));
    }

    /// `effective_search_path()` is a real Windows API call; the only
    /// thing we can reliably assert at unit-test scope is that it
    /// terminates and the breakdown counts add up to a non-empty result
    /// on a normal Rust dev box (where PATH is non-empty).
    #[test]
    fn effective_search_path_runs_and_counts_consistently() {
        let p = effective_search_path();
        assert_eq!(
            p.from_process + p.from_user_registry + p.from_system_registry,
            p.total()
        );
        // On any Windows machine running this test, PATH must have at
        // least one entry — the test runner itself comes from PATH.
        assert!(p.total() > 0, "effective_search_path should never be empty in practice");
    }

    /// Reading an oversized registry `Path` value must truncate at a
    /// `;` boundary, not split mid-segment, so the worst-case
    /// per-keystroke cost stays linear-bounded.
    #[test]
    fn cap_registry_value_truncates_at_semicolon_boundary() {
        // Build a value with two entries:
        //   - leading 70 KiB of 'a' (well past the 64 KiB cap)
        //   - then a tail entry that would survive if we naively
        //     truncated at exactly the byte cap
        let leading = "a".repeat(70 * 1024);
        let raw = format!("{leading};C:\\fits");
        let capped = cap_registry_value(raw.clone());
        assert!(capped.len() <= 64 * 1024, "cap must hold");
        assert!(
            !capped.contains(';'),
            "with no semicolon under the cap on the leading run, the tail must be dropped: {}",
            capped.len()
        );
    }

    /// A registry value within the cap is returned unchanged.
    #[test]
    fn cap_registry_value_passes_small_input_through() {
        let raw = "C:\\Windows;C:\\Users\\me\\.cargo\\bin".to_string();
        assert_eq!(cap_registry_value(raw.clone()), raw);
    }

    /// `absorb_registry_path_str` stops after `MAX_PATH_ENTRIES`
    /// segments, even when more are present.
    #[test]
    fn absorb_registry_path_str_caps_entry_count() {
        // 300 unique entries; only the first 256 should be absorbed.
        let raw: String = (0..300)
            .map(|i| format!("C:\\fake{i}"))
            .collect::<Vec<_>>()
            .join(";");
        let mut combined = OsString::new();
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        let added = absorb_registry_path_str(&raw, &mut combined, &mut seen);
        assert_eq!(added, 256, "must stop at MAX_PATH_ENTRIES");
    }
}
