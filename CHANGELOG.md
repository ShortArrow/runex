# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.17] - 2026-05-30

### Fixed

- **Git Bash: Ctrl+C is lost after an expansion (#7).** Under Git Bash
  (cygwin/msys bash) the readline `bind -x` handler is invoked on top
  of the cygwin signal layer. Spawning a Win32 `.exe` from inside that
  handler — which is exactly what every `runex hook` call did at
  trigger time — caused the very next `SIGINT` to be lost, so the
  user's reflexive `Ctrl+C` after an unwanted expansion no longer
  cleared the line buffer, and pressing `Enter` ran the stale
  expanded command. Reproduced on Windows 11 + Git Bash 2.50 with
  every abbreviation, regardless of cursor position or whether the
  subprocess output was consumed via `$(...)` or a temp file. The
  root cause is the spawn itself, not how the output is read.

  The bash integration cache now ships a **bake-mode dispatcher**
  selected at source time by `case "${OSTYPE-}"`. Under
  `msys*`/`cygwin*` the trigger handler resolves the abbreviation
  from a static table baked into the cache file (associative
  arrays for exact + condition + pattern rules, plus a tiny
  pure-bash renderer for `{}` cursor placement and `{number}`
  repetition). No subprocess is spawned, so the next `SIGINT`
  reaches the shell as it should and `Ctrl+C` clears the line as
  on every other platform.

### Changed

- **Shell taxonomy: `Shell::CygwinBash` variant dropped from the
  plan.** The 0.1.16 CHANGELOG mentioned that 0.1.17 would
  introduce a `Shell::CygwinBash` enum variant. After PoC we found
  the difference is purely a runtime-environment quirk, not a
  language-level shell distinction, and that taxonomy expansion
  would have pushed across the enum, config, export, init, and
  infra layers for a problem that fits in one runtime `case`
  block. The shipping approach keeps `Shell` unchanged and routes
  through `$OSTYPE` inside the cache file instead. Linux bash,
  WSL bash, zsh, pwsh, and nu users see no change.

- **Bash integration cache version bumped 1 → 2.** Caches written
  by 0.1.16 still source cleanly under 0.1.17 (the legacy exec
  path is still the `*)` arm of the `case`), but `runex doctor`
  now flags v1 caches as stale so users get nudged into
  `runex init bash` to pick up the bake dispatcher on Git Bash.

### Known trade-off

- **Git Bash only: command-position detection is now disabled.**
  The exec-path hook understood that `echo gst` does not expand
  `gst` because it is in argument position, not command position.
  Re-implementing that state machine in pure bash would more than
  double the size of the dispatcher block for marginal benefit,
  so the bake path expands any trailing token that matches an
  abbreviation, regardless of context. This is the *intentional*
  cost of fixing #7. Documented in `docs/setup.{md,ja.md}` and
  pinned by a regression test
  (`tests/bash_cygwin_bake_pty.rs::cygwin_bake_expands_even_when_token_is_not_in_command_position`).
  Quote literals you don't want expanded (`echo "gst"`) or pick
  abbreviation keys that won't collide with English words. Every
  other shell (including Linux bash and WSL bash) retains full
  command-position detection.

## [0.1.16] - 2026-05-23

### Fixed

- **Misleading `sudo` recipe in docs (#4).** `docs/recipes.md` and its
  Japanese translation showed `apt-up = "apt update && apt upgrade"`
  paired with `sudo apt-up<Space>` — but `sudo` only applies to the
  command immediately after it, so the `apt upgrade` half ran as the
  unprivileged user and silently failed. The section now spells out
  the pitfall, shows the correct multi-command form
  (`aptup = "sudo apt update && sudo apt upgrade"`, called without a
  leading `sudo`), and gives a clear rule of thumb for when to bake
  `sudo` into the expansion vs. typing it on the command line.

- **Trigger space leaks into cursor placeholders (#3).** When an
  abbreviation's `expand` contained `{}` (the cursor placeholder),
  the trigger space that fired the expansion was also inserted at
  the placeholder position. `gca<Space>` with
  `expand = "git commit -am '{}'"` yielded
  `git commit -am ' '` with the cursor after the stray space,
  instead of `git commit -am ''` with the cursor between the
  quotes. The trigger space is now suppressed whenever the
  expansion declares a placeholder, so the rule author's chosen
  cursor position is preserved.

### Added

- **`{number}` placeholder for numeric repetition (#1).** New named
  placeholder lets a single rule capture trailing digits in the
  token and repeat a unit string that many times in the expansion:

  ```toml
  [[abbr]]
  key    = "up{number}"
  expand = "cd {number}"
  number = "../"
  ```

  Typing `up3<Space>` then expands to `cd ../../../`. Exact rules
  still win when both could match the same token (e.g. `up2` exact
  beats `up{number}`), so adding a pattern rule never weakens the
  existing exact ones. Bounded by `MAX_NUMERIC_REPEAT = 128` and a
  32-byte unit cap, so a dynamic expansion can never exceed what a
  hand-written 4096-byte `expand` could already produce.

- **`runex list <FILTER>` exact-key filter (#2).** `runex list` now
  accepts an optional positional argument that narrows the output to
  the single rule whose key matches exactly. Works for both the TSV
  default output and `--json`. A no-match filter is a normal exit-0
  with empty output (or an empty JSON array), so the command stays
  scriptable. Match is case-sensitive and literal — no prefix /
  substring / glob expansion; reach for `runex which <token>` when
  you want the full per-shell + when_command_exists picture.

- **Static shell integration cache (per-keystroke latency fix).** `runex init <shell>`
  for bash/zsh/pwsh/nu now writes a static script to
  `<XDG_CACHE_HOME>/runex/integration.<ext>` (matching clink's
  long-standing pattern) and appends a one-line `source` to the
  user's rcfile/profile. The cache file has the absolute
  `current_exe()` path baked in, so per-keystroke hook
  invocations no longer re-resolve `runex` through `$PATH`.
  This removes the ~470 ms-per-keystroke latency users on WSL
  with a `mise` shim ahead of `~/.cargo/bin/runex` were seeing
  (mise startup overhead × every Space press in `bind -x`-style
  callbacks).
  - **Versioned cache header.** Each cache file starts with
    `# runex-integration-version: 1` plus a `# runex-bin: <abs>`
    line and a "do not edit" notice. Bumping the format in a
    future release will be a one-line change here that doctor
    surfaces as "outdated cache, re-run `runex init <shell>`".
  - **Interactive guard inside the cache.** Templates now
    early-return when sourced by a non-interactive shell
    (`bash -c '...'`, CI scripts, plugin sandboxes), so
    integration installs leave no side effects on those paths.
  - **Auto-refresh on `runex add` / `runex remove`.** Existing
    caches for shells the user has already installed get
    silently regenerated when config changes, so new
    abbreviations are picked up by the next shell start without
    needing an explicit re-init. Shells without a cache are
    skipped (no opt-in side effect).
  - **`runex doctor` cache freshness check.** New
    `integration:<shell>:cache` row per shell flags missing
    binaries, version mismatches, and legacy
    `eval "$(runex export bash)"`-style content. Clink keeps
    its existing byte-compare freshness probe.
- **`runex export <shell>` defaults `--bin` to `current_exe()`.**
  Omitting `--bin` (the recommended path) bakes the absolute
  binary path into the generated script. Passing `--bin runex`
  explicitly keeps the legacy bare-name behaviour for power
  users hand-managing dotfiles that source the same exported
  script across multiple machines with different installations.
  Also: `runex export <shell>` (non-clink) now prepends the
  same versioned header as the cache file, so the byte stream
  is interchangeable.

### Changed

- **`lua_quote_string` drops Unicode visual-deception
  characters (RLO, BOM, ZWSP, etc.).** Previously these were
  passed through unchanged because clink's only consumer
  (`--bin`) restricted input to printable ASCII via
  `validate_bin`. With the new static-cache layout, the clink
  install path also flows through `lua_quote_string`, so the
  quoter is now hardened in isolation rather than relying on
  upstream validation.

### Internal

- **New module `infra::integration_cache`.** Owns the cache
  path resolution, atomic write (sibling-temp + fsync +
  rename), and header generation. Generalises the pattern that
  was inline in `cmd::init::install_clink_lua` since 0.1.13.
- **New `infra::env::xdg_cache_home_with`.** Mirrors the
  existing `xdg_config_home_with`: `$XDG_CACHE_HOME` →
  `$LOCALAPPDATA` (Windows) → `~/.cache` (non-Windows) →
  `~/AppData/Local` (Windows fallback). Resolver-injectable for
  hermetic tests.
- **Cleaned up `app::init`.** Removed the inline `nu_quote_path`
  helper (replaced by `domain::shell::nu_quote_string` now that
  cache paths flow through there). The `nu_quote_path_escaping`
  / `nu_quote_path_deceptive` test mods were re-pinned against
  the new public API surface (`integration_line(Shell::Nu, …)`)
  so the security regression coverage stays intact through the
  refactor.

### Tests

- New `tests/shell_integration.rs` with five subprocess pins
  against bash 4+ (Linux only): non-interactive guard works,
  interactive subshell defines `__runex_expand`, header
  contains version + bin lines, rcfile gains a `source` line
  pointing at the cache, init cleans up a stale `.tmp` from a
  simulated previous crash.
- `infra::integration_cache::tests` (7 tests on Windows + 9 on
  Linux): cache_path resolution per shell, XDG fallback, atomic
  write, parent-dir auto-creation, symlink reject (Unix), header
  format pinning.
- `infra::integration_check::tests::cache_freshness` (7 tests):
  every doctor branch (Skipped × 2, Ok × 2, Outdated × 4)
  including the bare-`runex` opt-out path.
- `cmd::add_remove::tests` (3 tests): silent refresh on add,
  no-op preservation on zero-match remove, no auto-creation
  for shells without a pre-existing cache.
- `cli_integration` gains 3 new tests for `runex export bash`
  default vs explicit `--bin`, and an env-isolation fix for
  `init_cmd_in_dir` so parallel tests no longer race on the
  real `~/.cache`.

### Docs

- New ADR
  [`docs/decisions/0001-static-integration-cache.md`](docs/decisions/0001-static-integration-cache.md)
  records the design rationale, considered alternatives
  (doctor-WARN-only, rcfile-baked absolute path,
  current_exe-default-only, lazy bind via PROMPT_COMMAND), and
  the long-term implementation contract.
- New ADR
  [`docs/decisions/0002-containerized-linux-ci.md`](docs/decisions/0002-containerized-linux-ci.md)
  captures the containerised Linux CI design: why Linux CI runs
  inside a pinned GHCR image, why macOS / Windows stay native, and
  the digest-pin bump procedure.
- `CONTRIBUTING.md` documents the dev-container hand-check
  command and how to roll a new `runex-ci` image digest into
  `.github/workflows/ci.yml`.

### CI

- **Containerised Linux CI.** `test-linux` in
  `.github/workflows/ci.yml` now runs inside the pinned
  `ghcr.io/shortarrow/runex-ci@sha256:...` image instead of
  installing zsh / pwsh / nu / xclip / wl-clipboard / xsel
  ad-hoc on each run. The image is built and pushed by
  `.github/workflows/build-ci-image.yml`
  (Dockerfile: `containers/ci/ubuntu.Dockerfile`,
  sanity check: `containers/ci/sanity.sh`). Bumping the digest
  is a one-line commit so a re-built image cannot silently
  change what the gate runs against. macOS and Windows jobs
  stay on native runners.
- **Build-time reproducibility hardening.** `ubuntu:24.04` is
  pinned by manifest-list digest; `NU_VERSION`,
  `RUST_TOOLCHAIN`, and `NODE_MAJOR` are explicit `ARG`s so
  bumps show up in `git log -p`; `cargo test --locked` on every
  job (linux/macos/windows) makes Cargo.lock drift fail loudly.
- **Workflow security tightening.** All `actions/checkout`
  steps in `ci.yml` and `build-ci-image.yml` now set
  `persist-credentials: false`, matching `release.yml`.
  `build-ci-image.yml` also runs as a build-only check on
  `pull_request` (no GHCR push, no `packages: write` use), so a
  broken Dockerfile fails CI before it can land on `develop`.

### Docs

- `docs/setup.md` (and the Japanese translation) rewrites the
  PowerShell section for the static-cache install path, calls out
  the PSReadLine dependency explicitly, and documents two PS5-
  specific traps surfaced during 0.1.16 hand-checks: the default
  `Restricted` execution policy refusing to dot-source the cache
  file, and the `AllSigned` policy plus a newer PSReadLine in
  `Documents\PowerShell\Modules` triggering an untrusted-publisher
  prompt. The Troubleshooting list grows two pwsh-specific rows
  pointing at the same conditions.

### Migration

Users on 0.1.14 with `eval "$(runex export bash)"` (or the
shell-equivalent `Invoke-Expression (& 'runex' export pwsh | ...)`)
in their rcfile see no immediate functional change — that form
keeps working. But they don't get the static-cache speedup until
they (a) delete the legacy line and (b) re-run
`runex init <shell>`.

`runex doctor` now detects this case explicitly. After upgrading
to 0.1.16 the `integration:<shell>` row reports `Outdated` with
the rcfile path, the cache path, and a remediation hint, e.g.:

```
[WARN] integration:bash: marker found in ~/.bashrc but rcfile uses
       still calls `runex export bash` directly instead of sourcing the
       cache at ~/.cache/runex/integration.bash
       is unused — delete the old line and re-run `runex init bash`
```

If both the new cache-source line and the legacy `export <shell>`
line are present (rare — usually because `init` was re-run before
the legacy line was removed), the same row reports `Outdated` with
a slightly different message asking to delete the duplicate.

Doctor leaves the rcfile untouched. The fix is one line in the
user's rcfile; runex deliberately does not auto-edit shell startup
files.

### Known issues

- **Git Bash + cursor placeholder + Ctrl+C** (cygwin/msys readline
  limitation). On Windows Git Bash (the cygwin/msys port of bash
  used by Git for Windows), expanding an abbreviation whose
  `expand` contains `{}` leaves the cursor in the middle of the
  line. Pressing `Ctrl+C` right after the expansion does **not**
  clear the line buffer — the next `Enter` will then run the
  stale expanded command (e.g. an unintended empty
  `git commit -am ''`). The same flow works correctly on Linux
  bash, WSL bash, zsh, pwsh, and nu — only Git Bash's cygwin
  readline backend is affected. As a workaround, press
  `Backspace` (or any character key) before `Ctrl+C`, or just
  delete the line manually. Runex 0.1.16 will treat cygwin/msys
  bash as a distinct `Shell::CygwinBash` variant so the bash
  template can apply a workaround tailored to that backend.

## [0.1.14] - 2026-05-06

### Added

- **`runex paste-clipboard` (hidden subcommand) and nu Ctrl+V paste
  binding.** Reads the system clipboard text and writes it to stdout;
  the nu integration uses it to inject paste content via
  `commandline edit --insert`, sidestepping nu's per-keystroke
  abbreviation trigger. Enable by adding to `config.toml`:
  ```toml
  [keybind.paste_intercept]
  nu = "ctrl-v"
  ```
  Provider chain: Windows uses native `OpenClipboard` /
  `GetClipboardData(CF_UNICODETEXT)` via `windows-sys`; Linux tries
  `wl-paste` → `xclip -selection clipboard -o` → `xsel --clipboard
  --output`; WSL falls back to `powershell.exe Get-Clipboard` when
  no Linux clipboard daemon is available; macOS uses `pbpaste`.
  Cap is 1 MiB; per-provider timeout is 500 ms. The paste_intercept
  binding is not generated when the config does not opt in, so
  existing nu setups are unaffected.
- **Config schema: `[keybind.paste_intercept]` and `TriggerKey::ctrl-v`.**
  Currently only `nu = "ctrl-v"` is supported. Setting `ctrl-v` as a
  regular trigger or self-insert binding is rejected with
  `CtrlVAsTrigger` / `CtrlVAsSelfInsert`; setting paste_intercept on
  bash/zsh/pwsh is rejected with `PasteInterceptUnsupportedShell`
  (those shells either have no trigger-on-paste race, or
  short-circuit via `paste_pending`).

### Known limitations

- **nu (`nushell` 0.111): pasting content that contains the trigger
  space drops everything after the first triggering space — UNLESS
  you opt into the `[keybind.paste_intercept] nu = "ctrl-v"` binding
  added in this release.** Without paste_intercept, nu's reedline
  delivers paste characters one keystroke at a time, and the
  `executehostcommand` event the runex space binding uses resets the
  command line at fire time, so paste content arriving after the
  triggering space is lost. Workarounds, in order of preference:
  1. **(Recommended)** Configure the Ctrl+V paste binding:
     ```toml
     [keybind.paste_intercept]
     nu = "ctrl-v"
     ```
     Then paste with Ctrl+V — runex reads the clipboard and inserts
     it without the abbr binding ever seeing the spaces. Mouse
     middle-click and terminal right-click paste still go through
     the keymap and remain affected.
  2. Switch nu's trigger to a chord paste streams cannot contain:
     ```toml
     [keybind.trigger]
     nu = "shift-space"
     ```
  3. Quote/escape paste content, or paste it in pieces.
  This is upstream behaviour for every nu keymap binding, not just
  runex; bash/zsh/pwsh/clink are unaffected (no trigger-on-paste
  race, `paste_pending` short-circuit, or standalone-keypress-only
  bindings respectively).
- **Windows Terminal swallows `Ctrl+V` (and several other chords)
  before nu sees them.** This breaks the new
  `[keybind.paste_intercept] nu = "ctrl-v"` workaround on Windows
  Terminal even when the runex binding is correctly registered
  (verified via reedline `keybindings list`). Workarounds:
  1. Use a terminal that does not intercept Ctrl+V — WezTerm and
     Alacritty pass it through to nu unchanged.
  2. Remap or disable Ctrl+V in Windows Terminal settings (the
     `paste` binding) so the chord reaches the shell.
  3. Fall back to `[keybind.trigger] nu = "shift-space"` (Known
     limitation entry above), which sidesteps the trigger-on-paste
     issue without needing a Ctrl+V binding at all.
  bash/zsh/pwsh/clink are unaffected because they don't use
  paste_intercept.
  *Needs investigation:* during hand-check, `Ctrl+Shift+V` (the
  alternative paste chord on many Windows setups) also failed to
  reach a registered nu binding under both Windows Terminal and
  WezTerm. The root cause was not pinned down — it could be the
  terminal emulator, reedline's modifier name parsing, or nu's
  bracketed-paste handling. Until that's investigated, treat
  `Ctrl+Shift+V` as not a viable alternative chord and stick with
  the workarounds listed above.

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
