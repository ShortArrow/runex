# runex

> Turn runes into commands.

`runex` is a cross-shell abbreviation engine that expands short tokens into full commands in real time.

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

This README is intentionally minimal for crates.io.
Keep detailed user documentation in the repository root README.

## Install

```bash
cargo install runex
```

Or with `mise`:

```bash
mise use -g cargo:runex
```

If `runex` is not found after install, make sure Cargo's bin directory is on your `PATH`:

- Unix-like shells: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

## Documentation

- <https://github.com/ShortArrow/runex#readme>
- <https://github.com/ShortArrow/runex/blob/main/docs/config-reference.md>

Generated shell scripts and your `config.toml` become part of your local shell environment. Only load files you trust.
