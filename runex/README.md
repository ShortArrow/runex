# runex

> Turn runes into commands.

`runex` is a cross-shell abbreviation engine that expands short tokens into full commands in real time.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

## Features

- Cross-shell support (bash / zsh / pwsh / cmd / nushell)
- Real-time expansion with configurable trigger keys
- One shared config file across shells
- Conditional rules via `when_command_exists`
- Debuggable matching with `which --why` and `expand --dry-run`

## Install

```bash
cargo install runex
```

If `runex` is not found after install, make sure Cargo's bin directory is on your `PATH`:

- Unix-like shells: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

## Quick Start

```bash
runex init
```

This creates a config file and appends shell integration to your rc file with confirmation prompts.

Manual setup is also supported:

- bash: `eval "$(runex export bash)"`
- zsh: `eval "$(runex export zsh)"`
- PowerShell: `Invoke-Expression (& runex export pwsh | Out-String)`
- Clink: `runex export clink > %LOCALAPPDATA%\clink\runex.lua`

## Example Config

```toml
version = 1

[keybind]
trigger = "space"

[[abbr]]
key = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

## Commands

```text
runex expand --token <token>
runex expand --token <token> --dry-run
runex which <token>
runex which <token> --why
runex doctor
runex init
runex export <shell>
runex version
```

Global flags:

```text
--config <path>
--path-prepend <dir>
--json
```

## Documentation

- <https://github.com/ShortArrow/runex#readme>
- <https://github.com/ShortArrow/runex/blob/main/docs/config-reference.md>

Generated shell scripts and your `config.toml` become part of your local shell environment. Only load files you trust.
