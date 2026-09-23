# runex - Product Requirements Document

English | [日本語](PRD.ja.md)

## 1. Overview

runex is a cross-shell tool that expands short inputs (runes) into full commands (casts) as the user types.

- Input: short token (e.g. `gcm`)
- Output: expanded command (e.g. `git commit -m`)

The core concept is **"rune-to-cast expansion"**.

---

## 2. Concept

> Compress long incantations into runes, then expand them at execution time.

- Rune: a short input token
- Cast: the full command to be executed
- runex: the Rune → Cast expansion engine

---

## 3. Goals

### 3.1 Problems to solve

- Typing long commands is tedious
- Aliases and functions are scattered across shells
- Settings cannot be unified across pwsh / bash / nu
- No fish-abbr-like UX in other shells

### 3.2 Value proposition

- Cross-shell shared abbreviation definitions
- Real-time expansion on a configurable trigger key
- Conditional expansion (`when_command_exists`) for graceful multi-machine fallback
- Centralized management via a single `config.toml`
- Debuggability: `which --why` and `expand --dry-run` explain every expansion decision

---

## 4. Scope

### Supported shells

- bash
- zsh
- PowerShell (pwsh)
- cmd (via Clink)
- Nushell (nu)

---

## 5. Architecture

```text
config.toml
    ↓
runex (single Rust crate, internal modules: domain / app / infra)
    ↓
shell adapters
├─ pwsh  (PSReadLine)
├─ bash  (readline / bind)
├─ zsh   (zle / bindkey)
├─ clink (Lua)
└─ nu    (script)
```

Internal layering since 0.1.14:

- **`domain/`** — pure logic (model, expand, hook, sanitize,
  timings, shell quoting + templates). No I/O, no env reads.
- **`app/`** — orchestration / parse / validate / generate
  (config, doctor, init, shell_export, hook).
- **`infra/`** — file / registry / env access (env with
  `HomeDirResolver`, integration_cache, integration_check).
- **`cmd/`** — CLI subcommand handlers (one file per `Commands`
  enum variant).
- **`util/`** — leaf helpers (shell detection, command_exists
  factory, prompt).

Dependency direction: `cmd → app → domain`, `cmd → util/infra`,
`infra → domain` (one-way, no cycles). Pre-0.1.14 the same code
lived in two crates (`runex-core` + `runex`); the split was
removed in Phase C because the internal `pub` boundary it carried
served no external consumer.

Each shell adapter is a thin template that reads the live buffer,
calls `runex hook`, and applies the returned eval text. Every
per-keystroke decision is made in Rust. The adapter is installed
as a static cache file recorded in [ADR 0001](decisions/0001-static-integration-cache.md);
clink's transport is recorded in [ADR 0003](decisions/0003-clink-hex-line-transport.md).

---

## 6. Functional requirements

### 6.1 Core

- Token → expansion (first passing rule wins)
- Self-loop guard: `key == expand` → skip rule, continue evaluation
- `when_command_exists`: skip rule if any listed command does not resolve via `which` at hook time; continue evaluation
- Fallback: pass through undefined tokens unchanged
- Multiple rules with the same key: evaluated in order as a fallback chain

### 6.2 CLI

```
runex expand --token <token>              expand a token
runex expand --token <token> --dry-run   simulate expansion, show match trace
runex list                               list all abbreviations
runex list <key>                         show only the rule whose key matches exactly
runex which <token>                      show which rule matches
runex which <token> --why                show full match trace with skip reasons
runex doctor                             check config and environment
runex doctor --no-shell-aliases          skip alias conflict checks (avoids spawning shells)
runex doctor --strict                    also warn about unknown config fields
runex doctor --verbose                   show full error details
runex add <key> <expand>                 add an abbreviation rule to config
runex add <key> <expand> --when <cmd>    add with when_command_exists condition
runex remove <key>                       remove an abbreviation rule from config
runex init                               create config and install shell integration (auto-detect shell)
runex init <shell>                       target a specific shell (bash/zsh/pwsh/clink/nu)
runex init -y                            same, skip confirmation prompts
runex export <shell>                     print the shell integration script
runex export <shell> --bin <name>        use a custom binary name or path in the script
runex timings <key>                      show per-phase timing breakdown of expand
runex timings                            time all abbreviation rules
runex config where                       print the resolved config file path
runex config type                        print the config file contents to stdout
runex config show                        open the config file with the OS-associated app
runex config reload                      regenerate the installed shell integration caches from the config file
runex version                            show version and build commit
```

