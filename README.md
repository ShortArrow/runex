# runex

> Turn runes into commands.

runex is a cross-shell abbreviation engine that expands short tokens into full commands in real-time.

## Features

- Cross-shell support (bash / pwsh / cmd / nu)
- Real-time expansion (space-triggered)
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

## Setup

### PowerShell

```powershell
Invoke-Expression (& runex export pwsh)
```

### bash

```bash
eval "$(runex export bash)"
```

### Nushell

```nu
runex export nu | save ~/.config/nu/runex.nu
```

### cmd (Clink)

```bash
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## Config

`~/.config/runex/config.toml`

```toml
[[abbr]]
key = "ls"
expand = "lsd"

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

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
