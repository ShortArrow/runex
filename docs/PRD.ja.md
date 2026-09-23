# runex - Product Requirements Document

[English](PRD.md) | 日本語

> **翻訳方針:** この文書は英語版 [PRD.md](PRD.md) と同じ事実を扱います。第 2 節のキャッチコピーだけは日本語向けに言い換えています（英語版の "Compress long incantations into runes" に対応）。機能、スコープ、制約などの技術的な要件は英語版と一致させています。

## 1. 概要

runex は、入力中の短いトークン（ルーン）を完全なコマンド（詠唱）に展開する、クロスシェル対応のツールです。

- 入力: 短いトークン（例: `gcm`）
- 出力: 展開されたコマンド（例: `git commit -m`）

中核の概念は **「詠唱の展開（rune → cast）」** です。

---

## 2. コンセプト

> 高速詠唱、短縮詠唱、詠唱破棄、無詠唱

- Rune: 短い入力（トークン）
- Cast: 実行される完全なコマンド
- runex: Rune を Cast に変換するエンジン

---

## 3. 目的

### 3.1 解決したい問題

- 長いコマンドの入力が面倒
- alias や function がシェルごとに分散する
- pwsh、bash、nu で設定を統一できない
- fish の abbr のような体験が他のシェルにない

### 3.2 提供する価値

- クロスシェルで共有する略語定義
- 設定できるトリガーキーによるリアルタイム展開
- 条件付き展開（`when_command_exists`）によるマシン間差異のフォールバック
- 単一の `config.toml` による集中管理
- デバッグ性: `which --why` と `expand --dry-run` が判定の根拠を説明する

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
runex（単一 Rust クレート、内部 module: domain / app / infra）
    ↓
shell adapters
├─ pwsh  （PSReadLine）
├─ bash  （readline / bind）
├─ zsh   （zle / bindkey）
├─ clink （Lua）
└─ nu    （スクリプト）
```

0.1.14 以降の内部レイヤ:

- **`domain/`** — 純粋ロジック（model、expand、hook、sanitize、timings、shell quoting とテンプレート）。I/O も環境変数の読み取りもしない。
- **`app/`** — orchestration、parse、validate、generate（config、doctor、init、shell_export、hook）。
- **`infra/`** — file、registry、env へのアクセス（`HomeDirResolver` を持つ env、integration_cache、integration_check）。
- **`cmd/`** — CLI サブコマンドのハンドラ（`Commands` enum の variant ごとに 1 ファイル）。
- **`util/`** — 末端のヘルパー（シェル検出、command_exists の生成、prompt）。

依存の方向は `cmd → app → domain`、`cmd → util/infra`、`infra → domain` で、一方向かつ循環なし。0.1.14 より前は同じコードを 2 つの crate（`runex-core` と `runex`）に分けていました。`pub` 境界に外部の利用者がいない事実に基づき、Phase C で単一 crate にしました。

各シェルのアダプタは薄いテンプレートです。ラインエディタのバッファを読み、`runex hook` を呼び、返された eval 用テキストを適用するだけで、キー押下ごとの判断はすべて Rust 側で行います。アダプタは静的キャッシュファイルとして導入します（[ADR 0001](decisions/0001-static-integration-cache.md)）。clink のバッファ転送方式は [ADR 0003](decisions/0003-clink-hex-line-transport.md) に記録しています。

---

## 6. 機能要件

### 6.1 コア

- トークンから展開へ（最初に通過したルールを採用）
- 自己ループガード: `key == expand` ならルールを飛ばして評価を続ける
- `when_command_exists`: 列挙したコマンドのどれかがフック実行時に `which` で解決できなければ、ルールを飛ばして評価を続ける
- フォールバック: 未定義のトークンはそのまま通す
- 同じ key の複数ルール: 順番に評価するフォールバックチェーン

### 6.2 CLI

```
runex expand --token <token>              トークンを展開する
runex expand --token <token> --dry-run   展開せずに判定の経過を表示する
runex list                               全略語を一覧表示する
runex list <key>                         key が完全一致するルールだけを表示する
runex which <token>                      採用されるルールを表示する
runex which <token> --why                飛ばした理由を含む経過を表示する
runex doctor                             設定と環境を点検する
runex doctor --no-shell-aliases          alias 衝突の確認を省く（シェルを起動しない）
runex doctor --strict                    不明な設定フィールドも警告する
runex doctor --verbose                   エラーの詳細を省略せずに表示する
runex add <key> <expand>                 略語ルールを設定に追加する
runex add <key> <expand> --when <cmd>    when_command_exists 付きで追加する
runex remove <key>                       略語ルールを設定から削除する
runex init                               設定を作成し、シェル連携を導入する（シェルは自動判定）
runex init <shell>                       特定のシェルを対象にする（bash/zsh/pwsh/clink/nu）
runex init -y                            確認プロンプトを省く
runex export <shell>                     シェル連携スクリプトを出力する
runex export <shell> --bin <name>        スクリプトに埋め込むバイナリ名またはパスを指定する
runex timings <key>                      展開処理のフェーズ別所要時間を表示する
runex timings                            全ルールの所要時間を計測する
runex config where                       解決済みの設定ファイルパスを表示する
runex config type                        設定ファイルの内容を stdout に出力する
runex config show                        OS の関連付けアプリで設定ファイルを開く
runex config reload                      設定ファイルからシェル連携キャッシュを再生成する
runex version                            バージョンとビルドコミットを表示する
```

グローバルフラグ（全サブコマンドで使用可能）:

```
--config <path>      設定ファイルパスを上書きする（RUNEX_CONFIG より優先）
--path-prepend <dir> コマンド存在チェック用に DIR を PATH の先頭に追加する
--json               JSON 形式で出力する（対応: list, doctor, version, expand, which, timings, config where）
```

`runex hook` と `runex paste-clipboard` は非表示のサブコマンドで、シェル連携スクリプトが呼びます。利用者が直接使うものではありません。

### 6.3 設定ファイル

既定の場所: `$XDG_CONFIG_HOME/runex/config.toml`（未設定なら `~/.config/runex/config.toml`、全プラットフォーム共通）。
上書き: 環境変数 `RUNEX_CONFIG` または `--config` フラグ。

```toml
version = 1

