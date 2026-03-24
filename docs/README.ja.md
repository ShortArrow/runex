# runex

[English](../README.md) | 日本語

> その一打を、術式へ昇華せよ。

runex は、短いトークンをリアルタイムに完全なコマンドへ展開する、クロスシェル対応の略語エンジンです。  
手数は最小に、発動する一撃は最大に。日々の反復入力を、即応の詠唱へ変換します。

## 特性

- クロスシェル対応（bash / zsh / pwsh / cmd）
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

生成されたシェルスクリプトと `config.toml` は、そのままローカルのシェル環境へ入ります。同期・読込するファイルは、自分で信頼できるものだけにしてください。

インストール後に `runex` が見つからない場合は、Cargo の bin ディレクトリが `PATH` に入っているか確認してください。

- Unix 系シェル: `~/.cargo/bin`
- Windows: `%USERPROFILE%\.cargo\bin`

## 導入儀式

### PowerShell

`$PROFILE`:

```powershell
Invoke-Expression ((& runex export pwsh) -join "`n")
```

### bash

`~/.bashrc`:

```bash
eval "$(runex export bash)"
```

### zsh

`~/.zshrc`:

```zsh
eval "$(runex export zsh)"
```

### Nushell（Experimental）

Nushell 連携は現状 experimental です。

`config.nu`:

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

`%LOCALAPPDATA%\clink\runex.lua`:

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## 設定

`~/.config/runex/config.toml`

keybind を設定するまでは、どのキーにも何も割り当てられません。

```toml
version = 1

[keybind]
trigger = "space"

[[abbr]]
key = "ll."
expand = "ls -la"

[[abbr]]
key = "ll"
expand = "ls -l"

[[abbr]]
key = "gcm"
expand = "git commit -m"
```

`expand` はそのまま各シェルのネイティブな文字列として扱われます。`runex` はその中身を再解釈したり、シェル向けに再エスケープしたりしません。

指定できるキー:

- `space`
- `tab`
- `alt-space`

`trigger` は全シェル共通の展開キーの既定値です。
`bash`、`zsh`、`pwsh`、`nu` を書くと、そのシェルだけ個別に上書きできます。

上書き例:

```toml
[keybind]
trigger = "space"
bash = "alt-space"
zsh = "tab"
```

複数シェルや複数環境で物理的に同じ設定ファイルを共有したい場合は、`runex` 読み込み前に `RUNEX_CONFIG` でそのパスを指定します。

## 展開を回避したいとき

`trigger = "space"` を使う場合、必要なときだけ展開を避ける方法があります。

- 多くの端末設定では、`Shift+Space` で `runex` を発火させずに普通の空白を入れられます。ただし、これは端末や line editor 依存です。
- bash では、先頭に `\` を付けると一致しなくなるので、`\ls` のように書けば展開されません。`command ls` でも回避できます。
- PowerShell では `\ls` は bash のような escape ではなく、ただ別のトークンになるだけです。`ls` のような標準 alias をそのまま使いたいなら、`Get-ChildItem` のように完全なコマンド名を書く方が安全です。

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

- run + ex = expand / execute / express / extract / explode
- rune x (like 7z's "x" for extract)
- rune +x (like chmod's "+x" execute)

## ライセンス

MIT
