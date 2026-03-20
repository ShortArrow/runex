# runex

[English](../README.md) | 日本語

> その一打を、術式へ昇華せよ。

runex は、短いトークンをリアルタイムに完全なコマンドへ展開する、クロスシェル対応の略語エンジンです。  
手数は最小に、発動する一撃は最大に。日々の反復入力を、即応の詠唱へ変換します。

## 特性

- クロスシェル対応（bash / pwsh / cmd / nu）
- リアルタイム展開（スペーストリガ）
- 設定ファイル1つで管理
- 条件付きルール（OS / シェル / コマンド存在）
- 高速・軽量（Rust コア）

## 核となりし概念

runex は短い入力を **ルーン（rune）** として受け取り、完全な **キャスト（cast）** に展開します。

```text
gcm␣ → git commit -m
ls␣  → lsd
```

## 環境への召喚

```bash
cargo install runex
```

## 導入儀式

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

## 詠唱一覧

```bash
runex expand --token ls   # 単一トークンを展開
runex list                # 全ルーンを一覧表示
runex doctor              # 設定と環境をチェック
runex export <shell>      # シェル連携スクリプトを生成
```

## 発動例

- 入力:  `gcm␣`
- 出力:  `git commit -m ␣`

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

- ファジーマッチングフォールバック
- インタラクティブピッカー
- エディタリレーション

## 名の由来

- run + ex = expand / execute / expression / extension
- rune x (like 7z's "x" for extract)
- rune +x (like chrome's "x" for extensions)

## ライセンス

MIT
