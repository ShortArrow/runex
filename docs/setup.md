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
