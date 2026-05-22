# Recipes

[English](recipes.md) | [日本語](recipes.ja.md)

Practical, copy-pasteable `config.toml` snippets. Pick a recipe that
matches what you want, drop the `[[abbr]]` blocks into your config, and
hit your trigger key.

Your config file lives at `$XDG_CONFIG_HOME/runex/config.toml`
(falls back to `~/.config/runex/config.toml`). Override with
`RUNEX_CONFIG=<path>` or `runex --config <path>`.

For the full field reference see [config-reference.md](config-reference.md).
For the trigger-key setup itself see [setup.md](setup.md).

---

## 1. Common Git shortcuts

**Use case:** turn the most-typed Git invocations into 2-3 letter tokens.

```toml
[[abbr]]
key    = "gst"
expand = "git status"

[[abbr]]
key    = "gd"
expand = "git diff"

[[abbr]]
key    = "ga"
expand = "git add"

[[abbr]]
key    = "gco"
expand = "git checkout"

[[abbr]]
key    = "gp"
expand = "git push"

[[abbr]]
key    = "gpl"
expand = "git pull"
```

**Try it:** `gst<Space>` → `git status `. Press the trigger key after the
token; runex replaces the token in place and inserts the space the
trigger key would have produced anyway.

---

## 2. Use `bat` instead of `cat` when it's installed

**Use case:** prefer a richer pager when available, fall back silently
otherwise.

```toml
[[abbr]]
key    = "cat"
expand = "bat"
when_command_exists = ["bat"]
```

**Try it:** `cat<Space>file.rs` expands to `bat file.rs` if `bat` is on
your PATH; otherwise the rule is skipped and `cat` stays `cat`. Run
`runex doctor` to confirm `bat` is found — look for `command:bat: 'bat'
found (required by 'cat')`.

---

## 3. Three-step fallback chain

**Use case:** prefer `eza`, fall back to `lsd`, fall back to plain `ls`.
runex evaluates `[[abbr]]` blocks top-to-bottom and uses the first one
whose `when_command_exists` is satisfied (or has no condition).

```toml
[[abbr]]
key    = "ll"
expand = "eza --long --git --group-directories-first"
when_command_exists = ["eza"]

[[abbr]]
key    = "ll"
expand = "lsd --long --group-dirs first"
when_command_exists = ["lsd"]

[[abbr]]
key    = "ll"
expand = "ls -la"
```

**Try it:** `runex which ll --why` shows which rule matched and which
were skipped. The plain `ls -la` rule is the unconditional fallback —
it always matches if both `eza` and `lsd` are missing.

---

## 4. Cursor placeholder for "fill in the blank" commands

**Use case:** expand into a template where the cursor lands inside
quotes, ready to type. The `{}` marker controls where the cursor stops.

```toml
[[abbr]]
key    = "prc"
expand = 'gh pr create --title "{}" --body ""'

[[abbr]]
key    = "issn"
expand = 'gh issue create --title "{}" --body ""'
```

**Try it:** `prc<Space>` expands to `gh pr create --title "" --body ""`
with the cursor sitting between the title quotes. Type the title,
arrow-right to the body. No `{}` in the expansion = cursor goes to the
end (the default).

---

## 5. Per-shell trigger key

**Use case:** Alt+Space in bash (because bash's readline is more
forgiving with chord keys), Space everywhere else.

```toml
[keybind.trigger]
default = "space"
bash    = "alt-space"
```

**Try it:** in bash, hit Alt+Space after a token; in zsh / pwsh / nu /
clink, plain Space still triggers. The `default` value covers every
shell that doesn't have a specific override.

Accepted values: `"space"`, `"tab"`, `"alt-space"`, `"shift-space"`
(pwsh and nu only — see recipe 6).

---

## 6. Insert a literal space without expanding

**Use case:** in pwsh you usually want Space to expand, but sometimes
you genuinely want a plain space. Bind Shift+Space as the "skip
expansion" key.

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
pwsh = "shift-space"
nu   = "shift-space"
```

**Try it:** `gst<Shift+Space>` inserts `"gst "` literally — no
expansion. Plain `<Space>` still expands. bash and zsh can't reliably
detect Shift+Space; use `"alt-space"` there if you need an escape
hatch.

---

## 6b. nu: avoid the paste-eats-content issue

**Use case:** in nu (≥ 0.111 / reedline), pasting text that contains
spaces causes the runex binding to fire mid-paste, and the
`executehostcommand` event nu uses for keybinds resets the command
line at fire time. Everything after the first triggering space gets
dropped. This is upstream nu behaviour, not specific to runex.

**1st choice (since 0.1.14):** add a Ctrl+V binding that reads the
system clipboard directly, bypassing the per-keystroke trigger:

```toml
[keybind.trigger]
default = "space"

