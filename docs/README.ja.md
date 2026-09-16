# runex

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/ShortArrow/runex)
[![Downloads](https://img.shields.io/github/downloads/ShortArrow/runex/total.svg?maxAge=2592001)](https://github.com/ShortArrow/runex/releases/)
[![AUR Version](https://img.shields.io/aur/version/runex-bin)](https://aur.archlinux.org/packages/runex-bin)
[![Crates.io Version](https://img.shields.io/crates/v/runex)](https://crates.io/crates/runex)

[English](../README.md) | 日本語

> その一打を、術式へ昇華せよ。

runex は、入力中の短いトークンを完全なコマンドへ置き換えるツールです。bash、zsh、PowerShell、cmd（Clink 経由）、Nushell に対応し、設定ファイルは 1 つだけです。

![runex demo](https://raw.githubusercontent.com/ShortArrow/runex/main/docs/vhs/demo.gif)

## どこから読むか

| 目的 | 読む場所 |
|------|---------|
| インストールして試す | この README のあと [インストール](install.ja.md) と [セットアップ](setup.ja.md) |
| やりたいことに合う設定例を探す | [docs/recipes.ja.md](recipes.ja.md) |
| フィールドの正確な意味を調べる | [docs/config-reference.md](config-reference.md)（英語） |
| 設定したのに動かない | [docs/setup.ja.md のトラブルシューティング](setup.ja.md#トラブルシューティング) |
| 開発に参加する、リリースを切る | [CONTRIBUTING.md](../CONTRIBUTING.md)（英語） |

## 概念

runex は短い入力を **ルーン（rune）** として扱い、トリガーキーを押した時点で完全な **キャスト（cast）** に展開します。

```
gcm␣ → git commit -m
ls␣  → lsd
```

展開はコマンド実行前にラインエディタ上で起きます。展開後のコマンド全体を目で確認し、編集してから実行できます。

## 特徴

- `config.toml` 1 つで bash、zsh、pwsh、clink、nu を設定できます。
- 展開のトリガーキーは選べます。既定は Space です。
- ルールに `when_command_exists` を付けると、指定したコマンドがあるマシンでだけ展開します。`lsd` が無いマシンでは `ls` は `ls` のままです。
- 同じ key のルールを複数書くと、上から順に評価するフォールバックチェーンになります。
- `runex which <token> --why` で、展開された理由やされなかった理由を確認できます。

## 5 分で最初の展開

```bash
cargo install runex                # 1. インストール
runex init                         # 2. 設定を作成し、シェルに連携を追加
                                   #    (rc ファイルへ書く前に確認を求める)
exec $SHELL                        # 3. 新しいシェルで連携を読み込む
gst<Space>                         # 4. git status に展開される
```

`runex init` が書く設定には、`gst` を `git status` にするサンプルが 1 件入っています。これを自分のルールに置き換えてください。コピーして使える設定例は [docs/recipes.ja.md](recipes.ja.md) にあります。

## インストール

```bash
cargo install runex                       # Rust ツールチェーン
brew install shortarrow/runex/runex       # macOS / Linux
paru -S runex-bin                         # Arch Linux (AUR)
winget install ShortArrow.runex           # Windows
```

mise、リリースアーカイブ、対応ターゲットの一覧は [docs/install.ja.md](install.ja.md) にあります。

## セットアップ

`runex init` は設定ファイルを作成し、キャッシュディレクトリにシェル連携ファイルを書き、rc ファイルの末尾に `source` 行を 1 行追記します。書き込みのたびに確認を求めます。

```
$ runex init
Create config at ~/.config/runex/config.toml? [y/N] y
Created: ~/.config/runex/config.toml
Install shell integration (cache at ~/.cache/runex/integration.bash, source line in ~/.bashrc)? [y/N] y
Wrote integration cache to ~/.cache/runex/integration.bash
Appended source line to ~/.bashrc
```

rc ファイルの既存行は変更しません。`-y` を付けると確認を省略します。`runex init pwsh` のようにシェル名を渡すと、自動検出を省略します。保証の一覧とシェル別の詳細は [docs/setup.ja.md](setup.ja.md) にあります。

## 設定

設定ファイルの場所は `$XDG_CONFIG_HOME/runex/config.toml` です。この環境変数が無いときは、どのプラットフォームでも `~/.config/runex/config.toml` を使います。環境変数 `RUNEX_CONFIG` またはフラグ `--config <path>` で上書きできます。

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
expand = "git commit -am '{}'"   # {} = 展開後にカーソルが置かれる位置
```

`[keybind]` テーブルが無い設定では、どのキーも束縛されません。`runex init` が書く設定は Space を束縛します。

`runex add` と `runex remove` は設定ファイルを書き換え、シェル連携も更新します。設定ファイルを手で編集したあとは `runex config reload` を実行してください。編集内容がシェル連携に反映されます。

フィールドの一覧、評価順、上限値は [docs/config-reference.md](config-reference.md) に、場面別の設定例は [docs/recipes.ja.md](recipes.ja.md) にあります。

## コマンド

日常的に使うサブコマンドは次の 6 つです。

```
runex init [shell]               設定を作成し、シェル連携を導入する (書き込み前に確認)
runex doctor                     設定、コマンド解決、シェル連携を点検する
runex add <key> <expand>         ルールを設定ファイルに追加する
runex remove <key>               ルールを設定ファイルから削除する
runex which <token> --why        トークンが展開されるか、その理由を表示する
runex config reload              config.toml を手で編集したあと、シェル連携を再生成する
```

`runex --help` で全サブコマンドを、`runex <subcommand> --help` で各フラグを確認できます。グローバルフラグ `--config`、`--path-prepend`、`--json` は全サブコマンドで受け付けます。`--json` が構造化出力を返すのは `list`、`doctor`、`version`、`expand`、`which`、`timings`、`config where` です。

## 展開を避けたいとき

`trigger = "space"` の設定では、コマンド位置のトークンは Space のたびに展開されます。回避方法は次のとおりです。

- bash と zsh では、先頭に `\` を付ける（`\ls`）か、`command ls` と書きます。
- PowerShell では `\ls` は別のトークンなので展開されません。組み込み alias をそのまま使いたいときは、`Get-ChildItem` のように完全なコマンド名を書きます。

展開せずにスペースだけ入れるキーも設定できます。

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
default = "shift-space"   # pwsh/nu: Shift+Space は展開せずにスペースを入れる
# default = "alt-space"   # bash/zsh を含む全シェルで使える
```

| 値 | bash | zsh | pwsh | nu |
|---|---|---|---|---|
| `"alt-space"` | yes | yes | yes | yes |
| `"shift-space"` | no | no | yes | yes |

## alias との違い

| 機能             | alias | runex |
| ---------------- | ----- | ----- |
| クロスシェル     | No    | Yes   |
| リアルタイム展開 | No    | Yes   |
| 条件付きルール   | No    | Yes   |

alias は実行時に置換されるので、履歴には短い形が残り、定義はシェルごとに分かれます。runex は実行前に行を書き換えるので、履歴には完全なコマンドが残り、設定は 1 つで済みます。

## ロードマップ

ロードマップは [docs/PRD.ja.md](PRD.ja.md#9-ロードマップ) で管理しています。予定に入っていない案として、ファジー候補、対話式ピッカー、エディタ連携、`cargo-binstall` 用メタデータがあります。

## 名前の由来

- **run**（実行）
- **ex**（expand / execute）
- **rune**（短縮した詠唱）
- run + ex = expand / execute / express / extract / explode
- rune x（7z の "x" が展開を意味するように）
- rune +x（chmod の "+x" が実行を許すように）

## 謝辞

runex は [fish shell の略語機能](https://fishshell.com/docs/current/cmds/abbr.html) と [zsh-abbr](https://github.com/olets/zsh-abbr) に着想を得ています。リアルタイムなトークン展開はそこで生まれました。runex はそれを 1 つの設定ファイルで全シェルに広げます。

## ライセンス

[MIT](../LICENSE) または [Apache-2.0](../LICENSE) のいずれかを選べます（デュアルライセンス）。明示的に別途合意がない限り、本プロジェクトへの貢献も同じデュアルライセンスで提供されるものとします。

サードパーティのライセンスは [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) に記載しています。
