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

Next steps:
  1. Reload your shell: `source ~/.bashrc` (or `exec $SHELL`)
  2. Try `gst<Space>` — it should expand to `git status `.
  3. Add your own abbreviations: see https://github.com/ShortArrow/runex/blob/main/docs/recipes.md
  4. Verify any time with: `runex doctor`
```

`-y` を付けると確認プロンプトをすべてスキップします。シェル名を渡せば
auto-detection をスキップして特定のシェルだけを対象にできます (例:
`runex init pwsh`、`runex init clink`)。clink は rcfile ではなく lua
ファイルとして書かれます。

## `runex init` が何をする / しないか

`init` で最も多い不安は「既存の rcfile を壊さないか?」です。**壊しま
せん**。詳細:

**やること:**

- `~/.config/runex/config.toml` が存在しない場合のみ作成する
  (`OpenOptions::create_new` を使うため、既存ファイルは絶対に上書き
  しない)。デフォルトの設定には `gst → git status` のサンプルが入っ
  ているので、インストール直後に展開動作を確認できる。
- rcfile に `# runex-init` マーカー付きの 1 ブロックを追記する:

  ```bash
  # runex-init
  eval "$(runex export bash)"
  ```

- 各ファイル書き込み前に確認プロンプトを出す (`-y` で全スキップ可)。

**やらないこと:**

- **既存行を絶対に変更しない**。すべての書き込みが
  `OpenOptions::append` 経由なので、既存内容はバイト単位で保持され、
  新ブロックは末尾に追加される。
- **既存 rcfile を上書きしない**。
- **シンボリックリンクを辿らない** (Unix では `O_NOFOLLOW`)。
- **`# runex-init` マーカーが既にあれば再追記しない** — `runex init`
  はべき等で、2 回実行しても結果は単一ブロックのまま。
- **1 MB を超える rcfile では実行しない** (安全制限。サイズオーバー時
  はマーカー欠落として扱い fail safe)。

**アンインストール:** `# runex-init` 行から次の空行までを手で削除する
だけ。runex は `runex init` を再実行しない限りそのブロックに書き戻し
ません。シードした config も消したい場合は
`rm ~/.config/runex/config.toml`。

**clink は別扱い**: lua ファイルは `%LOCALAPPDATA%\clink\runex.lua`
(または `RUNEX_CLINK_LUA_PATH` 指定先) に静的コピーとして書く。
`runex init clink` はディスク上の内容と現在の正規 export 結果を比較
し、ドリフトしている場合のみ確認プロンプト + 上書き。同一なら no-op。

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

0.1.15 以降、`runex init pwsh` は `%LOCALAPPDATA%\runex\integration.ps1`
に静的キャッシュファイルを書き、`$PROFILE` に 1 行 `. <cache>` を
追記します。一度実行するだけで設定完了します:

```powershell
runex init pwsh
```

手で書く場合の最小形:

```powershell
if (Test-Path "$env:LOCALAPPDATA\runex\integration.ps1") { . "$env:LOCALAPPDATA\runex\integration.ps1" }
```

貼り付けたテキストは途中で展開されません (PSReadLine のキーキュー
経由で paste を検出し、スペースキーハンドラをスキップします)。

#### PSReadLine 必須

pwsh integration はトリガーキーに対して
`Set-PSReadLineKeyHandler` を登録するため、**ホスト側で PSReadLine
が load 可能** である必要があります。PowerShell 7 (`pwsh.exe`) と
Windows PowerShell 5.1 (`powershell.exe`) はどちらも PSReadLine
を同梱しており、integration template はどちらも同等に扱います。

Windows PowerShell 5.1 特有のつまずきポイントが 2 つ:

1. **実行ポリシー `Restricted`。** PS5 のデフォルトの
   LocalMachine ポリシーは `Restricted` で、`.ps1` キャッシュ
   ファイルの dot-source が拒否されます。`runex init pwsh` は
   キャッシュを書きますが、プロファイル側で source できません。
   解決:
   ```powershell
   Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
   ```
   (`RemoteSigned` は PS7 のデフォルトで、ほとんどのユーザー
   インストール modules が前提とするポリシーです。)
2. **`AllSigned` ポリシー + 古い vs 新しい PSReadLine 衝突。**
   ポリシーが `AllSigned` で、PS5 が `Documents\PowerShell\Modules`
   (PS7 と共有) から新しい PSReadLine を拾うと、load の都度
   「信頼されていない発行者からのスクリプトを実行しますか?」
   プロンプトが出ます。対処:
   - 一度 `[A] Always run` を選ぶ (Microsoft の証明書を
     `TrustedPublisher` ストアに追加)
   - もしくは PS5 の `$env:PSModulePath` から
     `Documents\PowerShell\Modules` を除外し、同梱の
     PSReadLine 2.0.0 (既に信頼済み) を load させる

`runex doctor` が `integration:pwsh: marker found` と表示するのに
Space トリガーが効かない場合、まずこの 2 つを疑います。
`Get-Module PSReadLine | Format-List Name,Version,Path` で
PSReadLine が実際に load されたか、どこから来たかを確認できます。

#### `$PROFILE` は pwsh 7 と PowerShell 5 で異なる

pwsh 7 は `Documents\PowerShell\Microsoft.PowerShell_profile.ps1` を
読み、Windows PowerShell 5 は
`Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` を
読みます。`runex init pwsh` は実行中のホストが見ている `$PROFILE`
にキャッシュ source 行を書くので、両方使い分けるなら **pwsh 7 と
PowerShell 5 それぞれから `runex init pwsh` を実行** してください。

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
5. **pwsh: marker found なのに Space が反応しない**: ほぼ次の 3 つのどれかです。(a) PSReadLine が load されていない — `Get-Module PSReadLine` で行が出るか確認、出なければインストール or import。(b) Windows PowerShell 5 の実行ポリシーが `Restricted` でキャッシュファイルの dot-source が拒否されている — `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` で解消。(c) `AllSigned` ポリシーで `Documents\PowerShell\Modules` 配下の新しい PSReadLine が拒否されている — 一度 `[A] Always run` を選ぶか `$env:PSModulePath` から該当ディレクトリを除外。[PowerShell セットアップセクション](#powershell) に詳細コマンドあり。
6. **pwsh: キャッシュヘッダが二重 / `__runex_queued_key_count` が認識されない**: キャッシュファイルに `# runex-integration-version` ヘッダが 2 段ある状態 (例: 何かが `runex export pwsh` の出力に独自ヘッダを連結した結果)。キャッシュの interactive guard が function 定義より前で評価されるため、不正なキャッシュは function 定義をスキップします。`runex init pwsh` を再実行してキャッシュを書き直してください。
