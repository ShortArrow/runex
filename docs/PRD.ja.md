# runex - Product Requirements Document

[English](PRD.md) | 日本語

## 1. 概要

runex は、短い入力（ルーン）を長いコマンド（詠唱）へリアルタイムに展開するクロスシェル対応ツールです。

- 入力: 短いトークン（例: `gcm`）
- 出力: 展開されたコマンド（例: `git commit -m`）

中核概念: **「詠唱の展開（rune → cast）」**

---

## 2. コンセプト

> 高速詠唱、短縮詠唱、詠唱破棄、無詠唱

- Rune: 短い入力（トークン）
- Cast: 実行される完全コマンド
- runex: Rune → Cast の変換エンジン

---

## 3. 目的

### 3.1 解決したい問題

- 長いコマンド入力が面倒
- alias / function がシェルごとに分散する
- pwsh / bash / nu で設定が統一できない
- fish の abbr のようなUXが他シェルにない

### 3.2 提供価値

- クロスシェル共通の略語定義
- 設定可能なトリガキーによるリアルタイム展開
- 条件付き展開（`when_command_exists`）— マシン間の差異にフォールバックで対応
- 単一 `config.toml` による集中管理
- デバッグ性: `which --why` と `expand --dry-run` で判定根拠を説明

---

## 4. スコープ

### 対応シェル

- bash
- zsh
- PowerShell (pwsh)
- cmd（Clink 経由）
- Nushell (nu)

---

## 5. アーキテクチャ

```text
config.toml
    ↓
runex core（Rust ライブラリクレート: runex-core）
    ↓
shell adapters
├─ pwsh  （PSReadLine）
├─ bash  （readline / bind）
├─ zsh   （zle / bindkey）
├─ clink （Lua）
└─ nu    （スクリプト）
```

---

## 6. 機能要件

### 6.1 コア

- トークン → 展開（最初に通過したルールを採用）
- 自己ループガード: `key == expand` → ルールをスキップして評価継続
- `when_command_exists`: リスト中のコマンドがひとつでもなければスキップして継続
- fallback: 未定義トークンはそのまま通過
- 同一 key の複数ルール: フォールバックチェーンとして順番に評価

### 6.2 CLI

```
runex expand --token <token>              トークンを展開
runex expand --token <token> --dry-run   展開せずマッチトレースを表示
runex list                               全略語を一覧表示
runex which <token>                      マッチするルールを表示
runex which <token> --why                スキップ理由を含む全トレースを表示
runex doctor                             設定と環境をチェック
runex doctor --no-shell-aliases          alias 競合チェックをスキップ（シェル起動なし）
runex doctor --strict                    不明な設定フィールドも警告
runex add <key> <expand>                 略語ルールを設定に追加
runex add <key> <expand> --when <cmd>    when_command_exists 付きで追加
runex remove <key>                       略語ルールを設定から削除
runex init                               設定を作成し、rc ファイルへシェル連携を追記
runex init -y                            確認プロンプトをスキップ
runex export <shell>                     シェル連携スクリプトを生成
runex export <shell> --bin <name>        スクリプト内のバイナリ名を変更
runex timings <key>                      展開フローのフェーズ別所要時間を表示
runex timings                            全ルールの所要時間を計測
runex precache --shell <shell>           コマンド存在チェックを事前キャッシュ
runex version                            バージョンとビルドコミットを表示
```

グローバルフラグ（全サブコマンドで使用可能）：

```
--config <path>      設定ファイルパスを上書き（RUNEX_CONFIG より優先）
--path-prepend <dir> コマンド存在チェック用に DIR を PATH の先頭に追加
--json               JSON 形式で出力（対応: list, doctor, version, expand, which, timings）
```

### 6.3 設定ファイル

デフォルト: `$XDG_CONFIG_HOME/runex/config.toml`（未設定なら `~/.config/runex/config.toml`、全プラットフォーム共通）。
上書き: 環境変数 `RUNEX_CONFIG` または `--config` フラグ。

```toml
version = 1

[keybind]
trigger = "space"        # 全シェル共通のデフォルトトリガ
bash    = "alt-space"    # シェル個別の上書き（省略可）

[[abbr]]
key    = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key    = "ls"
expand = "ls --color=auto"

[[abbr]]
key    = "gcm"
expand = "git commit -m"
```

全フィールドの詳細は `docs/config-reference.md` を参照。

---

## 7. 非機能要件

- 高速: 展開パスは <1ms で完了
- クロスプラットフォーム（Windows / Linux / macOS）
- シェル非依存コアロジック（runex-core クレート）
- 安全: 自己ループガードで無限展開を防止
- テスト容易性: `command_exists` は依存性注入で差し替え可能

---

## 8. 制約

- shell parser を完全実装しない — token 単位処理のみ
- トークン内のクォートは解釈しない
- runex は展開テキストを再エスケープしない。シェルにはそのままの文字列が渡る
- `runex init` は Clink のシェル連携を自動追記できない

---

## 9. ロードマップ

### 直近

- `doctor` / `init` のエッジケース対応と診断改善

### 後回し

- fuzzy 候補 / フォールバックマッチング
- インタラクティブピッカー
- 履歴学習
- IDE 連携（Neovim、VS Code）
- 配布チャネル拡充（GitHub Releases、`cargo-binstall`、`winget`、`mise github:`）

---

## 10. 成功指標

- 設定ファイル1つで全シェル統一
- 体感入力時間削減
- per-shell alias の散在解消

---

## 11. 名前の定義

runex =

- **run**（実行）
- **ex**（expand / execute）
- **rune**（短縮詠唱）

---

## 12. 一言定義

> runex is a rune-to-cast expansion engine.
