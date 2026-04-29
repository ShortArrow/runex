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

## `runex doctor` で動作確認

`runex doctor` は設定の検証・コマンド解決・シェル連携の健康診断を一度に行います:

```
$ runex doctor
[OK]  config_file: found: ~/.config/runex/config.toml
[OK]  config_parse: config loaded successfully
[OK]  effective_search_path: 116 entries (process=101, +user=0, +system=15)
[OK]  integration:bash: marker found in ~/.bashrc
[OK]  integration:zsh: rcfile not found at ~/.zshrc — assuming this shell is not in use
[WARN] integration:pwsh: marker missing in ~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1 — run `runex init pwsh`
[OK]  integration:nu: rcfile not found at ~/.config/nushell/env.nu — assuming this shell is not in use
[OK]  integration:clink: up-to-date at ~/AppData/Local/clink/runex.lua
[OK]  command:lsd: 'lsd' found (required by 'ls')
```

このうち 2 つの行が特徴的です:

- **`effective_search_path`** *(Windows のみ)*: `when_command_exists` を解決するときに runex が実際に検索する PATH の概要。Windows ではプロセス継承の PATH が縮退している場合 (clink の lua から起動する cmd で発生しがち) にレジストリの HKCU/HKLM `Environment\Path` で補強します。`+user=N, +system=K` が補強で取り込まれた追加エントリ数です。
- **`integration:<shell>`**: 各シェルの連携が組まれているかの確認。bash/zsh/pwsh/nu は rcfile に `# runex-init` マーカーがあるかを見ます。clink は特殊で、lua ファイルが自動再生成されない静的コピーのため、ディスク上の `runex.lua` と現行の `runex export clink` の出力を比較してドリフトを警告します。

## トラブルシューティング

トリガーキーで展開されず単なる空白挿入になる場合:

1. **まず `runex doctor` を実行**してください。多くの問題はここで顕在化します。
2. **`runex hook` が `unrecognized subcommand` で失敗**: 別の `runex` バイナリ (古い AUR/Homebrew/winget パッケージなど、hook サブコマンド導入前のもの) が PATH 上にあって新しいバイナリを隠している可能性があります。シェルテンプレートはエラー時に「リテラル空白挿入」へ safe-fail するので、統合バグのように見えます。`which runex` と `runex version` で解決された実体を確認し、古い方を削除/更新してください。
3. **clink: `ls<Space>` がアップグレード後に展開されない**: `runex.lua` は自動更新されません。`runex doctor` が `WARN integration:clink: outdated …` を出します。`runex export clink > %LOCALAPPDATA%\clink\runex.lua` を再実行して新しい cmd 窓を開いてください。
4. **clink: `lsd` がインストール済なのに `command:lsd not found`**: clink-injection された cmd の PATH が User-scope のレジストリエントリを欠いている可能性があります。runex は Windows ではレジストリでコマンド解決を補強しますが、HKCU/HKLM の `Environment\Path` どちらにも入っていないディレクトリを使っている場合は、いずれかに追加する必要があります (`RUNEX_CLINK_LUA_PATH` で lua ファイル自体の場所を上書きすることも可能)。
