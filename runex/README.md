# runex

> Turn runes into commands.

`runex` is a cross-shell abbreviation engine that expands short tokens into full commands in real time.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

## Install

```bash
cargo install runex
```

If `runex` is not found after install, make sure Cargo's bin directory is on your `PATH`:

- Unix-like shells: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

## Shells

- `bash`
- `zsh`
- `pwsh`
- `cmd` via Clink
- `nu` is currently experimental

## Setup

### bash

```bash
echo 'eval "$(runex export bash)"' >> ~/.bashrc
```

### zsh

```zsh
echo 'eval "$(runex export zsh)"' >> ~/.zshrc
```

### PowerShell

```powershell
if (!(Test-Path $PROFILE)) { New-Item -Type File -Path $PROFILE -Force }
Add-Content $PROFILE 'Invoke-Expression ((& runex export pwsh) -join "`n")'
```

### cmd (Clink)

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## Config

```toml
version = 1

[keybind]
trigger = "space"

[[abbr]]
key = "gcm"
expand = "git commit -m"

[[abbr]]
key = "ll"
expand = "lsd -l"
```

## Commands

```text
runex expand --token gcm
runex list
runex doctor
runex export bash
```

Generated shell scripts and your `config.toml` become part of your local shell environment. Only load files you trust.

Full documentation: <https://github.com/ShortArrow/runex>
