# Config Reference

Default path: `$XDG_CONFIG_HOME/runex/config.toml`, falling back to `~/.config/runex/config.toml` on all platforms including Windows.

Override with the `RUNEX_CONFIG` environment variable or the `--config` flag.

---

## Top-level fields

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | integer | yes | Schema version. Currently unused for validation — set to `1`. |
| `keybind` | table | no | Trigger key configuration. If omitted, no key is bound. |
| `abbr` | array of tables | no | Abbreviation rules. Evaluated in order. |

---

## `[keybind]`

Controls which key triggers expansion in each shell. All fields are optional.

| Field | Type | Default | Description |
|---|---|---|---|
| `trigger` | string | — | Default trigger for all shells. |
| `bash` | string | falls back to `trigger` | bash-specific override. |
| `zsh` | string | falls back to `trigger` | zsh-specific override. |
| `pwsh` | string | falls back to `trigger` | PowerShell-specific override. |
| `nu` | string | falls back to `trigger` | Nushell-specific override. |
| `self_insert` | string | — | Key to bind to plain-space insertion (bypasses expansion). |

Clink does not have a shell-specific field — it always uses `trigger`.

If `trigger` and all shell-specific fields are omitted, **no key is bound**. The shell integration script is still generated; expansion is available via `runex expand --token <token>`.

Shell-specific fields take precedence over `trigger`:

```toml
[keybind]
trigger = "space"
bash    = "alt-space"   # bash uses Alt+Space; other shells use Space
```

### Accepted key values

| Value | Key |
|---|---|
| `"space"` | Space bar |
| `"tab"` | Tab |
| `"alt-space"` | Alt + Space |
| `"shift-space"` | Shift + Space (pwsh and nu only) |

### `self_insert`

Binds a key to plain-space insertion, bypassing expansion entirely. Useful when the trigger key has a modifier variant that would otherwise fall through to the expansion handler.

```toml
[keybind]
trigger     = "space"
self_insert = "shift-space"   # Shift+Space inserts a space without expanding
```

Shell support for `self_insert`:

| Shell | `"alt-space"` | `"shift-space"` |
|---|---|---|
| bash | yes | no (terminal-dependent — use `alt-space` instead) |
| zsh | yes | no (terminal-dependent — use `alt-space` instead) |
| pwsh | yes | yes |
| nu | yes | yes |
| clink | — | — |

> **Note:** Setting `self_insert = "shift-space"` produces a warning from `runex doctor` because Shift+Space cannot be reliably detected in bash or zsh. Use `"alt-space"` for cross-shell support.

---

## `[[abbr]]`

Each `[[abbr]]` entry defines one abbreviation rule.

| Field | Type | Required | Description |
|---|---|---|---|
| `key` | string | yes | The short token to match. |
| `expand` | string | yes | The full text to expand into. |
| `when_command_exists` | array of strings | no | Only expand if **all** listed commands are present on PATH. |

### Evaluation order

Rules are evaluated top-to-bottom. For each rule:

1. If `key` does not match the input token, skip.
2. If `key == expand` (self-loop), skip and continue to the next rule.
3. If `when_command_exists` lists a command that is not found, skip and continue to the next rule.
4. Otherwise, expand and stop.

This means multiple rules with the same `key` work as a priority-ordered fallback chain:

```toml
# Rule #1: use lsd when available
[[abbr]]
key    = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

# Rule #2: fallback when lsd is absent
[[abbr]]
key    = "ls"
expand = "ls --color=auto"
```

Use `runex which <token> --why` to see which rule matched and which were skipped.

### Self-loop guard

If `key == expand`, the rule is silently skipped. This prevents a broken config from causing infinite expansion:

```toml
# No-op — ls stays ls
[[abbr]]
key    = "ls"
expand = "ls"
```

### `when_command_exists`

If any listed command is absent from PATH, the rule is skipped (not an error). Runex continues to the next rule with the same key.

Use `--path-prepend <dir>` to inject a directory into the command existence check without modifying the system PATH:

```
$ runex --path-prepend /tmp/fake-bins which ls
```

### Expansion text

`expand` is inserted verbatim as shell-native text. Runex does not re-escape or reinterpret it. Shell quoting and special characters are passed through as-is.

### Field limits and rejected characters

Runex validates each field at config load time and rejects invalid values with a clear error.

**Length limits:**

| Field | Maximum |
|---|---|
| `key` | 1 024 bytes |
| `expand` | 4 096 bytes |
| `when_command_exists` — each entry | 255 bytes |
| `when_command_exists` — number of entries | 64 |

**Rejected characters** (in `key`, `expand`, and `when_command_exists` entries):

- ASCII control characters (U+0000–U+001F, U+007F)
- Unicode visual-deception characters — zero-width spaces, bidirectional overrides (RLO/LRO), BOM (U+FEFF), and similar invisible code points

**`when_command_exists` — command names only:**

Entries must be bare command names (`lsd`, `bat`, `eza`). Path separators (`/`, `\`, `:`) are not allowed. To check a command installed outside your normal PATH, use `--path-prepend` at runtime instead.

---

## Environment variables

| Variable | Description |
|---|---|
| `RUNEX_CONFIG` | Override config file path. Overridden by `--config`. |

---

## Full example

```toml
version = 1

[keybind]
trigger     = "space"
bash        = "alt-space"
self_insert = "shift-space"

# lsd as ls — only when lsd is installed
[[abbr]]
key    = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

# Fallback ls
[[abbr]]
key    = "ls"
expand = "ls --color=auto"

# Git shortcuts
[[abbr]]
key    = "gcm"
expand = "git commit -m"

[[abbr]]
key    = "gp"
expand = "git push"

[[abbr]]
key    = "gst"
expand = "git status"

# bat as cat — only when bat is installed
[[abbr]]
key    = "cat"
expand = "bat"
when_command_exists = ["bat"]
```

---

## Debugging

### `runex which <token> --why`

Shows which rule matched and why earlier rules were skipped:

```
$ runex which ls --why
ls  ->  ls --color=auto
  rule #1 skipped: when_command_exists [lsd: NOT FOUND]
  rule #2 matched, no conditions
```

### `runex expand --token <token> --dry-run`

Simulates expansion and shows the full trace without touching the shell:

```
$ runex expand --token ls --dry-run
token: ls
rule #1 skipped: when_command_exists
  lsd: NOT FOUND
matched rule #2 (key = 'ls')
conditions: none
result: expanded  ->  ls --color=auto
```

```
$ runex expand --token xyz --dry-run
token: xyz
no rule matched 'xyz'
result: pass-through
```

See also: [Commands — doctor](../README.md#commands), [Commands — which](../README.md#commands).
