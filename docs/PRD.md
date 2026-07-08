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
  (config, doctor, init, precache).
- **`infra/`** — file / registry / env access (env with
  `HomeDirResolver`, integration_check).
- **`cmd/`** — CLI subcommand handlers (one file per `Commands`
  enum variant).
- **`util/`** — leaf helpers (shell detection, command_exists
  factory, prompt).

Dependency direction: `cmd → app → domain`, `cmd → util/infra`,
`infra → domain` (one-way, no cycles). Pre-0.1.14 the same code
lived in two crates (`runex-core` + `runex`); the split was
removed in Phase C because the internal `pub` boundary it carried
served no external consumer.

---

## 6. Functional Requirements

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
runex which <token>                      show which rule matches
runex which <token> --why                show full match trace with skip reasons
runex doctor                             check config and environment
runex doctor --no-shell-aliases          skip alias conflict checks (avoids spawning shells)
runex doctor --strict                    also warn about unknown config fields
runex add <key> <expand>                 add an abbreviation rule to config
runex add <key> <expand> --when <cmd>    add with when_command_exists condition
runex remove <key>                       remove an abbreviation rule from config
runex init                               create config and append shell integration (auto-detect shell)
runex init <shell>                       target a specific shell (bash/zsh/pwsh/clink/nu)
runex init -y                            same, skip confirmation prompts
runex export <shell>                     generate shell integration script
runex export <shell> --bin <name>        use a custom binary name in the script
runex timings <key>                      show per-phase timing breakdown of expand
runex timings                            time all abbreviation rules
runex config where                       print the resolved config file path
runex config type                        print the config file contents to stdout
runex config show                        open the config file with the OS-associated app
runex version                            show version and build commit
```

Global flags (accepted by every subcommand):

```
--config <path>      override config file path (overrides RUNEX_CONFIG)
--path-prepend <dir> prepend directory to PATH for command existence checks
--json               JSON output (supported by: list, doctor, version, expand, which, timings, config where)
```

### 6.3 Config File

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

## 7. Non-Functional Requirements

- Fast: expansion path completes in <1 ms
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
  shell templates reduced to thin wrappers (244 lines total across
  five shells).
- `runex doctor` now reports environment-level health: Windows
  `effective_search_path` breakdown and `integration:<shell>` rcfile
  marker / clink-lua drift detection.
- `runex init <shell>` accepts a shell positional and writes the clink
  lua integration directly (no more manual `runex export clink > …`).
  Seed config includes a working sample so `init` produces an
  immediately verifiable setup. Per-shell "Next steps" guidance after
  init.
- crates.io publish moved into CI via OIDC Trusted Publishing — no
  long-lived `CARGO_REGISTRY_TOKEN` anywhere. Test gate added so a
  tag push can never ship binaries from a commit whose tests didn't
  finish.
- `docs/recipes.md` cookbook with 12 use-case-driven `config.toml`
  snippets.

### Near-term

- Continue refining diagnostics surfaced by `doctor` and `init` as
  new failure modes are observed in the wild.
- **Strengthen end-to-end test coverage.** Today's CI exercises CLI
  subcommands and shell-helper functions invoked directly, but does
  not drive an actual key press through readline / clink / PSReadLine.
  The 0.1.12 clink regression (silent fallback to literal space when
  the cmd host's PATH was degraded) would have been caught by a
  PTY-driven keystroke test. Concrete items:
  - `runex init` rcfile-write property tests (append-only,
    `O_NOFOLLOW`, marker-idempotent, size-cap) against a `tempdir`
    HOME — low cost, codifies the safety guarantees the docs already
    promise.
  - PTY-based real-keystroke test using `expectrl` so a Space key
    press through bash's bind table is asserted to mutate the buffer
    as expected.
  - clink and nu integration tests parallel to the existing
    bash/zsh/pwsh ones (zero coverage today).

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
