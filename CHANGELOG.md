# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **clink (cmd.exe) integration: rejected `%` and `!` in shell buffer
  content to block cmd.exe injection.** The clink template's
  `runex_is_safe_line` gate previously rejected only ASCII control
  characters. cmd.exe expands `%FOO%` even inside double-quoted
  argv, and `!FOO!` when SETLOCAL ENABLEDELAYEDEXPANSION is in
  effect anywhere upstream — so a buffer containing `%PATH%` or
  worse `%X%" & calc & "%Y%` was rewritten by cmd before runex hook
  saw it, including being able to inject extra commands. The gate
  now drops on either of those metacharacters; users typing literal
  `%` or `!` lose the runex expansion on that keypress (the trigger
  key's plain literal-space fallback applies instead) but cmd
  itself still executes the typed command normally.
- **`runex init clink` now writes the lua via atomic-temp + rename**
  and refuses to follow a symlink at the install path. Previously a
  pre-existing symlink would silently redirect the export to
  whatever the symlink pointed at, and a crash mid-write left a
  half-written lua file that clink would parse-fail on the next cmd
  window.
- **`runex init`'s rcfile marker check now uses `O_NOFOLLOW`** on
  Unix, matching the policy of the rcfile write side. Previously
  the read could decide "marker already present" by following a
  symlink target while the write would refuse to follow — confusing
  at minimum and potentially usable for information leakage about
  the target file's contents via init's stdout.
- **Windows registry `Environment\Path` reads are now bounded** to
  64 KiB and 256 entries per hive, preventing an attacker (or a
  runaway installer) who can write to HKCU from making every
  `runex hook` keystroke spend extra CPU on a giant PATH walk.
- **Documented why `read_config_source` allows symlinks at the final
  path component** — the dotfiles pattern
  (`~/.config/runex/config.toml -> ~/dotfiles/...`) is widely used
  and a deliberate trade-off; the previous docstring claimed
  stricter behaviour than the code delivered.

## [0.1.13] - 2026-05-04

### Added
- **`runex init <shell>`** — `init` now accepts an optional shell
  positional argument so users can target a specific shell (e.g.
  `runex init pwsh`, `runex init clink`) instead of relying on
  `$SHELL` auto-detection. Plain `runex init` keeps the existing
  detect-and-do-one-shell behaviour. Closes the documentation /
  implementation mismatch where `runex doctor` and the docs were
  recommending `runex init <shell>` against a CLI that didn't accept
  shell arguments.
- **`runex init clink` writes `%LOCALAPPDATA%\clink\runex.lua`** for
  you. The lua file is generated from `runex export clink` against
  the current config, written under `%LOCALAPPDATA%\clink\runex.lua`
  by default (override with `RUNEX_CLINK_LUA_PATH`). Drift with the
  on-disk file is detected and confirmed before overwriting; identical
  content is a no-op. This replaces the manual
  `runex export clink > %LOCALAPPDATA%\clink\runex.lua` step that
  every clink user previously had to run by hand and re-run after
  every upgrade.
- **Next-steps guidance after `runex init`.** Each successful init
  prints a four-step blurb tailored to the target shell (how to
  reload, the seed `gst<Space>` demo, the recipes link, and `runex
  doctor` for verification).
- **Seed config now includes a working sample.** `runex init` writes a
  `[keybind.trigger] default = "space"` block plus a `gst → git
  status` `[[abbr]]` rule so a fresh install demonstrates expansion
  immediately. Existing configs are untouched (`init` still uses
  `OpenOptions::create_new` and refuses to overwrite).
- **`docs/recipes.md` cookbook** — 12 use-case-driven, copy-pasteable
  `config.toml` snippets covering Git shortcuts, per-shell command
  variants (the `expand = { default = …, pwsh = … }` table form),
  three-step fallback chains, cursor placeholders for fill-in-the-blank
  templates, `[keybind.self_insert]` for skip-expansion-this-once,
  Docker/kubectl bundles, and doctor-driven troubleshooting recipes.
  Cross-linked from README and `docs/config-reference.md`. Japanese
  mirror at `docs/recipes.ja.md`.

### Changed
- **`release.yml` gates binary build on `cargo test --workspace`**
  passing across ubuntu/windows/macOS. Previously, `build` started
  in parallel with whatever CI workflow the bump commit had triggered,
  so a tag push could in principle ship binaries from a commit whose
  tests never finished. The new `test` job is `needs:`-required by
  `build`, closing that race.
- **README & docs/setup explicitly document rcfile-write safety.** New
  "What `runex init` will and won't do" section in `docs/setup.md`
  (and the Japanese mirror) lists the append-only / `O_NOFOLLOW` /
  marker-idempotent / size-cap properties so users can confidently
  run `init` without fearing for their existing rcfile.
- **crates.io publish moved into CI via OIDC Trusted Publishing.**
  The `publish-crates` job in `release.yml` exchanges the workflow's
  GitHub OIDC token for a short-lived crates.io token
  (`rust-lang/crates-io-auth-action@v1.0.4`), publishes `runex-core`,
  waits for the sparse index to propagate, then publishes `runex`.
  No long-lived `CARGO_REGISTRY_TOKEN` is stored as a repository
  secret or kept on a developer laptop. One-time per-crate Trusted
  Publisher setup is required on crates.io — see
  `CONTRIBUTING.md` `### crates.io (OIDC Trusted Publishing)`.
  Skip the publish on a particular tag by including `[skip publish]`
  in the bump commit message.

## [0.1.12] - 2026-04-30

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
