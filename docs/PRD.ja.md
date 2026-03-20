# runex - Product Requirements Document

[English](PRD.md) | 日本語

## 1. 概要

runex は、短い入力（ルーン）を長いコマンド（詠唱）へリアルタイムに展開する
クロスシェル対応ツールである。

- 入力: 短いトークン（例: `gcm`）
- 出力: 展開されたコマンド（例: `git commit -m`）

本ツールは「短縮詠唱」ではなく、
**「詠唱の展開（rune → cast）」** を中核概念とする。

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
- alias/function がシェルごとに分散する
- pwsh / bash / nu で設定が統一できない
- fish の abbr のようなUXが他シェルにない

### 3.2 提供価値

- クロスシェル共通の略語定義
- 入力中リアルタイム展開（spaceトリガ）
- 条件付き展開（コマンド存在・OS・シェル）
- 単一 config.toml による集中管理

---

## 4. スコープ

### 対応シェル

- bash
- PowerShell (pwsh)
- cmd (via Clink)
- Nushell (nu)

---

## 5. アーキテクチャ

```text
config.toml
    ↓
runex core (Rust)
    ↓
shell adapters
├─ pwsh (PSReadLine)
├─ bash (readline)
├─ clink (lua)
└─ nu (script)
```

---

## 6. 機能要件

### 6.1 コア機能

- トークン → 展開
- 条件付き展開
- fallback（未定義時はそのまま）
- shell-aware動作

### 6.2 CLI

```bash
runex expand --token ls
runex list
runex doctor
runex export pwsh
runex export bash
runex export nu
runex export clink
```

### 6.3 設定ファイル

`~/.config/runex/config.toml`

```toml
version = 1

[[abbr]]
key = "ls"
expand = "lsd"
when_command_exists = ["lsd"]

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

---

## 7. 非機能要件

- 高速（<1ms レベル）
- クロスプラットフォーム（Windows/Linux/macOS）
- シェル非依存ロジック
- 安全（無限ループ防止）

---

## 8. 制約

- shell parser を完全実装しない
- token単位処理のみ
- quote内は初期非対応

---

## 9. 将来拡張

- fuzzy候補
- UI picker
- history学習
- IDE連携（Neovim等）

---

## 10. 成功指標

- 設定ファイル1つで全シェル統一
- 体感入力時間削減
- alias削減率

---

## 11. 名前の定義

runex =

- **run**（実行）
- **ex**（expand / execute）
- **rune**（短縮詠唱）

---

## 12. 一言定義

> runex is a rune-to-cast expansion engine.
