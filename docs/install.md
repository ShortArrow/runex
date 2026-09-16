# Installation

[English](install.md) | [日本語](install.ja.md)

Pick the channel that matches your platform and package manager. Every channel installs the same binary; the release process publishes them from the same tag.

| Channel | Platform | Command |
|---|---|---|
| Cargo | any with a Rust toolchain | `cargo install runex` |
| mise (build from source) | any with a Rust toolchain | `mise use -g cargo:runex` |
| mise (GitHub release binary) | Linux, macOS, Windows | `mise use -g github:ShortArrow/runex` |
| Homebrew tap | macOS, Linux | `brew install shortarrow/runex/runex` |
| AUR | Arch Linux | `paru -S runex-bin` |
| winget | Windows | `winget install ShortArrow.runex` |
| GitHub release archive | see the target list below | download, extract, put `runex` on `PATH` |

## Homebrew

runex is published from a third-party tap, [`shortarrow/homebrew-runex`](https://github.com/ShortArrow/homebrew-runex). The fully qualified name installs in one step:

```bash
brew install shortarrow/runex/runex
```

Adding the tap first lets you use the short name afterwards:

```bash
brew tap shortarrow/runex
brew install runex
```

## AUR

[`runex-bin`](https://aur.archlinux.org/packages/runex-bin) installs the release binary. The source package `runex` builds from crates.io instead. The two conflict with each other, so install one of them.

```bash
paru -S runex-bin   # or: yay -S runex-bin
```

## GitHub release archives

Each [GitHub release](https://github.com/ShortArrow/runex/releases) attaches one archive per target. The targets come from `.github/workflows/release.yml`.

| Target | Archive |
|---|---|
| `x86_64-pc-windows-msvc` | zip |
| `x86_64-unknown-linux-gnu` | tar.gz |
| `aarch64-unknown-linux-gnu` | tar.gz |
| `x86_64-apple-darwin` | tar.gz |
| `aarch64-apple-darwin` | tar.gz |
| `aarch64-linux-android` (Termux) | tar.gz |

Extract `runex` from the archive and place it in a directory on your `PATH`.

## After installing

Check that the shell finds the binary:

```bash
runex version
```

If the command is not found after `cargo install`, Cargo's bin directory is missing from `PATH`. Add `~/.cargo/bin` on Linux and macOS, or `%USERPROFILE%\.cargo\bin` on Windows.

The shell integration runex generates and your `config.toml` become part of your shell environment. Load only files you trust, and review them before syncing them across machines.

Next: [Setup](setup.md) wires runex into your shell.
