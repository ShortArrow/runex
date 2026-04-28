# Setup

[English](setup.md) | [日本語](setup.ja.md)

Once runex is installed, wire it up to your shell. `runex init` covers the common case; per-shell manual steps are below.

## Quickest path: `runex init`

`runex init` creates the config file and appends the shell integration line to your rc file, with a confirmation prompt at each step:

```
$ runex init
Create config at ~/.config/runex/config.toml? [y/N] y
Created: ~/.config/runex/config.toml
Append shell integration to ~/.bashrc? [y/N] y
Appended integration to ~/.bashrc
```

Pass `-y` to skip all prompts. Clink must be set up manually; see below.

## Manual setup per shell

### bash

Requires bash 4.0 or later. macOS ships bash 3.2; install a newer version via Homebrew (`brew install bash`).

Add to `~/.bashrc`:

```bash
eval "$(runex export bash)"
```

### zsh

Add to `~/.zshrc`:

```zsh
eval "$(runex export zsh)"
```

### PowerShell

Add to `$PROFILE`:

```powershell
Invoke-Expression (& runex export pwsh | Out-String)
```

Pasted text is inserted without mid-paste expansion (runex detects the paste via PSReadLine's key queue and skips the space handler).

### Nushell

Add to `~/.config/nushell/config.nu`:

```nu
source ~/.config/nushell/runex.nu
```

Then generate the script (re-run after config changes or after upgrading runex):

```nu
runex export nu | save --force ~/.config/nushell/runex.nu
```

### cmd (Clink)

Add to Clink's script directory (re-run after config changes or after upgrading runex):

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## Next steps

- Configure your abbreviations: see the [README Config section](../README.md#config) and [config-reference](config-reference.md)
- Run `runex doctor` to verify your setup

## Verifying with `runex doctor`

`runex doctor` reports config validation, command resolution, and
shell-integration health in one pass:

```
$ runex doctor
[OK]  config_file: found: ~/.config/runex/config.toml
[OK]  config_parse: config loaded successfully
[OK]  effective_search_path: 116 entries (process=101, +user=0, +system=15)
[OK]  integration:bash: marker found in ~/.bashrc
[OK]  integration:zsh: rcfile not found at ~/.zshrc — assuming this shell is not in use
[WARN] integration:pwsh: marker missing in ~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1 — run `runex init pwsh`
[OK]  integration:nu: rcfile not found at ~/.config/nushell/env.nu — assuming this shell is not in use
[OK]  integration:clink: up-to-date at ~/AppData/Local/clink/runex.lua
[OK]  command:lsd: 'lsd' found (required by 'ls')
```

Two of these rows are worth highlighting:

- **`effective_search_path`** *(Windows-only)*: a summary of the PATH
  runex actually searches when checking `when_command_exists`. On
  Windows, runex falls back to the registry's HKCU/HKLM
  `Environment\Path` if the inherited process PATH is missing entries
  (this happens with clink's lua-spawned cmd children). The
  `+user=N, +system=K` numbers tell you how many extra entries the
  registry contributed beyond the process PATH.
- **`integration:<shell>`**: tells you whether each shell has been
  hooked up. bash/zsh/pwsh/nu look for the `# runex-init` marker in
  the rcfile. clink is special — the lua file is a static copy that
  doesn't auto-refresh, so the check compares the on-disk
  `runex.lua` against what `runex export clink` would emit today and
  warns on drift.

## Troubleshooting

If a trigger key produces a literal space instead of expanding:

1. **First, run `runex doctor`.** Most regressions surface here.
2. **`runex hook` errors with `unrecognized subcommand`.** A different
   `runex` binary on your `PATH` is shadowing the current one — most
   commonly an outdated AUR/Homebrew/winget package from before the
   hook subcommand was introduced. The shell template safe-fails to
   "literal space" on any error from the hook, which makes this look
   like an integration bug. `which runex` and `runex version` will
   show whether the resolved binary is current; reinstall or remove
   the stale copy.
3. **clink: `ls<Space>` doesn't expand after upgrading runex.** The
   `runex.lua` on disk doesn't auto-refresh. `runex doctor` will
   report `WARN integration:clink: outdated …`. Re-run
   `runex export clink > %LOCALAPPDATA%\clink\runex.lua` and open a
   new cmd window.
4. **clink: `command:lsd not found` even though `lsd` is installed.**
   The clink-injected cmd's PATH may be missing the User-scope entries
   the registry holds. runex augments command resolution with the
   registry on Windows, but if you've placed binaries outside both
   HKCU and HKLM `Environment\Path`, you'll need to add the directory
   to one of them (or set `RUNEX_CLINK_LUA_PATH` if you've installed
   the lua file in a non-standard location).