[keybind.paste_intercept]
nu = "ctrl-v"
```

**Try it:** paste `echo a b c d` with Ctrl+V — the whole line stays
intact. Type `gst<Space>` to expand normally. Plain Space remains
the trigger; Ctrl+V is a separate path that pipes the clipboard
into the buffer via `runex paste-clipboard` (a hidden subcommand
the binding calls automatically).

Provider chain: Windows uses native `OpenClipboard`; Linux tries
`wl-paste` → `xclip` → `xsel`; WSL adds `powershell.exe
Get-Clipboard` as a final fallback; macOS uses `pbpaste`. Install
one if `runex paste-clipboard` reports "no clipboard provider
found".

**Caveats:**
- Mouse middle-click paste and terminal right-click paste inject
  characters through the keymap (not Ctrl+V), so they remain
  affected by the upstream limitation. Use Ctrl+V from the keyboard
  or fall back to choice 2 below.
- **Windows Terminal intercepts Ctrl+V** before nu sees it (it is
  the default `paste` binding). Either use a different terminal
  (WezTerm and Alacritty are confirmed to pass Ctrl+V through), or
  remap the Windows Terminal binding in `settings.json`. macOS
  Terminal.app and most Linux terminal emulators pass Ctrl+V to nu
  unchanged.

**2nd choice — switch the trigger to a chord paste streams cannot
contain:**

```toml
[keybind.trigger]
default = "space"
nu      = "shift-space"
```

Then paste `echo a b c d` and type `gst<Shift+Space>` to expand.
Bash/zsh/pwsh/clink keep plain Space as their trigger because they
handle paste-time chord events correctly (pwsh sets a paste-pending
flag, clink only fires the lua binding on standalone keypresses,
bash/zsh have no trigger-on-paste race in the first place).

---

## 6c. WSL + mise: keystroke latency from `runex` PATH lookup

**Use case:** if you noticed `Space` causing the prompt to blank
for ~1 second on WSL Linux before the expansion landed, you were
hitting per-keystroke `mise` startup overhead. The static integration
cache (the modern `runex init` layout) fixes this at the source.

**Symptom (0.1.14 and earlier):**

- Your bashrc has `eval "$(runex export bash)"` from `runex init`.
- Your `$PATH` has `~/.local/share/mise/shims` ahead of
  `~/.cargo/bin` (the standard `mise activate` setup).
- `mise install` placed a `runex` shim under
  `~/.local/share/mise/shims/runex`.
- Every Space press invokes `__runex_expand`, which calls
  `'runex' hook ...`. PATH resolves `runex` to the mise shim,
  which spawns the real `mise` binary, which then `exec`s the
  actual runex. Result: ~470 ms per keystroke before the hook
  even runs (measured: 0m0.474s through the shim, 0m0.002s direct).

**Fix:** re-run `runex init <shell>` once.

```bash
runex init bash --yes
exec bash    # or open a new terminal
```

That writes `~/.cache/runex/integration.bash` with the absolute
`runex` path baked in, replaces the rcfile's `eval $(...)` line
with a static `source` of that cache file, and per-keystroke
hook calls go directly to the real binary — no shim, no PATH
walk, no measurable latency.

**Verify:**

```sh
runex doctor
# integration:bash:cache: cache up-to-date at ~/.cache/runex/integration.bash
```

If you saw `Outdated WARN` instead, that's the legacy cache; the
re-init above clears it.

**Related minor speedup:** `runex hook` also runs
`when_command_exists` checks via `which::which`, which walks
`$PATH`. On WSL the inherited Windows PATH adds 90+ entries under
`/mnt/c/...` that get stat()ed over 9p. If you don't need any
Windows tools from inside WSL, drop those entries from `$PATH`
in your bashrc:

```bash
PATH=$(echo "$PATH" | tr ':' '\n' | grep -v '^/mnt/c/' | paste -sd ':' -)
export PATH
```

This is purely a `which::which` cache-miss optimisation; it's
optional and unrelated to the runex install. The static cache
file means runex itself no longer cares about
your PATH shape after init.

---

## 7. Different commands on Windows and Unix

**Use case:** `rm -i` on Unix, `Remove-Item` in PowerShell.

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
```

