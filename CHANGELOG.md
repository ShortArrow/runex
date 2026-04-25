# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
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
