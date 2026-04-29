# インストール

[English](install.md) | [日本語](install.ja.md)

runex は複数の経路で配布しています。お使いのプラットフォーム・ツールに合わせて選んでください。

## Cargo (Rust ツールチェーンがあればどこでも)

```bash
cargo install runex
```

## mise — ソースからビルド

```bash
mise use -g cargo:runex
```

## mise — GitHub リリースのビルド済みバイナリ

Rust ツールチェーン不要。

```bash
mise use -g github:ShortArrow/runex
```

## Homebrew (macOS / Linux)

runex はサードパーティの tap ([`shortarrow/homebrew-runex`](https://github.com/ShortArrow/homebrew-runex)) で配布しています。完全修飾名で一行インストール:

```bash
brew install shortarrow/runex/runex
```

…もしくは tap を追加してから短縮名でインストール:

```bash
brew tap shortarrow/runex
brew install runex
```

## AUR (Arch Linux)

ビルド済みバイナリの [`runex-bin`](https://aur.archlinux.org/packages/runex-bin) から:

```bash
paru -S runex-bin   # または yay -S runex-bin
```

## winget (Windows)

```powershell
winget install ShortArrow.runex
```

## ビルド済みバイナリ (全プラットフォーム)

各 [GitHub リリース](https://github.com/ShortArrow/runex/releases) に以下のバイナリが添付されています:

- Windows (x86_64)
- macOS (x86_64 / aarch64)
- Linux (x86_64 / aarch64)
- Termux / Android (aarch64)

アーカイブを展開し、`runex` を `PATH` の通ったどこかに配置してください。

## インストール後

`runex` が見つからない場合、Cargo の bin ディレクトリが `PATH` に入っているか確認してください:

- Linux/macOS: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

生成されたシェルスクリプトと `config.toml` はローカルのシェル環境に入ります。信頼できるファイルだけを読み込んでください。

`runex` が `PATH` 上にあることを確認したら、[setup](setup.ja.md) に進んでシェル連携を設定してください。
