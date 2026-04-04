# runex - Product Requirements Document

English | [日本語](PRD.ja.md)

## 1. Overview

runex is a cross-shell tool that expands short inputs (runes) into full commands (casts) in real-time.

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

### 3.1 Problems to Solve

- Typing long commands is tedious
- Aliases and functions are scattered across shells
- Settings cannot be unified across pwsh / bash / nu
- No fish-abbr-like UX in other shells

### 3.2 Value Proposition

- Cross-shell shared abbreviation definitions
- Real-time expansion on a configurable trigger key
- Conditional expansion (`when_command_exists`) for graceful multi-machine fallback
- Centralized management via a single `config.toml`
- Debuggability: `which --why` and `expand --dry-run` explain every expansion decision

---

## 4. Scope

### Supported Shells

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
runex core (Rust library crate: runex-core)
    ↓
shell adapters
├─ pwsh  (PSReadLine)
├─ bash  (readline / bind)
├─ zsh   (zle / bindkey)
├─ clink (Lua)
└─ nu    (script)
```

---

## 6. Functional Requirements

### 6.1 Core

- Token → expansion (first passing rule wins)
- Self-loop guard: `key == expand` → skip rule, continue evaluation
- `when_command_exists`: skip rule if any listed command is absent from PATH; continue evaluation
- Fallback: pass through undefined tokens unchanged
- Multiple rules with the same key: evaluated in order as a fallback chain

### 6.2 CLI

```
runex expand --token <token>              expand a token
runex expand --token <token> --dry-run   simulate expansion, show match trace
runex list                               list all abbreviations
runex which <token>                      show which rule matches
runex which <token> --why                show full match trace with skip reasons
runex doctor                             check config and environment
runex doctor --no-shell-aliases          skip alias conflict checks (avoids spawning shells)
runex init                               create config and append shell integration to rc file
runex init -y                            same, skip confirmation prompts
runex export <shell>                     generate shell integration script
runex export <shell> --bin <name>        use a custom binary name in the script
runex version                            show version and build commit
```

Global flags (accepted by every subcommand):

```
--config <path>      override config file path (overrides RUNEX_CONFIG)
--path-prepend <dir> prepend directory to PATH for command existence checks
--json               JSON output (supported by: list, doctor, version)
```

### 6.3 Config File

Default: `$XDG_CONFIG_HOME/runex/config.toml`, falling back to `~/.config/runex/config.toml` on all platforms.
Override: `RUNEX_CONFIG` env var or `--config` flag.

```toml
version = 1

[keybind]
trigger = "space"        # default trigger for all shells
bash    = "alt-space"    # shell-specific override (optional)

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

## 7. Non-Functional Requirements

- Fast: expansion path completes in <1 ms
- Cross-platform: Windows / Linux / macOS
- Shell-independent core logic (runex-core crate)
- Safe: self-loop guard prevents infinite expansion
- Testable: `command_exists` injected via dependency injection

---

## 8. Constraints

- No full shell parser — token-level processing only
- Quoted strings inside tokens are not interpreted
- runex does not re-escape expansion text; the shell receives it as-is
- Clink shell integration cannot be automatically appended by `runex init`

---

## 9. Roadmap

### Near-term

- Harden `doctor` and `init` around edge cases and clearer diagnostics

### Later

- Fuzzy suggestions / fallback matching
- Interactive picker
- History-based learning
- IDE integration (Neovim, VS Code)
- Broader distribution channels (GitHub Releases, `cargo-binstall`, `winget`, `mise github:`)

---

## 10. Success Criteria

- All shells unified under a single config file
- Perceived reduction in typing time
- Reduction in per-shell alias sprawl

---

## 11. Name Definition

runex =

- **run** (execute)
- **ex** (expand / execute)
- **rune** (compressed command)

---

## 12. One-Line Definition

> runex is a rune-to-cast expansion engine.
