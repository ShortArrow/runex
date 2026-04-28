# Config Reference

Default path: `$XDG_CONFIG_HOME/runex/config.toml`, falling back to `~/.config/runex/config.toml` on all platforms including Windows.

Override with the `RUNEX_CONFIG` environment variable or the `--config` flag.

---

> **Note:** Unknown fields and table names are silently ignored. Typos like `[[abr]]` or `expad = "lsd"` will not produce an error — the misspelled entry is simply skipped. Use `runex doctor` to verify that your config loads correctly.

---

## Top-level fields

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | integer | yes | Schema version. Must be `1`; other values are rejected at load time. |
| `keybind` | table | no | Trigger key configuration. If omitted, no key is bound. |
| `precache` | table | no | **Deprecated since 0.2.0.** Retained for backward compatibility but has no run-time effect. See [`[precache]` (deprecated)](#precache-deprecated) below. |
| `abbr` | array of tables | no | Abbreviation rules. Evaluated in order. |

---

## `[keybind]`

Controls which key triggers expansion in each shell. Both subtables are optional.

`[keybind]` has two subtables:

- `[keybind.trigger]` — the key that triggers abbreviation expansion
- `[keybind.self_insert]` — a key that inserts a plain space without expanding (optional)

Each subtable accepts the following fields:

| Field | Type | Default | Description |
|---|---|---|---|
| `default` | string | — | Key for all shells not otherwise specified. |
| `bash` | string | falls back to `default` | bash-specific override. |
| `zsh` | string | falls back to `default` | zsh-specific override. |
| `pwsh` | string | falls back to `default` | PowerShell-specific override. |
| `nu` | string | falls back to `default` | Nushell-specific override. |

Clink does not have a shell-specific field — it always uses `default`.

If `default` and all shell-specific fields are omitted, **no key is bound** for that subtable. The shell integration script is still generated; expansion is available via `runex expand --token <token>`.

Shell-specific fields take precedence over `default`:

```toml
[keybind.trigger]
default = "space"
bash    = "alt-space"   # bash uses Alt+Space; other shells use Space
```

### Accepted key values

| Value | Key |
|---|---|
| `"space"` | Space bar |
| `"tab"` | Tab |
| `"alt-space"` | Alt + Space |
| `"shift-space"` | Shift + Space (pwsh and nu only) |

### `[keybind.self_insert]`

Binds a key to plain-space insertion, bypassing expansion entirely. Useful when the trigger key has a modifier variant that would otherwise fall through to the expansion handler.

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
default = "shift-space"   # Shift+Space inserts a space without expanding
```

Shell support for `self_insert`:

| Shell | `"alt-space"` | `"shift-space"` |
|---|---|---|
| bash | yes | no (terminal-dependent — use `alt-space` instead) |
| zsh | yes | no (terminal-dependent — use `alt-space` instead) |
| pwsh | yes | yes |
| nu | yes | yes |
| clink | — | — |

> **Note:** Setting `self_insert.default = "shift-space"` (or `self_insert.bash`/`self_insert.zsh`) produces a warning from `runex doctor` because Shift+Space cannot be reliably detected in bash or zsh. Use `"alt-space"` for cross-shell support.

---

## `[precache]` (deprecated)

> [!IMPORTANT]
> **Deprecated since 0.2.0.** This section has no run-time effect.
> `runex doctor --strict` warns when it is present. Remove the section from your config to silence the warning.

Earlier versions used the shell integration to populate a per-session command-existence cache at rc/profile time. With the move to the [`runex hook`](#how-shell-integration-works) per-keystroke RPC, `when_command_exists` is now evaluated against `which::which` (and the optional `--path-prepend` directory) every time the hook fires; the precache layer is gone and the section's `path_only` field is ignored.

If you previously relied on `path_only = false`'s shell-native detection (cmdlets, aliases, user-defined functions), expect those rules to behave like `path_only = true` did before — only PATH-resolvable binaries match. Re-architecting that detection on top of `runex hook` is tracked separately.

---

## How shell integration works

`runex export <shell>` emits a small bootstrap that registers a key handler
on the trigger key. When the user presses that key the handler invokes the
hidden subcommand `runex hook`, passing the current buffer and cursor as
arguments:

```
runex hook --shell <shell> --line "<buffer>" --cursor <byte_offset>
```

The Rust core decides whether to expand (command-position detection,
known-token check, `when_command_exists`, cursor-placeholder handling) and
emits a shell-specific directive that the bootstrap evaluates. The five
shells use different output formats but the same flow:

| shell | hook output format |
|---|---|
| bash | `READLINE_LINE='...'; READLINE_POINT=N` (eval'd) |
| zsh | `LBUFFER='...'; RBUFFER='...'` (eval'd) |
| pwsh | `$__RUNEX_LINE = '...'; $__RUNEX_CURSOR = N` (Invoke-Expression'd) |
| clink | `return { line = "...", cursor = N }` (Lua `load()` in a sandbox) |
| nu | `{"line": "...", "cursor": N}` (parsed via `from json`) |

Failures (missing config, malformed buffer) are silent: the hook returns
an `InsertSpace` action so the bootstrap inserts a literal trigger key and
the user keeps typing. Configuration changes take effect on the next
keypress because the hook reads the config every time — no shell restart
required after `runex add` / `runex remove`.

---

## `[[abbr]]`

Each `[[abbr]]` entry defines one abbreviation rule.

| Field | Type | Required | Description |
|---|---|---|---|
| `key` | string | yes | The short token to match. |
| `expand` | string or per-shell table | yes | The full text to expand into. See [per-shell form](#per-shell-form) below. |
| `when_command_exists` | array of strings or per-shell table | no | Only expand if **all** listed commands resolve via `which` at hook time. |

#### Per-shell form

Both `expand` and `when_command_exists` accept either a flat value (applied to every shell) or a per-shell table. The table fields are `default`, `bash`, `zsh`, `pwsh`, and `nu` — any missing shell falls back to `default`. Clink always uses `default`.

```toml
# Flat (shared across shells)
[[abbr]]
key    = "gcm"
expand = "git commit -m"
when_command_exists = ["git"]

# Per-shell (different expansions / conditions per shell)
[[abbr]]
key = "rm"
expand = { default = "rm -i", pwsh = "Remove-Item" }
when_command_exists = { default = ["rm"], pwsh = ["Remove-Item"] }
```

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

If any listed command cannot be resolved, the rule is skipped (not an error). Runex continues to the next rule with the same key. Resolution uses [`which::which`](https://crates.io/crates/which) and honors `--path-prepend <dir>` when set.

Use `--path-prepend <dir>` to inject a directory into the command existence check without modifying the system PATH:

```
$ runex --path-prepend /tmp/fake-bins which ls
```

### Expansion text

`expand` is inserted verbatim as shell-native text. Runex does not re-escape or reinterpret it. Shell quoting and special characters are passed through as-is.

### Cursor placeholder

Use `{}` in the expansion text to control where the cursor lands after expansion:

```toml
[[abbr]]
key    = "gcam"
expand = "git commit -am '{}'"
```

When `gcam` is expanded, the cursor will be placed between the quotes instead of at the end. If `{}` is absent, the cursor goes to the end of the expansion (default behaviour).

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

Entries must be bare command names (`lsd`, `bat`, `eza`). Beyond the path separators `/`, `\`, `:`, the following characters are rejected at config load time to keep `which`-style resolution unambiguous and to defend against any future code path that might re-introduce shell-side resolution:

- Shell metacharacters: `&` `|` `;` `<` `>` `` ` `` `$` `(` `)` `{` `}` `'` `"`
- cmd.exe metacharacters: `%` `^`
- Whitespace: space, tab
- Glob patterns: `*` `?` `[` `]`
- Precache protocol delimiters: `,` `=`
- Other risky punctuation: `!` `#` `~`

To check a command installed outside your normal PATH, use `--path-prepend` at runtime instead.

---

## Environment variables

| Variable | Description |
|---|---|
| `RUNEX_CONFIG` | Override config file path. Overridden by `--config`. |

---

## Full example

```toml
version = 1

[keybind.trigger]
default = "space"
bash    = "alt-space"

[keybind.self_insert]
pwsh = "shift-space"
nu   = "shift-space"

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

### `runex timings`

Shows per-phase timing breakdown of the expand flow, similar to `starship timings`:

```
$ runex timings ls --shell bash
 Phase                        Duration
 ──────────────────────────────────────
 config_load                  1.23ms
 shell_resolve                0.01ms
 expand                       5.67ms
   command_exists: lsd         3.12ms
   command_exists: ls          2.34ms
 ──────────────────────────────────────
 Total                        6.91ms
```

When the key argument is omitted, all abbreviation keys are timed. Use `--json` for machine-readable output.

### `runex doctor` — rejected rule diagnostics

When a rule fails per-field validation, `parse_config` returns the first error it encounters, which is the message shown in `config_parse`. `doctor` always also lists **every** rejected rule with its field path so you can see what else needs fixing:

```
$ runex doctor
[OK]    config_file: found: ~/.config/runex/config.toml
[ERROR] config_parse: failed to load config: abbr rule #1: key is empty (...)
[WARN]  config_rejected_rules: 3 invalid abbr field(s) found; config loading still stops at the first one
[WARN]  config_validation.abbr[1].key: rule #1 field 'key' rejected: key is empty
[WARN]  config_validation.abbr[2].expand: rule #2 field 'expand' rejected: expand is empty
[WARN]  config_validation.abbr[3].when_command_exists[1]: rule #3 field 'when_command_exists[1]' rejected: when_command_exists entry contains a shell metacharacter or glob pattern
```

Array indices and rule numbers in the field path are 1-based. The field path is a logical path — it mirrors the in-memory shape (e.g. `expand.pwsh`, `when_command_exists.default[2]`), not literal TOML syntax.

### `runex doctor` — environment & integration health

Beyond config validation, `doctor` surfaces two categories of
environment-level checks. Each row's status (`OK` / `WARN`) tells you
whether action is required.

| Check | Status | Meaning |
|-------|--------|---------|
| `effective_search_path` *(Windows-only)* | `OK` | Reports the PATH runex uses when resolving `when_command_exists` entries. The breakdown `entries (process=N, +user=M, +system=K)` shows how many came from the inherited process PATH versus the registry's HKCU and HKLM `Environment\Path`. If `+user` or `+system` is non-zero, the parent process inherited a degraded PATH and runex augmented it from the registry. Useful for diagnosing `command:foo not found` warnings that contradict your shell's PATH. |
| `effective_search_path` *(Windows-only)* | `WARN` | The process PATH is empty — almost certainly a misconfigured launcher. |
| `integration:bash` / `:zsh` / `:pwsh` / `:nu` | `OK` | The `# runex-init` marker is present in the rcfile (so `eval "$(runex export <shell>)"` is wired up), or the rcfile doesn't exist (treated as "user doesn't run that shell"). |
| `integration:bash` / `:zsh` / `:pwsh` / `:nu` | `WARN` | The rcfile exists but lacks the marker. Run `runex init <shell>` to install the integration line. |
| `integration:clink` | `OK` | The `runex.lua` on disk matches what `runex export clink` would emit today, or no clink integration is found (treated as "user doesn't run clink"). |
| `integration:clink` | `WARN` | The on-disk `runex.lua` has drifted from the current export — typical after upgrading runex. Re-run `runex export clink > %LOCALAPPDATA%\clink\runex.lua`. |

`integration:clink` is a content comparison rather than a marker check
because the clink lua file is a static copy with no auto-refresh path.
bash/zsh/pwsh/nu re-source `runex export <shell>` on every shell start
so they can't drift.

The `RUNEX_CLINK_LUA_PATH` environment variable overrides the search
location used by the clink check (default candidates: `%LOCALAPPDATA%\clink\runex.lua`,
then `~/.local/share/clink/runex.lua` for non-Windows clink forks).

### `runex doctor --strict`

Warns about unknown fields in the config file and unreachable duplicate rules. Useful for catching typos:

```
$ runex doctor --strict
[OK]    config_file: found: ~/.config/runex/config.toml
[OK]    config_parse: config loaded successfully
[WARN]  strict.unknown_field.abr: unknown top-level field 'abr' (did you mean 'abbr'?)
[WARN]  strict.unknown_field.abbr[1].expad: unknown field 'expad' in abbr[1] (did you mean 'expand'?)
```

### `runex add` / `runex remove`

Edit abbreviation rules from the command line without opening the config file:

```bash
# Add a rule
runex add gcm "git commit -m"

# Add with when_command_exists condition
runex add ls lsd --when lsd

# Remove a rule
runex remove gcm
```

See also: [Commands — doctor](../README.md#commands), [Commands — which](../README.md#commands).
