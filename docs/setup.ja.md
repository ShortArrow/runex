# セットアップ

[English](setup.md) | [日本語](setup.ja.md)

runex をインストールしたら、シェルに連携を設定します。`runex init` が最短ルート。シェル別の手動設定は下にまとめています。

## 最短ルート: `runex init`

設定ファイルを作成し、rc ファイルへのシェル連携行の追記を確認付きで行います:

```
$ runex init
Create config at ~/.config/runex/config.toml? [y/N] y
Created: ~/.config/runex/config.toml
Append shell integration to ~/.bashrc? [y/N] y
Appended integration to ~/.bashrc
```

`-y` を付けると確認プロンプトをすべてスキップします。Clink は自動追記に対応していないため、手動で追加してください(下記)。

## シェル別の手動設定

### bash

bash 4.0 以降が必要です。macOS には bash 3.2 が同梱されています。Homebrew で新しいバージョンを入れてください (`brew install bash`)。

`~/.bashrc` に追加:

```bash
eval "$(runex export bash)"
```

### zsh

`~/.zshrc` に追加:

```zsh
eval "$(runex export zsh)"
```

### PowerShell

`$PROFILE` に追加:

```powershell
Invoke-Expression (& runex export pwsh | Out-String)
```

貼り付けたテキストは途中で展開されません (PSReadLine のキーキュー経由で paste を検出し、スペースキーハンドラをスキップします)。

### Nushell

`~/.config/nushell/config.nu` に追加:

```nu
source ~/.config/nushell/runex.nu
```

スクリプトを生成 (設定変更時・runex をアップグレードした時に再実行):

```nu
runex export nu | save --force ~/.config/nushell/runex.nu
```

### cmd (Clink)

Clink のスクリプトディレクトリに追加 (設定変更時・runex をアップグレードした時に再実行):

```cmd
runex export clink > %LOCALAPPDATA%\clink\runex.lua
```

## 次のステップ

- 略語の設定: [README の Config セクション](README.ja.md#設定) と [config-reference](config-reference.md) を参照
- `runex doctor` を実行して設定を検証
