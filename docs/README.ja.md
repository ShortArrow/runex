# runex

[English](../README.md) | 日本語

> その一打を、術式へ昇華せよ。

runex は、短いトークンをリアルタイムに完全なコマンドへ展開する、クロスシェル対応の略語エンジンです。  
手数は最小に、発動する一撃は最大に。日々の反復入力を、即応の詠唱へ変換します。

## 特性

- クロスシェル対応（bash / pwsh / cmd / nu）
- リアルタイム展開（トリガは変更可能）
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

一時適用:

```powershell
Invoke-Expression ((& runex export pwsh) -join "`n")
```

永続化（`$PROFILE`）:

```powershell
if (!(Test-Path $PROFILE)) { New-Item -Type File -Path $PROFILE -Force }
Add-Content $PROFILE 'Invoke-Expression ((& runex export pwsh) -join "`n")'
```

### bash

一時適用:

```bash
eval "$(runex export bash)"
```

永続化（`~/.bashrc`）:

```bash
echo 'eval "$(runex export bash)"' >> ~/.bashrc
```

### Nushell

一時適用:

```nu
runex export nu | save ~/.config/nu/runex.nu
```

永続化（`config.nu`）:

```nu
mkdir ~/.config/nu
runex export nu | save -f ~/.config/nu/runex.nu
open ~/.config/nu/config.nu
```

次の1行を `config.nu` に追加:

```nu
source ~/.config/nu/runex.nu
```

### cmd (Clink)

一時適用 / スクリプト配置:

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

永続化:
Clink が `%LOCALAPPDATA%\clink\*.lua` を読む設定なら、上の配置だけで有効です。

## 設定

`~/.config/runex/config.toml`

```toml
version = 1

[keybind]
trigger = "space"

[[abbr]]
key = "ls"
expand = "lsd"

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

指定できるキー:

- `space`
- `tab`
- `alt-space`

`trigger` は全シェル共通の既定値です。`bash`、`pwsh`、`nu` を書くと、そのシェルだけ個別に上書きできます。

上書き例:

```toml
[keybind]
trigger = "space"
bash = "alt-space"
```

複数シェルや複数環境で物理的に同じ設定ファイルを共有したい場合は、`runex` 読み込み前に `RUNEX_CONFIG` でそのパスを指定します。

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
