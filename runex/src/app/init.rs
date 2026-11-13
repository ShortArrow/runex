use crate::domain::shell::Shell;

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
pub(crate) fn default_config_content() -> &'static str {
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
/// Resolver-injectable factory for the clink lua install path.
/// Production callers pass [`crate::infra::env::SystemHomeDir`]; tests
/// pass an [`crate::infra::env::EnvHomeDir`] so a hermetic `cmd::init::handle`
/// run can verify the chosen install path without touching real env
/// vars or `dirs::home_dir()`.
pub(crate) fn clink_lua_install_path_with_resolver(
    env: &dyn crate::infra::env::HomeDirResolver,
) -> std::path::PathBuf {
    if let Some(p) = env.env_var("RUNEX_CLINK_LUA_PATH") {
        return std::path::PathBuf::from(p);
    }
    if let Some(local) = env.env_var("LOCALAPPDATA") {
        return std::path::PathBuf::from(local).join("clink").join("runex.lua");
    }
    if let Some(home) = env.home_dir() {
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
pub(crate) fn next_steps_message(shell: Shell, rc_path: Option<&std::path::Path>) -> String {
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

#[cfg(test)]
mod tests {
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

    /// `clink_lua_install_path_with_resolver` decides where `runex
    /// init clink` writes the lua file. Honours
    /// `RUNEX_CLINK_LUA_PATH` first, then `LOCALAPPDATA` (Windows
    /// convention), then a POSIX-style fallback for clink forks on
    /// Linux. Tests pass an `EnvHomeDir` for hermetic resolution
    /// without racing on `std::env::set_var`.
    #[test]
    fn clink_install_path_honors_env_override() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([(
            "RUNEX_CLINK_LUA_PATH".to_string(),
            "/tmp/runex_test_clink.lua".to_string(),
        )]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let p = clink_lua_install_path_with_resolver(&env);
        assert_eq!(p, std::path::PathBuf::from("/tmp/runex_test_clink.lua"));
    }

    #[test]
    fn clink_install_path_uses_localappdata_when_set() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([(
            "LOCALAPPDATA".to_string(),
            "/tmp/local_appdata_test".to_string(),
        )]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let p = clink_lua_install_path_with_resolver(&env);
        assert_eq!(
            p,
            std::path::PathBuf::from("/tmp/local_appdata_test/clink/runex.lua")
        );
    }

    #[test]
    fn clink_install_path_falls_back_to_home() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> =
            HashMap::from([("HOME".to_string(), "/home/user".to_string())]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let p = clink_lua_install_path_with_resolver(&env);
        assert_eq!(
            p,
            std::path::PathBuf::from("/home/user/.local/share/clink/runex.lua")
        );
    }

    /// An empty env var must be treated as unset; otherwise
    /// `RUNEX_CLINK_LUA_PATH=` would anchor writes to "" which either
    /// fails outright or hits an unintended cwd.
    /// `EnvHomeDir::env_var` already returns `None` for empty strings,
    /// so this test pins both the closure semantics and the
    /// resolver's empty-string handling.
    #[test]
    fn clink_install_path_treats_empty_env_as_unset() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([
            ("RUNEX_CLINK_LUA_PATH".to_string(), String::new()),
            ("LOCALAPPDATA".to_string(), String::new()),
            ("HOME".to_string(), "/home/u".to_string()),
        ]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let p = clink_lua_install_path_with_resolver(&env);
        assert!(p.starts_with("/home/u"), "expected home fallback, got {p:?}");
    }

}
