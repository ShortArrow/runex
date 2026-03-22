# runex

English | [日本語](docs/README.ja.md)

> Turn runes into commands.

runex is a cross-shell abbreviation engine that expands short tokens into full commands in real-time.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

## Features

- Cross-shell support (bash / zsh / pwsh / cmd)
- Real-time expansion (customizable trigger)
- Single config file
- Conditional rules (OS / shell / command existence)
- Fast and lightweight (Rust core)

## Concept

runex treats short inputs as **runes**, and expands them into full **casts**.

```
gcm␣ → git commit -m
ls␣  → lsd
```

## Installation

```bash
cargo install runex
```

Generated shell scripts and your `config.toml` are part of your local shell environment. Only load and sync files you trust.

If `runex` is not found after install, make sure Cargo's bin directory is on your `PATH`:

- Unix-like shells: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

## Setup

### PowerShell

`$PROFILE`:

```powershell
Invoke-Expression ((& runex export pwsh) -join "`n")
```

### bash

`~/.bashrc`:

```bash
eval "$(runex export bash)"
```

### zsh

`~/.zshrc`:

```zsh
eval "$(runex export zsh)"
```

### Nushell (Experimental)

Nushell integration is currently experimental and not considered stable yet.

`config.nu`:

```nu
mkdir ~/.config/nu
runex export nu | save -f ~/.config/nu/runex.nu
open ~/.config/nu/config.nu
```

Then add this line to `config.nu`:

```nu
source ~/.config/nu/runex.nu
```

### cmd (Clink)

`%LOCALAPPDATA%\clink\runex.lua`:

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## Config

`~/.config/runex/config.toml`

No keybindings are active until you configure them.

```toml
version = 1

[keybind]
trigger = "space"

[[abbr]]
key = "ls"
expand = "lsd"

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

`expand` is inserted as shell-native text. `runex` does not reinterpret or re-escape it for you.

Supported key values:

- `space`
- `tab`
- `alt-space`

`trigger` sets the default expand key for all shells.
Shell-specific keys like `bash`, `zsh`, `pwsh`, and `nu` override that default.

Example override:

```toml
[keybind]
trigger = "space"
bash = "alt-space"
zsh = "tab"
```

If you want multiple shells or environments to share one physical config file, set `RUNEX_CONFIG` to that path before loading `runex`.

## Avoiding Expansion

If you use `trigger = "space"`, there are a few practical ways to avoid expansion when needed.

- In many terminal setups, `Shift+Space` inserts a plain space without triggering `runex`. This is convenient, but terminal- and shell-dependent.
- In bash, prefixing the token with `\` avoids a match, so `\ls` stays literal. `command ls` also works.
- In PowerShell, `\ls` is just a different token, not a built-in escape. For built-in aliases such as `ls`, prefer the full command name such as `Get-ChildItem`.

## Commands

```bash
runex expand --token ls   # expand a single token
runex list                # list all runes
runex doctor              # check config and environment
runex export <shell>      # generate shell integration script
```

## Example

```
Input:  gcm␣
Output: git commit -m ␣
```

## Why not alias?

| Feature           | alias | runex |
| ----------------- | ----- | ----- |
| Cross-shell       | No    | Yes   |
| Real-time expand  | No    | Yes   |
| Conditional rules | No    | Yes   |

## Philosophy

- One config, all shells
- Minimal typing, maximal power
- Runes over repetition

## Future

- Fuzzy suggestions
- Interactive picker
- Editor integrations

## Name

- **run** (execute)
- **ex** (expand / execute)
- **rune** (compressed command)

## License

MIT
