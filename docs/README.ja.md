# runex

[English](../README.md) | 日本語

> その一打を、術式へ昇華せよ。

runex は、短いトークンをリアルタイムに完全なコマンドへ展開する、クロスシェル対応の略語エンジンです。

## 特性

- クロスシェル対応（bash / zsh / pwsh / cmd / nushell）
- リアルタイム展開（トリガキー変更可能）
- 設定ファイル1つで全シェルを管理
- 条件付きルール（`when_command_exists`） — 指定したコマンドが現在のシェルで解決できる場合のみ展開
- 高速・軽量（Rust コア）

## 概念

runex は短い入力を **ルーン（rune）** として受け取り、完全な **キャスト（cast）** に展開します。

```
gcm␣ → git commit -m
ls␣  → lsd
```

## クイックスタート

```bash
cargo install runex
runex init
```

## インストール

```bash
cargo install runex                       # Rust ツールチェーン
brew install shortarrow/runex/runex       # macOS / Linux
paru -S runex-bin                         # Arch Linux (AUR)
winget install ShortArrow.runex           # Windows
```

mise・ビルド済みバイナリ・プラットフォーム別の注意事項など、その他の経路は [docs/install.ja.md](install.ja.md) を参照してください。

## セットアップ

`runex init` が設定ファイルを作成し、rc ファイルにシェル連携行を追記します。各ステップで確認プロンプトが出ます:

```
$ runex init
Create config at ~/.config/runex/config.toml? [y/N] y
Created: ~/.config/runex/config.toml
Append shell integration to ~/.bashrc? [y/N] y
Appended integration to ~/.bashrc
```

`-y` を付けると確認をすべてスキップします。シェル別の手動設定 (bash / zsh / pwsh / nu / clink) は [docs/setup.ja.md](setup.ja.md) を参照してください。

## 設定

デフォルトパス: `$XDG_CONFIG_HOME/runex/config.toml`（未設定なら `~/.config/runex/config.toml`、全プラットフォーム共通）。

環境変数 `RUNEX_CONFIG` または `--config` フラグで上書きできます。

keybind を設定するまでは、どのキーにも何も割り当てられません。

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
expand = "git commit -am '{}'"   # {} = 展開後ここにカーソルが残る
```

全フィールド・評価順・フォールバックチェーンの詳細は [docs/config-reference.md](config-reference.md) を参照してください。

## コマンド

```
runex expand --token <token>              トークンを展開
runex expand --token <token> --dry-run   展開せずマッチトレースを表示
runex list                               全略語を一覧表示
runex which <token>                      マッチするルールを表示
runex which <token> --why                スキップ理由を含む全トレースを表示
runex doctor                             設定と環境をチェック
runex doctor --no-shell-aliases          alias 競合チェックをスキップ
runex doctor --strict                    不明な設定フィールドも警告
runex add <key> <expand>                 略語ルールを設定に追加
runex add <key> <expand> --when <cmd>    when_command_exists 付きで追加
runex remove <key>                       略語ルールを設定から削除
runex init                               設定ファイルを作成し、シェル連携を追記
runex init -y                            確認プロンプトをスキップ
runex export <shell>                     シェル連携スクリプトを生成
runex export <shell> --bin <name>        スクリプト内のバイナリ名を変更
runex timings <key>                      展開フローのフェーズ別所要時間を表示
runex timings                            全ルールの所要時間を計測
runex version                            バージョンとビルドコミットを表示
```

グローバルフラグ（全サブコマンドで使用可能）：

```
--config <path>      設定ファイルパスを上書き
--path-prepend <dir> コマンド存在チェック用に DIR を PATH の先頭に追加
--json               JSON 形式で出力（対応コマンド: list, doctor, version, expand, which, timings）
```

`runex doctor` は設定検証と並んで環境レベルのチェックも表示します:
`effective_search_path` (Windows 専用の PATH 補強概要、詳細は
[`docs/config-reference.md`](config-reference.md#runex-doctor--environment--integration-health))
と `integration:<shell>` (rcfile マーカーの有無、clink lua のドリフト検知)。
出力例は [`docs/setup.ja.md`](setup.ja.md#runex-doctor-で動作確認) を参照。

## 展開を回避したいとき

`trigger = "space"` を使う場合：

- bash では先頭に `\` を付ける（例: `\ls`）か、`command ls` を使います。
- PowerShell では `\ls` は別トークンになるだけです。標準 alias をそのまま使いたいなら `Get-ChildItem` のように完全なコマンド名を書いてください。

`self_insert` でキーを「展開せずにスペース挿入」にバインドすることもできます：

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
default = "shift-space"   # pwsh/nu: Shift+Space は展開せずにスペースを挿入
# default = "alt-space"   # bash/zsh を含む全シェル対応
```

| 値 | bash | zsh | pwsh | nu |
|---|---|---|---|---|
| `"alt-space"` | yes | yes | yes | yes |
| `"shift-space"` | no | no | yes | yes |

## alias との差異

| 機能             | alias | runex |
| ---------------- | ----- | ----- |
| クロスシェル     | No    | Yes   |
| リアルタイム展開 | No    | Yes   |
| 条件付きルール   | No    | Yes   |

## 哲学

- 1つの設定、全てのシェル
- 最小の入力、最大の威力
- 履歴検索より即応の展開

## ロードマップ

直近:

- `doctor` / `init` のエッジケース対応と診断改善

後回し:

- ファジーマッチングフォールバック
- インタラクティブピッカー
- エディタ連携
- 配布チャネル拡充（GitHub Releases、`cargo-binstall`、`winget`、`mise github:`）

## 名の由来

- run + ex = expand / execute / express / extract / explode
- rune x（7z の "x" で展開するように）
- rune +x（chmod の "+x" で実行可能にするように）

## 謝辞

runex は [fish shell の略語システム](https://fishshell.com/docs/current/cmds/abbr.html) と [zsh-abbr](https://github.com/olets/zsh-abbr) に着想を得ています。リアルタイムなトークン展開というアイデアはそこから生まれました — runex はそれを単一の設定ファイルであらゆるシェルに持ち込みます。

## ライセンス

[MIT](../LICENSE) または [Apache-2.0](../LICENSE) のいずれかを選択可能（デュアルライセンス）。明示的に別途合意がない限り、本プロジェクトへのあらゆる貢献も同様のデュアルライセンスで提供されるものとします。

サードパーティライセンスは [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) に記載しています。
