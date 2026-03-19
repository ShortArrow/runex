# runex

> ルーンをコマンドに変える。

runex は、短いトークンをリアルタイムに完全なコマンドへ展開する、クロスシェル対応の略語エンジンです。

## 特徴

- クロスシェル対応（bash / pwsh / cmd / nu）
- リアルタイム展開（スペーストリガ）
- 設定ファイル1つで管理
- 条件付きルール（OS / シェル / コマンド存在）
- 高速・軽量（Rust コア）

## コンセプト

runex は短い入力を **ルーン（rune）** として扱い、完全な **キャスト（cast）** に展開します。

```
gcm␣ → git commit -m
ls␣  → lsd
```

## インストール

```bash
cargo install runex
```

## セットアップ

### PowerShell

```powershell
Invoke-Expression (& runex export pwsh)
```

### bash

```bash
eval "$(runex export bash)"
```

### Nushell

```nu
runex export nu | save ~/.config/nu/runex.nu
```

### cmd (Clink)

```bash
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## 設定

`~/.config/runex/config.toml`

```toml
[[abbr]]
key = "ls"
expand = "lsd"

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

## コマンド

```bash
runex expand --token ls   # 単一トークンを展開
runex list                # 全ルーンを一覧表示
runex doctor              # 設定と環境をチェック
runex export <shell>      # シェル連携スクリプトを生成
```

## 使用例

```
入力:  gcm␣
出力:  git commit -m ␣
```

## alias との違い

| 機能             | alias | runex |
| ---------------- | ----- | ----- |
| クロスシェル     | No    | Yes   |
| リアルタイム展開 | No    | Yes   |
| 条件付きルール   | No    | Yes   |

## 思想

- 1つの設定、全てのシェル
- 最小の入力、最大のパワー
- 繰り返しよりルーン

## 将来の展望

- ファジー候補
- インタラクティブピッカー
- エディタ連携

## 名前の由来

- **run**（実行）
- **ex**（expand / execute）
- **rune**（圧縮されたコマンド）

## ライセンス

MIT
