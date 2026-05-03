use std::path::PathBuf;

use crate::config::xdg_config_home;
use crate::sanitize::{double_quote_escape, is_nu_drop_char};
use crate::shell::{bash_quote_string, lua_quote_string, nu_quote_string, pwsh_quote_string, Shell};

/// Quote a filesystem path for embedding in a Nu shell string literal.
///
/// Uses Nu double-quoted string syntax (no `^` prefix — this is a string value,
/// not a command invocation).  Escapes `\`, `"`, and `$` (to suppress variable
/// interpolation of `$env.FOO` etc.).  NUL, DEL, ASCII control characters, Unicode
/// line/paragraph separators, and visual-deception characters are all dropped.
fn nu_quote_path(path: &str) -> String {
    let mut out = String::from("\"");
    for ch in path.chars() {
        if let Some(esc) = double_quote_escape(ch) {
            out.push_str(esc);
        } else if ch == '$' {
            out.push_str("\\$");
        } else if is_nu_drop_char(ch) {
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

/// Marker comment written into rc files to enable idempotent init.
pub const RUNEX_INIT_MARKER: &str = "# runex-init";

/// Seed config written by `runex init` when no config exists yet.
///
/// Includes a working trigger key and one runnable sample abbreviation
/// so the user can verify expansion immediately after a fresh install
/// without first having to read the docs. The codex usability review
/// flagged "installed but nothing happens" as the second-most painful
/// onboarding break, so the seed is deliberately *useful* rather than
/// minimal.
///
/// `runex init` only writes this content when the config file does not
/// already exist (`OpenOptions::create_new`). Existing configs are
/// never touched.
pub fn default_config_content() -> &'static str {
    r#"version = 1

[keybind.trigger]
default = "space"

# Sample abbreviation. After restarting your shell, type `gst<Space>`
# and it will expand to `git status `.
[[abbr]]
key    = "gst"
expand = "git status"

# Add your own below. For more recipes (per-shell commands, fallback
# chains, cursor placeholders, etc.) see:
# https://github.com/ShortArrow/runex/blob/main/docs/recipes.md
"#
}

/// The single line appended to the shell rc file.
///
/// ## Drift resistance
///
/// For bash/zsh/pwsh/nu the line either *re-evaluates* the export at
/// every shell start (`eval "$(runex export ...)"`,
/// `Invoke-Expression (& runex export pwsh ...)`) or re-writes the
/// shell-side script every start (nu's `save --force`). That makes the
/// integration **drift-proof**: upgrading runex automatically picks up
/// the latest template the next time the shell starts.
///
/// **clink is the exception.** The lua file lives outside any rcfile
/// reload pathway, so users have to re-run `runex init clink` after a
/// `runex` upgrade. `runex doctor` flags this drift via the
/// `integration:clink` check (see
/// [`crate::integration_check::check_clink_lua_freshness`]).
pub fn integration_line(shell: Shell, bin: &str) -> String {
    match shell {
        Shell::Bash => format!("eval \"$({} export bash)\"", bash_quote_string(bin)),
        Shell::Zsh => format!("eval \"$({} export zsh)\"", bash_quote_string(bin)),
        Shell::Pwsh => format!(
            "Invoke-Expression (& {} export pwsh | Out-String)",
            pwsh_quote_string(bin)
        ),
        Shell::Nu => {
            let cfg_dir = xdg_config_home()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config".to_string());
            let nu_bin = nu_quote_string(bin);
            let nu_path = nu_quote_path(&format!("{cfg_dir}/runex/runex.nu"));
            format!(
                "{nu_bin} export nu | save --force {nu_path}\nsource {nu_path}"
            )
        }
        Shell::Clink => format!(
            "-- add {} export clink output to your clink scripts directory",
            lua_quote_string(bin)
        ),
    }
}

/// Where `runex init clink` writes the lua integration script.
///
/// Resolution order (first match wins):
///
/// 1. `RUNEX_CLINK_LUA_PATH` env — explicit override for non-standard
///    clink installations or for testing.
/// 2. `%LOCALAPPDATA%\clink\runex.lua` — clink's default state directory
///    on Windows. This is what `clink info` reports as the scripts dir.
/// 3. `~/.local/share/clink/runex.lua` — POSIX-style fallback for the
///    Linux clink fork (rare, included for completeness).
///
/// We deliberately do not shell out to `clink info` to discover the
/// scripts directory: that would invert the dependency direction
/// (Rust → shell tool) for one path lookup, and the env-var override
/// already lets users with non-standard installs cope.
pub fn default_clink_lua_install_path() -> std::path::PathBuf {
    clink_lua_install_path_with(|k| std::env::var(k).ok(), dirs::home_dir)
}

/// Pure variant of [`default_clink_lua_install_path`] for testing —
/// env access and the home-dir lookup are injected so tests can pin
/// the result without racing other threads on `std::env::set_var`.
pub(crate) fn clink_lua_install_path_with<E, H>(env_get: E, home_dir: H) -> std::path::PathBuf
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<std::path::PathBuf>,
{
    if let Some(p) = env_get("RUNEX_CLINK_LUA_PATH") {
        if !p.is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    if let Some(local) = env_get("LOCALAPPDATA") {
        if !local.is_empty() {
            return std::path::PathBuf::from(local).join("clink").join("runex.lua");
        }
    }
    if let Some(home) = home_dir() {
        return home.join(".local").join("share").join("clink").join("runex.lua");
    }
    std::path::PathBuf::from("runex.lua")
}

/// "What to do next" blurb shown after `runex init` finishes. The
/// integration line lives in the rcfile but the *currently-running*
/// shell hasn't sourced it yet, so the user has to either reload the
/// rcfile or open a fresh shell. Each shell has its own idiomatic
/// reload command; clink keeps no rcfile and just needs a new cmd.
///
/// `rc_path` is the file we just appended to (or `None` for clink, where
/// the integration goes into a separate lua file rather than an rcfile).
pub fn next_steps_message(shell: Shell, rc_path: Option<&std::path::Path>) -> String {
    let reload = match shell {
        Shell::Bash | Shell::Zsh => match rc_path {
            Some(p) => format!("Reload your shell: `source {}` (or `exec $SHELL`)", p.display()),
            None => "Reload your shell: `exec $SHELL`".to_string(),
        },
        Shell::Pwsh => match rc_path {
            Some(p) => format!("Reload your profile: `. $PROFILE` (resolves to {})", p.display()),
            None => "Reload your profile: `. $PROFILE`".to_string(),
        },
        Shell::Nu => "Reload nushell: open a new shell (or run `exec nu`)".to_string(),
        Shell::Clink => "Open a new cmd window — clink loads the lua at startup.".to_string(),
    };
    format!(
        "Next steps:\n  1. {reload}\n  2. Try `gst<Space>` — it should expand to `git status `.\n  3. Add your own abbreviations: see https://github.com/ShortArrow/runex/blob/main/docs/recipes.md\n  4. Verify any time with: `runex doctor`"
    )
}

/// The rc file path for a given shell (best-effort; may not exist yet).
///
/// For PowerShell, `$PROFILE` is a runtime variable and cannot be resolved statically,
/// so the conventional filesystem path is used instead.
pub fn rc_file_for(shell: Shell) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
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
            let cfg = xdg_config_home().unwrap_or_else(|| home.join(".config"));
            Some(cfg.join("nushell").join("env.nu"))
        }
        Shell::Clink => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod integration_line {
        use super::*;

    #[test]
    fn default_config_content_has_version() {
        assert!(default_config_content().contains("version = 1"));
    }

    /// The seed config must include a working keybind so that the very
    /// first `runex init` produces a setup that actually expands. Without
    /// this, users hit "I installed runex and nothing happens" — the
    /// codex usability review flagged this as the second-most painful
    /// onboarding break after the missing `init <shell>` surface.
    #[test]
    fn default_config_content_includes_default_trigger() {
        let s = default_config_content();
        assert!(s.contains("[keybind.trigger]"), "missing [keybind.trigger]: {s}");
        assert!(s.contains("default = \"space\""), "missing default trigger: {s}");
    }

    /// The seed config must include at least one runnable abbreviation so
    /// the user can verify expansion immediately after `runex init`.
    #[test]
    fn default_config_content_includes_sample_abbr_gst() {
        let s = default_config_content();
        assert!(s.contains("key    = \"gst\""), "missing gst sample: {s}");
        assert!(s.contains("expand = \"git status\""), "missing gst expand: {s}");
    }

    /// `next_steps_message` produces the after-init "what to do next"
    /// blurb. Each shell's blurb has to mention how to *reload* the
    /// integration (since rcfile changes don't take effect in the
    /// already-running shell), how to find more abbreviations, and how
    /// to verify with `runex doctor`.
    #[test]
    fn next_steps_for_bash_mentions_source_command() {
        let msg = next_steps_message(Shell::Bash, Some(std::path::Path::new("/home/u/.bashrc")));
        assert!(msg.contains("source /home/u/.bashrc") || msg.contains("exec"),
            "bash next_steps must explain how to reload: {msg}");
        assert!(msg.contains("runex doctor"), "must suggest doctor: {msg}");
        assert!(msg.contains("recipes"), "must point at recipes: {msg}");
    }

    #[test]
    fn next_steps_for_clink_mentions_new_cmd_window() {
        let msg = next_steps_message(Shell::Clink, None);
        assert!(msg.to_lowercase().contains("cmd"),
            "clink next_steps must mention opening a new cmd window: {msg}");
        assert!(msg.contains("runex doctor"), "must suggest doctor: {msg}");
    }

    #[test]
    fn next_steps_for_pwsh_mentions_dot_profile() {
        let msg = next_steps_message(
            Shell::Pwsh,
            Some(std::path::Path::new("/u/Microsoft.PowerShell_profile.ps1")),
        );
        assert!(msg.contains("$PROFILE") || msg.contains(". /"),
            "pwsh next_steps must explain reload: {msg}");
    }

    /// `clink_lua_install_path_with` decides where `runex init clink`
    /// writes the lua file. Honours `RUNEX_CLINK_LUA_PATH` first, then
    /// `LOCALAPPDATA` (Windows convention), then a POSIX-style fallback
    /// for clink forks on Linux. Tests use the closure-injected variant
    /// to avoid racing on the global env from parallel test threads.
    #[test]
    fn clink_install_path_honors_env_override() {
        let p = clink_lua_install_path_with(
            |k| match k {
                "RUNEX_CLINK_LUA_PATH" => Some("/tmp/runex_test_clink.lua".into()),
                _ => None,
            },
            || None,
        );
        assert_eq!(p, std::path::PathBuf::from("/tmp/runex_test_clink.lua"));
    }

    #[test]
    fn clink_install_path_uses_localappdata_when_set() {
        let p = clink_lua_install_path_with(
            |k| match k {
                "LOCALAPPDATA" => Some("/tmp/local_appdata_test".into()),
                _ => None,
            },
            || None,
        );
        assert_eq!(
            p,
            std::path::PathBuf::from("/tmp/local_appdata_test/clink/runex.lua")
        );
    }

    #[test]
    fn clink_install_path_falls_back_to_home() {
        let p = clink_lua_install_path_with(
            |_| None,
            || Some(std::path::PathBuf::from("/home/user")),
        );
        assert_eq!(
            p,
            std::path::PathBuf::from("/home/user/.local/share/clink/runex.lua")
        );
    }

    /// An empty env var must be ignored (treated as if unset). Otherwise
    /// `RUNEX_CLINK_LUA_PATH=` would silently anchor writes to "" which
    /// either fails outright or hits an unintended cwd.
    #[test]
    fn clink_install_path_treats_empty_env_as_unset() {
        let p = clink_lua_install_path_with(
            |k| match k {
                "RUNEX_CLINK_LUA_PATH" | "LOCALAPPDATA" => Some(String::new()),
                _ => None,
            },
            || Some(std::path::PathBuf::from("/home/u")),
        );
        assert!(p.starts_with("/home/u"), "expected home fallback, got {p:?}");
    }

    #[test]
    fn integration_line_bash() {
        assert_eq!(
            integration_line(Shell::Bash, "runex"),
            r#"eval "$('runex' export bash)""#
        );
    }

    #[test]
    fn integration_line_pwsh() {
        let line = integration_line(Shell::Pwsh, "runex");
        assert!(line.contains("Invoke-Expression"));
        assert!(line.contains("'runex' export pwsh"));
    }

    /// `bin = "run'ex"` must not break out of the eval context.
    #[test]
    fn integration_line_bash_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Bash, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in bash line: {line}");
        assert!(line.contains(r"run'\''ex"), "expected bash-escaped form: {line}");
    }

    #[test]
    fn integration_line_zsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Zsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in zsh line: {line}");
        assert!(line.contains(r"run'\''ex"), "expected zsh-escaped form: {line}");
    }

    /// PowerShell doubles single quotes inside single-quoted strings.
    #[test]
    fn integration_line_pwsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Pwsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in pwsh line: {line}");
        assert!(line.contains("run''ex"), "expected pwsh-escaped form: {line}");
    }

    /// `bin = "app; echo PWNED"` — semicolon must be enclosed in single quotes,
    /// neutralising `;` to prevent command injection.
    #[test]
    fn integration_line_bash_semicolon_does_not_inject() {
        let line = integration_line(Shell::Bash, "app; echo PWNED");
        assert!(
            line.contains("'app; echo PWNED'"),
            "bin must be single-quoted in bash line: {line}"
        );
    }

    /// `bin = "app; Write-Host PWNED"` — semicolon must be enclosed in single quotes.
    #[test]
    fn integration_line_pwsh_semicolon_does_not_inject() {
        let line = integration_line(Shell::Pwsh, "app; Write-Host PWNED");
        assert!(
            line.contains("'app; Write-Host PWNED'"),
            "bin must be single-quoted in pwsh line: {line}"
        );
    }

    /// Nu: quoting without `^` makes a string, not a command — must use `^"runex"`.
    #[test]
    fn integration_line_nu_uses_caret_external_command_syntax() {
        let line = integration_line(Shell::Nu, "runex");
        assert!(
            line.contains("^\"runex\""),
            "nu integration line must use ^\"...\" syntax: {line}"
        );
    }

    #[test]
    fn integration_line_nu_escapes_special_chars_in_bin() {
        let line = integration_line(Shell::Nu, "my\"app");
        assert!(line.contains("^\"my\\\"app\""), "nu: special chars must be escaped: {line}");
    }

    /// `nu_quote_path` must wrap the path in double quotes so Nu doesn't tokenize on spaces.
    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_spaces() {
        let quoted = nu_quote_path("/home/my user/.config");
        assert_eq!(quoted, "\"/home/my user/.config\"");
        assert!(!quoted.starts_with('/'), "path must be quoted, not raw");
    }

    /// Windows paths: backslashes must be escaped inside Nu double-quoted strings.
    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_backslash() {
        let quoted = nu_quote_path(r"C:\Users\my user\AppData");
        assert_eq!(quoted, r#""C:\\Users\\my user\\AppData""#);
    }

    /// The generated Nu integration line must quote the runex.nu path.
    /// Both `save` and `source` must use quoted paths (starting with `"`).
    #[test]
    fn integration_line_nu_save_path_is_quoted() {
        let line = integration_line(Shell::Nu, "runex");
        for fragment in ["save --force \"", "source \""] {
            assert!(
                line.contains(fragment),
                "nu line must contain `{fragment}`: {line}"
            );
        }
    }

    /// A single quote in bin must be passed through `lua_quote_string`.
    /// `lua_quote_string("run'ex")` = `"run'ex"` (single quotes need no escaping in Lua double-quoted strings).
    #[test]
    fn integration_line_clink_single_quote_in_bin_is_lua_quoted() {
        let line = integration_line(Shell::Clink, "run'ex");
        assert!(
            line.contains("\"run'ex\""),
            "bin must be lua-quoted in clink line: {line}"
        );
    }

    /// `lua_quote_string` escapes `\n` to `\\n`, preventing the Lua comment from being broken.
    #[test]
    fn integration_line_clink_newline_in_bin_does_not_inject() {
        let line = integration_line(Shell::Clink, "runex\nos.execute('evil')");
        assert!(
            !line.contains('\n'),
            "literal newline must be escaped in clink line: {line:?}"
        );
        assert!(
            line.contains("\\n"),
            "expected \\n escape sequence in clink line: {line:?}"
        );
    }

    } // mod integration_line

    /// Control-character and NUL handling in `nu_quote_path`.
    ///
    /// `nu_quote_path` embeds paths into Nu double-quoted string literals.
    /// Newlines, carriage returns, tabs, NUL, DEL, and Unicode line separators
    /// must be escaped or dropped so they cannot break out of the string context.
    mod nu_quote_path_escaping {
        use super::*;

    #[test]
    fn nu_quote_path_escapes_newline() {
        let quoted = nu_quote_path("/home/user/.config\nevil");
        assert!(!quoted.contains('\n'), "nu_quote_path must escape newline: {quoted}");
        assert!(quoted.contains("\\n"), "expected \\n escape: {quoted}");
    }

    #[test]
    fn nu_quote_path_escapes_carriage_return() {
        let quoted = nu_quote_path("/path\r/evil");
        assert!(!quoted.contains('\r'), "nu_quote_path must escape CR: {quoted}");
        assert!(quoted.contains("\\r"), "expected \\r escape: {quoted}");
    }

    /// If XDG_CONFIG_HOME contains a newline, it must not inject Nu statements into env.nu.
    #[test]
    fn integration_line_nu_newline_in_xdg_does_not_inject() {
        let quoted = nu_quote_path("/home/user/.config\nsource /tmp/evil.nu\n#");
        assert!(!quoted.contains('\n'), "newline injection must be escaped in nu path: {quoted}");
    }

    #[test]
    fn nu_quote_path_escapes_nul() {
        let quoted = nu_quote_path("path\x00evil");
        assert!(!quoted.contains('\0'), "nu_quote_path must not produce literal NUL: {quoted:?}");
        assert!(quoted.contains("path"), "path prefix must be preserved: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_escapes_tab() {
        let quoted = nu_quote_path("path\t/evil");
        assert!(!quoted.contains('\t'), "nu_quote_path must escape tab: {quoted:?}");
        assert!(quoted.contains("\\t"), "expected \\t escape: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_drops_del() {
        let quoted = nu_quote_path("path\x7fend");
        assert!(!quoted.contains('\x7f'), "nu_quote_path must drop DEL: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_drops_unicode_line_separators() {
        for ch in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            let input = format!("path{ch}end");
            let quoted = nu_quote_path(&input);
            assert!(!quoted.contains(ch), "nu_quote_path must drop U+{:04X}: {quoted:?}", ch as u32);
        }
    }

    #[test]
    fn rc_file_for_bash_ends_with_bashrc() {
        if let Some(path) = rc_file_for(Shell::Bash) {
            assert!(path.to_str().unwrap().ends_with(".bashrc"));
        }
    }

    /// C0 control chars other than `\n`, `\r`, `\t`, `\0`, `\x7f` must be dropped.
    #[test]
    fn nu_quote_path_drops_remaining_c0_control_chars() {
        let dangerous_c0: &[char] = &[
            '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
            '\x08', '\x0b', '\x0c', '\x0e', '\x0f',
            '\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
            '\x18', '\x19', '\x1a', '\x1b',
            '\x1c', '\x1d', '\x1e', '\x1f',
        ];
        for &ch in dangerous_c0 {
            let input = format!("path{}end", ch);
            let quoted = nu_quote_path(&input);
            assert!(
                !quoted.contains(ch),
                "nu_quote_path must drop C0 control U+{:04X}: {quoted:?}",
                ch as u32
            );
        }
    }

    } // mod nu_quote_path_escaping

    /// `nu_quote_path` embeds the XDG_CONFIG_HOME path into the `source "..."` line
    /// written to env.nu. If XDG_CONFIG_HOME contains Unicode visual-deception
    /// characters (RLO, BOM, ZWSP, etc.), the displayed `source` path would appear
    /// different from its actual content, potentially deceiving the user into
    /// thinking a safe path is being sourced. These characters must be dropped.
    mod nu_quote_path_deceptive {
        use super::*;

    /// U+202E (Right-to-Left Override) reverses display order in the terminal.
    #[test]
    fn nu_quote_path_drops_rlo() {
        let quoted = nu_quote_path("/home/user\u{202E}/.config");
        assert!(
            !quoted.contains('\u{202E}'),
            "nu_quote_path must drop U+202E (RLO): {quoted:?}"
        );
    }

    /// U+FEFF (BOM / zero-width no-break space) is invisible.
    #[test]
    fn nu_quote_path_drops_bom() {
        let quoted = nu_quote_path("/home/user\u{FEFF}/.config");
        assert!(
            !quoted.contains('\u{FEFF}'),
            "nu_quote_path must drop U+FEFF (BOM): {quoted:?}"
        );
    }

    /// U+200B (Zero-Width Space) is invisible.
    #[test]
    fn nu_quote_path_drops_zwsp() {
        let quoted = nu_quote_path("/home/user\u{200B}/.config");
        assert!(
            !quoted.contains('\u{200B}'),
            "nu_quote_path must drop U+200B (ZWSP): {quoted:?}"
        );
    }

    /// Non-deceptive Unicode (e.g. Japanese path components) must pass through.
    #[test]
    fn nu_quote_path_preserves_non_deceptive_unicode() {
        let quoted = nu_quote_path("/home/ユーザー/.config");
        assert!(
            quoted.contains("ユーザー"),
            "nu_quote_path must preserve non-deceptive Unicode: {quoted:?}"
        );
    }

    /// A `$` in XDG_CONFIG_HOME would allow Nu variable interpolation inside
    /// the double-quoted `source "..."` path. Must be escaped as `\$`.
    #[test]
    fn nu_quote_path_escapes_dollar_sign() {
        let quoted = nu_quote_path("/home/$USER/.config");
        let bytes = quoted.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'$' {
                let mut preceding = 0usize;
                let mut j = i;
                while j > 0 && bytes[j - 1] == b'\\' {
                    preceding += 1;
                    j -= 1;
                }
                assert!(
                    preceding % 2 == 1,
                    "nu_quote_path: '$' at byte {i} has {preceding} preceding backslashes \
                     (even = Nu interpolation not suppressed). Full output: {quoted:?}"
                );
            }
        }
        assert!(quoted.contains("\\$"), "expected \\$ in: {quoted:?}");
    }

    } // mod nu_quote_path_deceptive

}
