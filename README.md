# runex

English | [日本語](docs/README.ja.md)

> Turn runes into commands.

runex is a cross-shell abbreviation engine that expands short tokens into full commands in real-time.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

## Features

- Cross-shell support (bash / zsh / pwsh / cmd / nushell)
- Real-time expansion (customizable trigger key)
- Single config file shared across shells
- Conditional rules (`when_command_exists`) — only expand when the listed commands resolve in the current shell
- Fast and lightweight (Rust core)

## Concept

runex treats short inputs as **runes**, and expands them into full **casts**.

```
gcm␣ → git commit -m
ls␣  → lsd
```

## Quick start

```bash
cargo install runex
runex init
```

## Install

```bash
cargo install runex                       # Rust toolchain
brew install shortarrow/runex/runex       # macOS / Linux
paru -S runex-bin                         # Arch Linux (AUR)
winget install ShortArrow.runex           # Windows
```

Other options (mise, pre-built binaries, platform notes): see [docs/install.md](docs/install.md).

## Setup

`runex init` creates the config and appends the shell integration line to your rc file, with a confirmation prompt at each step:

```
$ runex init
Create config at ~/.config/runex/config.toml? [y/N] y
Created: ~/.config/runex/config.toml
Append shell integration to ~/.bashrc? [y/N] y
Appended integration to ~/.bashrc
```

Pass `-y` to skip all prompts. Per-shell manual setup (bash / zsh / pwsh / nu / clink) is documented in [docs/setup.md](docs/setup.md).

## Config

Default path: `$XDG_CONFIG_HOME/runex/config.toml`, falling back to `~/.config/runex/config.toml` on all platforms.

Override with the `RUNEX_CONFIG` environment variable or the `--config` flag.

No keybindings are active until you configure them.

```toml
version = 1

[keybind.trigger]
default = "space"

[[abbr]]
key    = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key    = "gcm"
expand = "git commit -m"

[[abbr]]
key    = "gcam"
expand = "git commit -am '{}'"   # {} = cursor stays here after expansion
```

See [docs/config-reference.md](docs/config-reference.md) for the full reference, including evaluation order, fallback chains, and all accepted fields. For copy-pasteable scenarios (Git shortcuts, per-shell commands, fallback chains, …) see [docs/recipes.md](docs/recipes.md).

## Commands

```
runex expand --token <token>              expand a token
runex expand --token <token> --dry-run   show match trace without expanding
runex list                               list all abbreviations
runex which <token>                      show which rule matches
runex which <token> --why                show full match trace with skip reasons
runex doctor                             check config and environment
runex doctor --no-shell-aliases          skip alias conflict checks
runex doctor --strict                    also warn about unknown config fields
runex add <key> <expand>                 add an abbreviation rule to config
runex add <key> <expand> --when <cmd>    add with when_command_exists condition
runex remove <key>                       remove an abbreviation rule from config
runex init                               create config and append shell integration
runex init -y                            same, skip confirmation prompts
runex export <shell>                     generate shell integration script
runex export <shell> --bin <name>        use a custom binary name in the script
runex timings <key>                      show per-phase timing breakdown of expand
runex timings                            time all abbreviation rules
runex version                            show version and build commit
```

Global flags (available on every subcommand):

```
--config <path>      override config file path
--path-prepend <dir> prepend a directory to PATH for command existence checks
--json               JSON output (supported by: list, doctor, version, expand, which, timings)
```

`runex doctor` reports several environment-level checks alongside the
config validation: `effective_search_path` (Windows-only PATH augmentation
summary, see [`docs/config-reference.md`](docs/config-reference.md#runex-doctor--environment--integration-health))
and `integration:<shell>` (rcfile-marker presence and clink lua drift
detection). See [`docs/setup.md`](docs/setup.md) for an annotated example.

## Avoiding Expansion

If you use `trigger = "space"`, there are a few practical ways to avoid expansion:

- In bash, prefix the token with `\` — e.g. `\ls` — or use `command ls`.
- In PowerShell, `\ls` is just a different token. For built-in aliases, prefer the full command name (e.g. `Get-ChildItem`).

You can also bind a key to plain-space insertion using `self_insert`:

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
default = "shift-space"   # pwsh/nu: Shift+Space inserts a space without expanding
# default = "alt-space"   # all shells including bash/zsh
```

| Value | bash | zsh | pwsh | nu |
|---|---|---|---|---|
| `"alt-space"` | yes | yes | yes | yes |
| `"shift-space"` | no | no | yes | yes |

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

## Roadmap

Near-term:

- Harden `doctor` and `init` around edge cases and clearer diagnostics

Later:

- Fuzzy suggestions
- Interactive picker
- Editor integrations
- Broader distribution channels (GitHub Releases, `cargo-binstall`, `winget`, `mise github:`)

## Name

- **run** (execute)
- **ex** (expand / execute)
- **rune** (compressed command)
- **run** + **ex** = expand / execute / express / extract / explode
- **rune x** (like 7z's "x" for extract)
- **rune +x** (like chmod's "+x" execute)

## Acknowledgements

runex is inspired by [fish shell's abbreviation system](https://fishshell.com/docs/current/cmds/abbr.html) and [zsh-abbr](https://github.com/olets/zsh-abbr). The idea of real-time token expansion originated there — runex brings it to every shell with a single config file.

## License

Dual-licensed under either of [MIT](LICENSE) or [Apache-2.0](LICENSE) at your option. Unless explicitly stated otherwise, any contribution intentionally submitted for inclusion in this work by you shall be dual-licensed as above, without any additional terms or conditions.

Third-party dependency licenses are documented in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
