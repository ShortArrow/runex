# runex - Product Requirements Document

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
- Real-time expansion on space trigger
- Conditional expansion (command existence / OS / shell)
- Centralized management via a single config.toml

---

## 4. Scope

### Supported Shells

- bash
- PowerShell (pwsh)
- cmd (via Clink)
- Nushell (nu)

---

## 5. Architecture

```
config.toml
    ↓
runex core (Rust)
    ↓
shell adapters
├─ pwsh (PSReadLine)
├─ bash (readline)
├─ clink (lua)
└─ nu (script)
```

---

## 6. Functional Requirements

### 6.1 Core

- Token → expansion
- Conditional expansion
- Fallback (pass through undefined tokens)
- Shell-aware behavior

### 6.2 CLI

```bash
runex expand --token ls
runex list
runex doctor
runex export pwsh
runex export bash
runex export nu
runex export clink
```

### 6.3 Config File

`~/.config/runex/config.toml`

```toml
version = 1

[[abbr]]
key = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

---

## 7. Non-Functional Requirements

- Fast (<1ms level)
- Cross-platform (Windows / Linux / macOS)
- Shell-independent logic
- Safe (infinite loop prevention)

---

## 8. Constraints

- No full shell parser implementation
- Token-level processing only
- Quoted strings not supported initially

---

## 9. Future Extensions

- Fuzzy suggestions
- UI picker
- History-based learning
- IDE integration (Neovim, etc.)

---

## 10. Success Criteria

- All shells unified under a single config file
- Perceived reduction in typing time
- Reduction in alias count

---

## 11. Name Definition

runex =

- **run** (execute)
- **ex** (expand / execute)
- **rune** (compressed command)

---

## 12. One-Line Definition

> runex is a rune-to-cast expansion engine.
