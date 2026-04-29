# Installation

[English](install.md) | [日本語](install.ja.md)

runex ships through multiple channels. Pick the one that fits your platform and tooling.

## Cargo (all platforms with a Rust toolchain)

```bash
cargo install runex
```

## mise — compile from source

```bash
mise use -g cargo:runex
```

## mise — pre-built binary from GitHub releases

No Rust toolchain needed.

```bash
mise use -g github:ShortArrow/runex
```

## Homebrew (macOS / Linux)

runex lives in a third-party tap ([`shortarrow/homebrew-runex`](https://github.com/ShortArrow/homebrew-runex)). Install it in one line with the fully-qualified name:

```bash
brew install shortarrow/runex/runex
```

…or add the tap first and install with the short name:

```bash
brew tap shortarrow/runex
brew install runex
```

## AUR (Arch Linux)

Pre-built binary via [`runex-bin`](https://aur.archlinux.org/packages/runex-bin):

```bash
paru -S runex-bin   # or: yay -S runex-bin
```

## winget (Windows)

```powershell
winget install ShortArrow.runex
```

## Pre-built binary (all platforms)

Each [GitHub release](https://github.com/ShortArrow/runex/releases) ships binaries for:

- Windows (x86_64)
- macOS (x86_64 / aarch64)
- Linux (x86_64 / aarch64)
- Termux / Android (aarch64)

Download the appropriate archive, extract `runex`, and place it somewhere on your `PATH`.

## After install

If `runex` is not found, make sure Cargo's bin directory is on your `PATH`:

- Linux/macOS: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

Generated shell scripts and your `config.toml` are part of your local shell environment. Only load and sync files you trust.

Once runex is on your `PATH`, continue to [setup](setup.md) to wire up shell integration.