**Try it:** `rmf<Space>foo.txt` expands to `rm -i foo.txt` in bash and
`Remove-Item foo.txt` in PowerShell. Other shells without a specific
override fall back to `default`. The same per-shell table form works
for `when_command_exists`:

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
when_command_exists = { default = ["rm"], pwsh = ["Remove-Item"] }
```

---

## 8. Platform-specific dependency check

**Use case:** the abbreviation depends on a tool that's only available
on certain platforms — `wslpath` only exists inside WSL, for example.
On the platform that doesn't need a precondition (here pwsh, where the
expansion uses native PowerShell variables), an empty
`when_command_exists` list says "no precondition, always expand".

```toml
[[abbr]]
key    = "winhome"
expand = { default = "/mnt/c/Users/$USER", pwsh = "$env:USERPROFILE" }
when_command_exists = { default = ["wslpath"], pwsh = [] }
```

**Try it:** in WSL bash, `winhome<Space>` expands only when `wslpath`
is on PATH (i.e. you really are inside WSL). In pwsh the empty list
short-circuits the precondition — always expand. An empty
`when_command_exists` list is treated as "no condition", not "fail".

---

## 9. `sudo` doesn't break expansion

**Use case:** you want abbreviations to keep working after `sudo`.
runex's command-position detection treats `sudo <token>` the same as
`<token>` at the start of the line — so does `|`, `||`, `&&`, `;`.

```toml
[[abbr]]
key    = "apt-update"
expand = "apt update"
```

**Try it:**

```
sudo apt-update<Space>
```

expands to `sudo apt update `. `runex which apt-update --why` will
confirm the rule matched. Same trick works after `|`, `||`, `&&`,
`;`, and `sudo` itself.

### Pitfall: `sudo <abbr>` does **not** propagate `sudo` across `&&`

`sudo` only applies to the single command it prefixes. If the
expansion contains `&&` or `;`, every command on the right of those
separators runs as your normal user. So this trips people up:

```toml
[[abbr]]
key    = "apt-up"
expand = "apt update && apt upgrade"   # WRONG: apt upgrade won't be root
```

```
sudo apt-up<Space>
# expands to: sudo apt update && apt upgrade
# `apt update` runs as root, `apt upgrade` runs as you and fails.
```

If the whole pipeline needs root, bake `sudo` into each command and
type the abbr without `sudo` in front of it (issue #4):

```toml
[[abbr]]
key    = "aptup"
expand = "sudo apt update && sudo apt upgrade"   # OK: both as root
```

```
aptup<Space>
# expands to: sudo apt update && sudo apt upgrade
```

Rule of thumb:

- **Single command** → put `sudo` on the command line (`sudo abbr`)
  and leave it out of the `expand` value.
- **Multi-command (`&&`, `;`, `|`)** → bake `sudo` into every command
  in `expand` and call the abbr bare.

---

## 10. Docker and kubectl command bundle

**Use case:** flatten the most common container management invocations
into 2-4 letter tokens.

```toml
[[abbr]]
key    = "dps"
expand = "docker ps"

[[abbr]]
key    = "dpsa"
expand = "docker ps -a"

[[abbr]]
key    = "dimg"
expand = "docker images"

[[abbr]]
key    = "dexec"
expand = "docker exec -it"

[[abbr]]
key    = "kg"
expand = "kubectl get"

[[abbr]]
key    = "kgp"
expand = "kubectl get pods"

[[abbr]]
key    = "kga"
expand = "kubectl get all"

[[abbr]]
key    = "kdp"
expand = "kubectl describe pod"

