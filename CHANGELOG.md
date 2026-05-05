# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.14] - 2026-05-05

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

### Internal

Phase B refactor — internal-only restructure, **no user-visible
behaviour change**: config schema, hook output format, and `runex
doctor --json` are all unchanged from 0.1.13.

- **`runex/src/main.rs` split into per-subcommand handlers under
  `runex/src/cmd/`** (one file per `Commands` enum variant). The
  pre-Phase-B 1542-line `main.rs` shrinks to dispatch + `Cli` /
  `Commands` derives + the runtime builder. Each handler is now
  unit-testable from inside the process — `cmd::which::handle("a"
  .repeat(1025), …)` returns `CmdOutcome::ExitCode(1)` instead of
  killing the test process.
- **`std::process::exit` calls collapsed from 8 sites to 1.**
  Handlers report failures by returning `Ok(CmdOutcome::ExitCode(n))`
  through the new `CmdResult` type; only `main()` ever calls
  `process::exit`.
- **`AppContext` runtime builder** centralises the
  `resolve_config + resolve_shell + compute_precache_fingerprint
  + make_command_exists` four-line dance that used to be open-
  coded in five handlers. `AppContext::build` for the strict
  path; `AppContext::build_optional` (returning `OptionalContext`)
  for hook / doctor where missing config is non-fatal.
- **Leaf utilities extracted to `runex/src/util/`**
  (`shell` / `path` / `prompt`). Command-specific policy stays
  with the owning handler — `validate_bin` in `cmd/export.rs`,
  `install_rcfile_integration` in `cmd/init.rs`, etc.
- **Shared PTY/subprocess test harness** at
  `runex/tests/support/`. `PtySession::spawn(PtyShell::Bash | Zsh
  | Pwsh, …)` factors out the per-shell launch flags and prompt
  setup that were previously open-coded in each shell test.
- **`runex-core::env::HomeDirResolver`** — new resolver trait with
  `SystemHomeDir` (production) and `EnvHomeDir` (test, closure-
  driven) implementations. `_with` variants of `rc_file_for`,
  `xdg_config_home`, and `default_clink_lua_paths` accept a
  resolver so init-handler tests can be hermetic without touching
  process env. The non-`_with` variants remain as thin wrappers
  over `SystemHomeDir`; **public API is additive only**.

### Tests

- New `bash_pty_integration.rs` (rewritten via the support
  harness — 1 scenario), `zsh_pty_integration.rs` (1 scenario),
  `pwsh_pty_integration.rs` (1 scenario). Linux only: expectrl's
  Windows ConPTY backend is still flagged unstable in the dep
  declaration, so Windows continues to rely on the existing
  `*_integration.rs` subprocess tests.
- `tests::handler_outcomes` (7 unit tests) pin the new
  `CmdOutcome::ExitCode(1)` contract for `handle_which`,
  `handle_expand`, and `validate_bin`.
- `tests::app_context` (3 unit tests) pin fingerprint stability
  (same args → same fingerprint) and the missing-config branches
  for both builder variants.
- 18 new unit tests in `runex-core` covering the
  `HomeDirResolver` trait and the `_with` variants
  (`rc_file_for_with`, `default_clink_lua_paths_with`,
  `xdg_config_home_with`).

### Changed

Phase C refactor — workspace single-crate switch, **no user-visible
behaviour change**: config schema, hook output format, and `runex
doctor --json` are all unchanged from 0.1.13. `cargo install runex`
keeps working exactly as before.