[keybind.trigger]
default = "space"       # 全シェル共通の既定トリガー
bash    = "alt-space"   # シェル個別の上書き（省略可）

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

全フィールドの詳細は `docs/config-reference.md` を参照してください。

---

## 7. 非機能要件

- 高速: プロセス内の展開処理（設定読み込み、シェル解決、展開）はキー押下 1 回あたり 1 ms 未満で完了する。計測手段は `runex timings <key> --json`。2026-09-16 に 0.1.20 の release ビルド（`b549c9c`）を Windows 11 で、ルール 1 件の設定に対して 10 回計測した結果は合計 205〜345 µs、中央値 217 µs。プロセス起動とシェル側のキー処理はこの数値に含まない。
- クロスプラットフォーム（Windows / Linux / macOS）
- シェルに依存しないコアロジック（`runex/src/domain/` modules）
- 安全: 自己ループガードで無限展開を防ぐ
- テスト容易性: `command_exists` は依存性注入で差し替えられる

---

## 8. 制約

- shell parser を完全には実装しない。トークン単位の処理のみ
- トークン内のクォートは解釈しない
- runex は展開テキストを再エスケープしない。シェルにはそのままの文字列が渡る

---

## 9. ロードマップ

### 完了（0.1.11 以降）

- キー押下ごとのロジックを `runex hook` サブコマンドに集約し、シェルテンプレートを薄いラッパーにした
- bash / zsh / pwsh / nu 向けの静的連携キャッシュ。rc ファイルから絶対パスで source する（ADR 0001）。手編集後は `runex config reload` で再生成する
- `runex doctor` が環境の状態を表示する。Windows の `effective_search_path` の内訳、`integration:<shell>` の rc ファイルマーカー確認、`integration:<shell>:cache` のヘッダー確認、clink lua の差分検知
- `runex init <shell>` がシェル引数を受け、clink の lua も直接書き出す。既定の設定に動作サンプル（`gst → git status`）を含め、`init` 後にシェル別の "Next steps" を表示する
- crates.io への publish を OIDC Trusted Publishing で CI 化した。長寿命の `CARGO_REGISTRY_TOKEN` はどこにも置かない。テストが通らないコミットからはタグ push でもバイナリを作らない
- digest で固定したイメージによる Linux CI のコンテナ化（ADR 0002）
- 配布経路: 6 ターゲットの GitHub Releases、crates.io、AUR（`runex-bin` と `runex`）、Homebrew tap、winget、`mise github:`
- clink はバッファを 16 進数で送る。`"`、`%`、`!` を含むバッファでも展開できる（ADR 0003）
- bash、zsh、pwsh、nu の PTY 経由キー押下テスト（`runex/tests/*_pty_integration.rs`）と、`runex init` の rc ファイル書き込みに関するプロパティテスト（`runex/tests/cli_integration.rs`）
- `docs/recipes.md`（用途別の `config.toml` スニペット集）

### 直近

- 実運用で見つかる新しい failure mode に応じて、`doctor` と `init` の診断を強化する
- clink のキー押下テスト。現状の clink のテストは、テンプレートが組み立てる cmd.exe のコマンドラインを実際の cmd.exe で実行するもの（`runex/tests/cli_integration.rs`）で、clink 自体は動かしていない

### 後回し

- ファジー候補やフォールバックマッチング
- 対話式ピッカー
- 履歴からの学習
- IDE 連携（Neovim、VS Code）
- `Cargo.toml` への `cargo-binstall` 用メタデータ

---

## 10. 成功指標

- 設定ファイル 1 つで全シェルを統一する
- 体感の入力時間が減る
- シェルごとの alias の散在が解消する

---

## 11. 名前の定義

runex =

- **run**（実行）
- **ex**（expand / execute）
- **rune**（短縮詠唱）

---

## 12. 一言定義

> runex is a rune-to-cast expansion engine.
>
> （runex はルーン（短縮詠唱）をキャスト（実行コマンド）に展開するエンジン）
