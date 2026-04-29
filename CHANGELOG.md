# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

> Release-time reminder: bump the AUR `runex-bin` PKGBUILD alongside any
> hook/CLI surface change. Older binaries (e.g. AUR 0.1.11 pre-hook) on
> a user's `PATH` make rcfile-driven integrations silently fall back to
> "literal space" because `runex hook` errors out as an unknown
> subcommand. The shell template safe-fails by design, so this isn't a
> bug to fix in the code — it's an operations note.

### Added
- **`runex doctor` now reports shell-integration health.** New
  `integration:<shell>` rows tell the user whether each shell's rcfile
  contains the `runex-init` marker (so a forgotten `runex init <shell>`
  is visible at a glance) and, for clink specifically, whether the
  `runex.lua` file on disk has drifted from what `runex export clink`
  would emit today. The clink check catches the most common upgrade
  pitfall — bash/zsh/pwsh/nu re-source their integration on shell
  start, but clink keeps a static copy that has to be refreshed by
  re-running `runex init clink`. A missing clink lua file is treated
  as "user doesn't run clink" and is silently skipped rather than
  warned about. New module `runex-core/src/integration_check.rs`
  houses the comparison logic.
- **`runex doctor` now reports every rejected abbreviation rule with
  its field path.** `parse_config` still stops at the first invalid
  field (so `config_parse` shows one error), but doctor walks the
  TOML source and surfaces every `config_validation.abbr[N].<field>`
  failure in one pass. Lets users fix all the typos in one edit
  instead of running doctor in a loop.
- **AUR `runex-bin` and Homebrew tap (`shortarrow/runex`) packaging.**
  `packaging/aur-bin/` and `packaging/homebrew/` ship the manifests
  plus release-helper scripts that fetch tarball sha256s from a tag's
  GitHub Release artifacts and stage commits for both downstream
  clones. See `CONTRIBUTING.md#publish-to-package-registries`.

### Changed
- **License: dual-licensed under MIT OR Apache-2.0** (was MIT only in
  0.1.11). Follows the Rust-ecosystem convention of letting recipients
  pick whichever fits their project. The `LICENSE` file now contains
  both texts, `Cargo.toml` declares `license = "MIT OR Apache-2.0"`,
  and the README sections (English + Japanese) are updated. No code
  change; this only affects the legal terms under which 0.1.12+
  binaries and source can be redistributed.
- **Shell integration rewritten as thin wrappers around the new `runex hook`
  subcommand.** The script emitted by `runex export <shell>` is now a small
  bootstrap (~16 lines for bash, ~95 for pwsh) that calls `runex hook`
  on every trigger keypress. Command-position detection, token extraction,
  cursor placeholder handling and shell escaping have all moved into the
  Rust core. After upgrading, re-run `runex init` (or
  `runex export <shell>`) — the existing `eval` line in your rc file keeps
  working but the *contents* it sources change.

### Deprecated
- `[precache]` config section is now a no-op. Existing configs continue to
  parse without errors, but the `path_only` field has no run-time effect
  since the hook bootstrap consults the config (and `which`) per keypress.
  `runex doctor --strict` warns when the section is present so you can
  remove it at your leisure.
- `runex precache` subcommand is hidden from `--help` and is no longer
  invoked by any shell integration. It remains available for one
  additional release for backward compatibility and may be removed in a
  future version.

### Removed
- Per-shell embedded `case` / `switch` token tables in exported scripts.
  Abbreviation keys are no longer baked into shell code at export time;
  the hook reads them from config at keypress time.
- `bash_quote_pattern` and the `*_known_cases` helpers used to render those
  tables. Internal API only — no user-visible impact.

### Fixed
- **clink shell integration mis-quoted argv0 for cmd.exe**, causing
  `'runex' is not recognized` on every keypress when the binary path
  reached io.popen. POSIX single-quote wrapping is interpreted
  literally by cmd, so the template now uses cmd's own double-quote
  wrapping. Subsumed (and re-validated) by the hook migration that
  rewrote the clink template wholesale.
- **clink (cmd.exe) integration: abbreviations failed to expand when the
  cmd host process had a degraded PATH.** When clink injected into a
  cmd.exe whose PATH lacked the User-scope entries from the registry
  (e.g. `~/.cargo/bin`, `~/AppData/Local/Microsoft/WinGet/Links`),
  `runex hook`'s `which::which` lookups would fail and
  `when_command_exists` rules silently evaluated false, producing a
  no-op space insertion instead of expansion. `runex hook` now augments
  command resolution with HKCU/HKLM `Environment\Path` on Windows so
  binaries installed under the User PATH stay reachable regardless of
  how the parent process was launched. `runex doctor` also reports an
  `effective_search_path: N entries (process=…, +user=…, +system=…)`
  line so this kind of degradation is visible at a glance. See
  `runex/src/win_path.rs` and the regression test
  `runex/tests/windows_path_isolation.rs`.
- `runex export clink` now embeds the absolute path of the running
  executable when called with the default `--bin runex`, sidestepping
  the same PATH-inheritance issue for clink's lua side.

## [0.1.11] - 2026-04-18

Initial public release.

### Added
- Cross-shell abbreviation engine for bash, zsh, PowerShell, cmd/Clink and
  Nushell.
- `runex add` / `runex remove` for in-place config edits.
- `runex doctor` with strict-mode validation, unknown-field detection, and
  per-rule rejection diagnostics.
- `runex timings` for per-phase expand profiling.
- Cursor placeholder (`{}`) support inside expansions.
- Distribution: winget (PR submitted), AUR (`runex-bin`), Homebrew tap
  (`shortarrow/runex`).
