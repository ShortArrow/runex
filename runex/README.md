# runex

> Turn runes into commands.

`runex` expands a short token into a full command as you type, in bash, zsh, PowerShell, cmd (via Clink) and Nushell, from one config file.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

This README is intentionally minimal for crates.io. The user documentation lives in the repository.

## Install

```bash
cargo install runex
```

Or with `mise`:

```bash
mise use -g cargo:runex
```

If `runex` is not found after install, add Cargo's bin directory to your `PATH`: `~/.cargo/bin` on Linux and macOS, `%USERPROFILE%\.cargo\bin` on Windows.

## Set up

```bash
runex init        # creates the config and installs the shell integration, asking before each write
exec $SHELL
gst<Space>        # expands to: git status
```

## Documentation

- README: <https://github.com/ShortArrow/runex#readme>
- Setup and troubleshooting: <https://github.com/ShortArrow/runex/blob/main/docs/setup.md>
- Config reference: <https://github.com/ShortArrow/runex/blob/main/docs/config-reference.md>

The generated shell integration and your `config.toml` become part of your shell environment. Load only files you trust.
