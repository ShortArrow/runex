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

**Use case:** the abbreviation depends on a tool that's named differently
per platform — `wsl` from inside Windows pwsh, `lsb_release` on Linux.

```toml
[[abbr]]
key    = "winhome"
expand = { default = "/mnt/c/Users/$USER", pwsh = "$env:USERPROFILE" }
when_command_exists = { default = ["wslpath"], pwsh = [] }
```

**Try it:** in WSL bash, `winhome<Space>` expands only when `wslpath`
exists. In pwsh the empty list means "no precondition" — always
expand. An empty `when_command_exists` list is treated as "no
condition", not "fail".

---

## 9. `sudo` doesn't break expansion

**Use case:** you want abbreviations to keep working after `sudo`.
runex's command-position detection treats `sudo <token>` the same as
`<token>` at the start of the line.

```toml
[[abbr]]
key    = "apt-up"
expand = "apt update && apt upgrade"
```

**Try it:**

```
sudo apt-up<Space>
```

expands to `sudo apt update && apt upgrade `. Same with `|` and `&&`:
runex recognises `<token>` after `|`, `||`, `&&`, `;`, and `sudo` as
command position. `runex which <token> --why` will show the rule
matched.

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

## Next steps

- Full field reference: [config-reference.md](config-reference.md)
- Shell setup details: [setup.md](setup.md)
- Troubleshooting: [setup.md → Troubleshooting](setup.md#troubleshooting)
- Run `runex doctor` to verify any change.
