# インストール

[English](install.md) | 日本語

プラットフォームとパッケージマネージャに合う経路を 1 つ選んでください。どの経路でも同じバイナリが入ります。リリース作業では同じタグから各経路へ配布します。

| 経路 | プラットフォーム | コマンド |
|---|---|---|
| Cargo | Rust ツールチェーンがある環境 | `cargo install runex` |
| mise（ソースからビルド） | Rust ツールチェーンがある環境 | `mise use -g cargo:runex` |
| mise（GitHub リリースのバイナリ） | Linux、macOS、Windows | `mise use -g github:ShortArrow/runex` |
| Homebrew tap | macOS、Linux | `brew install shortarrow/runex/runex` |
| AUR | Arch Linux | `paru -S runex-bin` |
| winget | Windows | `winget install ShortArrow.runex` |
| GitHub リリースのアーカイブ | 下のターゲット一覧を参照 | 展開して `runex` を `PATH` 上に置く |
| APT リポジトリ | Debian、Ubuntu（amd64、arm64） | pkg.shortarrow.jp を追加して `sudo apt install runex` |
| Debian パッケージ | Debian、Ubuntu（amd64、arm64） | ダウンロードして `sudo apt install ./runex_<version>-1_<arch>.deb` |

## Homebrew

runex はサードパーティ tap の [`shortarrow/homebrew-runex`](https://github.com/ShortArrow/homebrew-runex) から配布しています。完全修飾名なら 1 コマンドで入ります。

```bash
brew install shortarrow/runex/runex
```

先に tap を追加しておくと、以後は短い名前で扱えます。

```bash
brew tap shortarrow/runex
brew install runex
```

## AUR

[`runex-bin`](https://aur.archlinux.org/packages/runex-bin) はリリースバイナリをそのまま入れるパッケージです。ソースパッケージの `runex` は crates.io からビルドします。両者は競合するので、どちらか一方だけを入れてください。

```bash
paru -S runex-bin   # または yay -S runex-bin
```

## GitHub リリースのアーカイブ

各 [GitHub リリース](https://github.com/ShortArrow/runex/releases) には、ターゲットごとに 1 つずつアーカイブが添付されています。ターゲットの一覧は `.github/workflows/release.yml` の定義と一致します。

| ターゲット | アーカイブ |
|---|---|
| `x86_64-pc-windows-msvc` | zip |
| `x86_64-unknown-linux-gnu` | tar.gz |
| `aarch64-unknown-linux-gnu` | tar.gz |
| `x86_64-unknown-linux-musl` | tar.gz |
| `aarch64-unknown-linux-musl` | tar.gz |
| `x86_64-apple-darwin` | tar.gz |
| `aarch64-apple-darwin` | tar.gz |
| `aarch64-linux-android`（Termux） | tar.gz |

アーカイブから `runex` を取り出し、`PATH` の通ったディレクトリに置いてください。

Linux では `linux-musl` のアーカイブを選んでください。静的リンクなので、Ubuntu 22.04 や Debian 12 を含むどのディストリビューションでも起動します。`linux-gnu` のアーカイブはビルドしたランナーの glibc（現在は 2.39）にリンクしているので、それ以上の glibc が必要です。

## APT リポジトリ

Debian と Ubuntu（amd64、arm64）では、APT リポジトリ `https://pkg.shortarrow.jp/apt` から入れられます。ShortArrow のプロジェクトのパッケージを配布しているリポジトリです。最初に 1 回だけ、鍵とソースを追加します。

```bash
sudo curl -fsSL https://pkg.shortarrow.jp/apt/shortarrow-archive-keyring.gpg -o /usr/share/keyrings/shortarrow-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/shortarrow-archive-keyring.gpg] https://pkg.shortarrow.jp/apt stable main" | sudo tee /etc/apt/sources.list.d/shortarrow.list
sudo apt update
sudo apt install shortarrow-archive-keyring runex
```

`shortarrow-archive-keyring` は鍵を収めたパッケージです。上のように名前を指定して入れてください。依存として入っただけだと、`apt autoremove` で削除されることがあります。以後は `sudo apt upgrade` でほかのパッケージと一緒に runex も更新されます。

新しいリリースがリポジトリに載るのは、リポジトリの管理者が公開を承認した後です。そのため GitHub のリリースより遅れることがあります。

## Debian パッケージ

リポジトリを追加せずに入れる場合は、リリースからパッケージをダウンロードします。v0.1.20 より後のリリースには、`runex_<version>-1_amd64.deb` と `runex_<version>-1_arm64.deb` も添付します。中身は musl 版のバイナリ（`/usr/bin/runex`）だけで、ほかのパッケージには依存しません。アーキテクチャに合うものをダウンロードし、apt で入れてください。apt が記録するので、`apt remove runex` で削除できます。

```bash
sudo apt install ./runex_<version>-1_amd64.deb
```

リリースのたびに、公開する前に Debian 12、Ubuntu 22.04、Ubuntu 24.04 でこの方法のインストールを確かめています。

## インストール後の確認

シェルからバイナリが見えるか確認します。

```bash
runex version
```

`cargo install` の後に見つからない場合は、Cargo の bin ディレクトリが `PATH` に入っていません。Linux と macOS では `~/.cargo/bin`、Windows では `%USERPROFILE%\.cargo\bin` を追加してください。

runex が生成するシェル連携スクリプトと `config.toml` は、シェル環境の一部として読み込まれます。信頼できるファイルだけを読み込み、他のマシンへ同期する前に内容を確認してください。

次は [セットアップ](setup.ja.md) で、シェルへの連携を設定します。