[[abbr]]
key    = "klog"
expand = "kubectl logs -f"
```

**Try it:** scope each tool with a distinctive prefix (`d` for docker,
`k` for kubectl) so the keys don't collide with abbreviations from
other categories.

---

## 11. Dealing with a name conflict against an existing alias

**Use case:** you wrote `key = "ls"` but your shell already has an
`alias ls=...`, so the rule never fires (the shell expands the alias
before runex's hook runs).

`runex doctor` flags this:

```
[WARN]  shell:bash:key:ls: conflicts with existing alias 'ls' -> ls --color=auto
```

You have two ways out:

```toml
# Option A: rename the abbreviation key.
[[abbr]]
key    = "ll"
expand = "lsd"
when_command_exists = ["lsd"]
```

```bash
# Option B: drop the conflicting alias from your rcfile.
unalias ls 2>/dev/null
```

**Try it:** after the change, run `runex doctor` again. The warning
should be gone.

---

## 12. "Why isn't my command being found?"

**Use case:** `runex doctor` reports `command:foo: 'foo' not found`
even though `which foo` works in your interactive shell.

Read the doctor output:

```
[OK]    effective_search_path: 116 entries (process=101, +user=0, +system=15)
[WARN]  command:foo: 'foo' not found (required by 'bar')
```

Cross-check:

- **`+user=0`** on Windows: the User-scope `Environment\Path` from the
  registry contributed nothing on top of the inherited PATH. If `foo`
  lives under `~/AppData/Local/...`, the parent process's PATH might be
  degraded. See [setup.md → Troubleshooting](setup.md#troubleshooting).
- **PATH unset entirely**: `effective_search_path` row would be `WARN`
  with `process=0`. Restart the shell with a clean environment.
- **`foo` isn't on any registered PATH**: install it, or add its
  install dir to your shell's PATH and rerun `runex doctor`.

---

## 13. Looking up one abbreviation in a long list

**Use case:** your config has grown past a screenful of rules and
`runex list` scrolls off the top before you can find the one you
care about.

```bash
runex list ll
# ll<TAB>ls -la
```

Pass the key as a positional argument to `runex list` and only the
matching entry is printed. The match is exact and case-sensitive —
`runex list ll` does not also print `ll.` — so a hit is unambiguous
and a non-match is silent (`runex list nope` exits 0 with no output,
which is the same shape a `[[ -z "$(runex list X)" ]]` script needs).

The filter applies to `--json` the same way:

```bash
runex list ll --json
# [
#   { "key": "ll", "expand": "ls -la", "when_command_exists": null }
# ]
```

For prefix / substring / fuzzy lookup, use `runex which <token>`
instead — it reports the same key plus the per-shell expansion and any
`when_command_exists` gates that were applied.

---

## 14. Numeric repetition with `{number}`

**Use case:** writing `up`, `up2`, `up3`, … `up10` as separate rules
when they all just want to repeat the same unit (`../`) gets old fast.

```toml
[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"
```

```
up3<Space>     # → cd ../../../
up10<Space>    # → cd ../../../../../../../../../../
```

The `{number}` placeholder appears in both the `key` (where it
captures the trailing digits) and the `expand` (where it gets
replaced by `number * <captured count>`).

### Coexisting with exact rules

Exact-key rules always win when they could also match the token.
That means you can layer special cases on top of a pattern:

```toml
[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"

[[abbr]]
key    = "up"          # bare `up` doesn't match the pattern (no digits)
expand = "cd .."

[[abbr]]
key    = "up3"         # special case wins for `up3` even when the
expand = "cd ~/notes"  # pattern would also handle it
```

```
up<Space>      # → cd ..
up2<Space>     # → cd ../../
up3<Space>     # → cd ~/notes   (exact rule wins)
up4<Space>     # → cd ../../../../
```

### Limits and gotchas

- `{number}` is the only recognised placeholder today; `{foo}` or
  any other `{...}` shape is rejected at config-parse time.
- The captured number must be 1–128. `up0` and `up129` pass through
  unchanged (no expansion).
- The `number` unit is capped at 32 bytes so the rendered result
  cannot exceed the existing 4096-byte limit on `expand`.
- Only ASCII decimal digits count — `up3<Space>` works, `up３`
  (full-width) does not.
- The cursor placeholder `{}` works in the same template; named
  substitution happens first, then `{}` is removed and the cursor
  lands there.

---

## Next steps

- Full field reference: [config-reference.md](config-reference.md)
- Shell setup details: [setup.md](setup.md)
- Troubleshooting: [setup.md → Troubleshooting](setup.md#troubleshooting)
- Run `runex doctor` to verify any change.