Global flags (accepted by every subcommand):

```
--config <path>      override config file path (overrides RUNEX_CONFIG)
--path-prepend <dir> prepend directory to PATH for command existence checks
--json               JSON output (supported by: list, doctor, version, expand, which, timings, config where)
```

`runex hook` and `runex paste-clipboard` are hidden subcommands called by the shell integration, not by users.

### 6.3 Config file

Default: `$XDG_CONFIG_HOME/runex/config.toml`, falling back to `~/.config/runex/config.toml` on all platforms.
Override: `RUNEX_CONFIG` env var or `--config` flag.

```toml
version = 1

[keybind.trigger]
default = "space"       # default trigger for all shells
bash    = "alt-space"   # shell-specific override (optional)

[[abbr]]
key    = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key    = "ls"
expand = "ls --color=auto"

[[abbr]]
key    = "gcm"
expand = "git commit -m"
```

See `docs/config-reference.md` for the full field reference.

---

## 7. Non-functional requirements

- Fast: the in-process expansion path (config load, shell resolve,
  expand) completes in under 1 ms per key press. `runex timings <key>
  --json` is the instrument. Measured on 2026-09-16 with a release
  build of 0.1.20 (`b549c9c`) on Windows 11 against a one-rule config:
  10 runs, total 205–345 µs, median 217 µs. Process spawn and the
  shell's own key handling are outside this figure.
- Cross-platform: Windows / Linux / macOS
- Shell-independent core logic (`runex/src/domain/` modules)
- Safe: self-loop guard prevents infinite expansion
- Testable: `command_exists` injected via dependency injection

---

## 8. Constraints

- No full shell parser — token-level processing only
- Quoted strings inside tokens are not interpreted
- runex does not re-escape expansion text; the shell receives it as-is

---

## 9. Roadmap

### Done (post-0.1.11)

- Per-keystroke logic centralised in the `runex hook` subcommand;
  shell templates reduced to thin wrappers.
- Static integration cache for bash / zsh / pwsh / nu, sourced from
  the rcfile by absolute path (ADR 0001). `runex config reload`
  regenerates it after a hand edit.
- `runex doctor` reports environment-level health: Windows
  `effective_search_path` breakdown, `integration:<shell>` rcfile
  marker check, `integration:<shell>:cache` header check, and
  clink lua drift detection.
- `runex init <shell>` accepts a shell positional and writes the clink
  lua integration directly. The seed config includes a working sample
  (`gst → git status`). Per-shell "Next steps" guidance after init.
- crates.io publish moved into CI via OIDC Trusted Publishing — no
  long-lived `CARGO_REGISTRY_TOKEN` anywhere. Test gate added so a
  tag push can never ship binaries from a commit whose tests didn't
  finish.
- Containerized Linux CI with a digest-pinned image (ADR 0002).
- Distribution: GitHub Releases for six targets, crates.io, AUR
  (`runex-bin` and `runex`), Homebrew tap, winget, `mise github:`.
- clink sends the buffer hex-encoded, so `"`, `%` and `!` in the
  buffer no longer break expansion (ADR 0003).
- PTY-driven keystroke tests for bash, zsh, pwsh and nu
  (`runex/tests/*_pty_integration.rs`), plus rcfile-write property
  tests for `runex init` (`runex/tests/cli_integration.rs`).
- `docs/recipes.md` cookbook with use-case-driven `config.toml`
  snippets.

### Near-term

- Continue refining diagnostics surfaced by `doctor` and `init` as
  new failure modes are observed in the wild.
- A clink keystroke test. Today's clink coverage runs the template's
  cmd.exe command line through a real cmd.exe
  (`runex/tests/cli_integration.rs`) but does not drive clink itself.

### Later

- Fuzzy suggestions / fallback matching
- Interactive picker
- History-based learning
- IDE integration (Neovim, VS Code)
- `cargo-binstall` metadata in `Cargo.toml`

---

## 10. Success criteria

- All shells unified under a single config file
- Perceived reduction in typing time
- Reduction in per-shell alias sprawl

---

## 11. Name definition

runex =

- **run** (execute)
- **ex** (expand / execute)
- **rune** (compressed command)

---

## 12. One-line definition

> runex is a rune-to-cast expansion engine.