- **`runex-core` absorbed into `runex`.** The two-crate workspace
  the project shipped since 0.1.0 collapses to a single crate.
  Every module that lived under `runex-core/src/` is now under
  `runex/src/{domain,app,infra}/`:
  - `domain/` (pure logic, no I/O): `model`, `expand`, `hook`,
    `sanitize`, `timings`, `shell` (+ embedded shell-script
    templates).
  - `app/` (orchestration / parse / validate / generate):
    `config`, `doctor`, `init`, `precache`.
  - `infra/` (file / registry / env access): `env` (with
    `HomeDirResolver`), `integration_check`.
  Rationale: `runex-core` had zero external reverse dependencies
  on crates.io but was published every release because `cargo
  publish` requires version-pinned path-deps to be on the index.
  The internal `pub` boundary it carried was inappropriate (the
  crate was always internal-only — see the
  "Not a public API" disclaimer the 0.1.13 docstring carried).
  Folding the modules into the bin crate removes the publish
  ceremony and lets the dependency direction (`cmd → app →
  domain`, `cmd → util/infra`, `infra → domain`) be enforced by
  module visibility instead of crate boundaries.
- **crates.io publish reduced to one crate.** The release
  workflow's `publish-crates` job no longer publishes
  `runex-core`; only `runex` ships. The Trusted Publisher
  registration for `runex-core` on crates.io is left in place
  (harmless), and `runex-core 0.1.13` (the last published
  version) stays on crates.io un-yanked for any cargo lockfile
  that still pins it.

### Refactor

Phase D — strict Clean Architecture cleanup on top of the Phase C
single-crate layout. **No user-visible behaviour change**: config
schema, hook output, and `runex doctor --json` remain identical to
0.1.13.

- **`infra → app` import cycle removed.** `RUNEX_INIT_MARKER` and
  `rc_file_for*` moved out of `app::init` into
  `infra::integration_check` and `infra::env` respectively. The
  former cycle (`app::doctor → infra::integration_check →
  app::init`) is now gone.
- **`domain::shell` split.** Orchestration symbols
  (`export_script`, `trigger_for`, `*_bind_lines`, etc.) moved to
  `app::shell_export`. `domain::shell` retains only the `Shell`
  enum and pure quoting helpers — no `Config` dependency.
- **`app::config` file I/O moved to `infra::config_store`.**
  `default_config_path`, `read_config_source`, `load_config`'s
  body, `append_abbr_block`, `remove_abbr_block`, and the atomic
  write/symlink-reject helpers all live under `infra/` now.
  `app::config` keeps parse + validate; thin wrappers preserve the
  call-site API.
- **`app::expand` and `app::hook` use-case wrappers added.** Every
  `cmd/*` handler that used to import `crate::domain::expand` or
  `crate::domain::hook` now goes through `app/`. The
  `HookAction` type is re-exported from `app::hook` so cmd code
  doesn't reach into `domain` for it either.
- **`HomeDirResolver` injection wired to the production
  `cmd::init::handle` path.** The handler now accepts
  `&dyn HomeDirResolver`; main dispatch passes `&SystemHomeDir`.
  Inline `cmd::init::tests` drive the handler with `EnvHomeDir` for
  hermetic end-to-end coverage. The standalone `_with` /
  resolver-less helper variants are removed in favour of the single
  resolver-injectable form.
- **Architecture rules pinned in CI.**
  `runex/tests/architecture.rs` adds four compile-time-ish tests:
  `no_infra_to_app_imports` (with a small exempt list for
  type-only imports), `no_domain_to_anyone_else_imports`,
  `no_cmd_to_domain_behavior_imports`,
  `no_filesystem_calls_in_app_layer`. Future regressions surface
  in CI rather than in code review.

### Internal

- **Visibility tightened crate-wide: every `pub` item is now
  `pub(crate)`.** `runex` is bin-only; there is no library API to
  preserve. The narrower visibility makes accidental cross-layer
  reach harder and shrinks the surface clippy needs to lint.
- **Dropped unused `IntegrationCheck::{name, detail}` accessors.**
  Every consumer destructures via `match`; the methods had zero
  callers and were carried over from the dropped runex-core public
  surface.
- **`Cargo.toml` `[lib] deferred` comment removed**, replaced with
  the actual bin-only contract (no `[lib]` is intentional).
- **`util/`, `cmd/`, `infra/env`, `app/` module docstrings updated**
  to describe the post-Phase-D layering instead of the
  Phase-C-future tense they were written in. `main.rs` crate-root
  docstring documents the layering diagram and points at the
  architecture test.

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
