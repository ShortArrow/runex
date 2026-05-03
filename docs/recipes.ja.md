# レシピ

[English](recipes.md) | 日本語

実用的でコピペ可能な `config.toml` スニペット集。やりたいことに合わせ
てレシピを選び、`[[abbr]]` ブロックを設定にコピーして、トリガーキーを
押すだけ。

設定ファイルは `$XDG_CONFIG_HOME/runex/config.toml` (未設定なら
`~/.config/runex/config.toml`)。`RUNEX_CONFIG=<path>` または
`runex --config <path>` で上書き可能。

各フィールドの完全なリファレンスは [config-reference.md](config-reference.md)
(英語のみ)、トリガーキーの設定は [setup.ja.md](setup.ja.md) を参照。

---

## 1. よく使う Git コマンドの省略

**ユースケース:** Git の頻出コマンドを 2-3 文字のトークンに短縮。

```toml
[[abbr]]
key    = "gst"
expand = "git status"

[[abbr]]
key    = "gd"
expand = "git diff"

[[abbr]]
key    = "ga"
expand = "git add"

[[abbr]]
key    = "gco"
expand = "git checkout"

[[abbr]]
key    = "gp"
expand = "git push"

[[abbr]]
key    = "gpl"
expand = "git pull"
```

**動作:** `gst<Space>` → `git status `。トークンの直後にトリガーキーを
押すと、トークンを置き換えてさらに「トリガーキーが本来挿入したスペー
ス」も付与される。

---

## 2. `bat` があれば `cat` を置き換える

**ユースケース:** リッチなページャを使いたいが、無い環境では普通の
`cat` にフォールバック。

```toml
[[abbr]]
key    = "cat"
expand = "bat"
when_command_exists = ["bat"]
```

**動作:** PATH に `bat` があれば `cat<Space>file.rs` → `bat file.rs`、
無ければルールがスキップされて `cat` のまま。`runex doctor` で
`command:bat: 'bat' found (required by 'cat')` が出れば OK。

---

## 3. 3 段フォールバックチェーン

**ユースケース:** `eza` が入っていれば最優先、無ければ `lsd`、それも
無ければ素の `ls`。runex は `[[abbr]]` を上から評価し、
`when_command_exists` を満たす最初のルール (条件なしのものを含む) を
採用する。

```toml
[[abbr]]
key    = "ll"
expand = "eza --long --git --group-directories-first"
when_command_exists = ["eza"]

[[abbr]]
key    = "ll"
expand = "lsd --long --group-dirs first"
when_command_exists = ["lsd"]

[[abbr]]
key    = "ll"
expand = "ls -la"
```

**動作:** `runex which ll --why` でどのルールがマッチしてどれがスキッ
プされたかを確認できる。最後の `ls -la` は条件なしなので、`eza` も
`lsd` も無いときの最終フォールバック。

---

## 4. カーソル位置を指定する展開

**ユースケース:** クォート内など「あとで埋めたい場所」にカーソルを置
いた状態で展開したい。`{}` でカーソル位置を制御する。

```toml
[[abbr]]
key    = "prc"
expand = 'gh pr create --title "{}" --body ""'

[[abbr]]
key    = "issn"
expand = 'gh issue create --title "{}" --body ""'
```

**動作:** `prc<Space>` → `gh pr create --title "" --body ""` でカーソル
はタイトルのクォート内。タイトルを入力して→キーで body へ。`{}` が
無い場合カーソルは展開後の末尾 (デフォルト挙動)。

---

## 5. シェル別トリガーキー

**ユースケース:** bash では Alt+Space (readline がコードで扱いやすい
ため)、その他のシェルでは Space。

```toml
[keybind.trigger]
default = "space"
bash    = "alt-space"
```

**動作:** bash ではトークン後に Alt+Space で展開、zsh / pwsh / nu /
clink は普通の Space で展開。`default` は個別オーバーライドが無い全
シェルに適用。

利用可能な値: `"space"`, `"tab"`, `"alt-space"`, `"shift-space"`
(Shift+Space は pwsh と nu のみ — レシピ 6 参照)。

---

## 6. 展開せずにスペースを入れたい

**ユースケース:** pwsh では普段 Space で展開するが、たまに本当に「ただ
のスペース」を入れたい。Shift+Space を「展開回避キー」に割り当てる。

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
pwsh = "shift-space"
nu   = "shift-space"
```

**動作:** `gst<Shift+Space>` → `gst ` (展開なし)、普通の `<Space>` は
展開。bash / zsh は Shift+Space を確実に検出できないため、これらで
回避キーを使う場合は `"alt-space"` を割り当てる。

---

## 7. Windows と Unix で違うコマンド

**ユースケース:** Unix では `rm -i`、PowerShell では `Remove-Item`。

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
```

**動作:** bash で `rmf<Space>foo.txt` → `rm -i foo.txt`、pwsh で
`Remove-Item foo.txt`。それ以外の個別指定がないシェルは `default` に
フォールバック。同じ per-shell テーブル形式は `when_command_exists`
にも使える:

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
when_command_exists = { default = ["rm"], pwsh = ["Remove-Item"] }
```

---

## 8. プラットフォーム別の依存チェック

**ユースケース:** ツール名がプラットフォームで違う場合 — Windows の
pwsh では `wsl` 経由、Linux では `lsb_release` で判定など。

```toml
[[abbr]]
key    = "winhome"
expand = { default = "/mnt/c/Users/$USER", pwsh = "$env:USERPROFILE" }
when_command_exists = { default = ["wslpath"], pwsh = [] }
```

**動作:** WSL bash では `wslpath` がある時だけ `winhome<Space>` 展開、
pwsh では空配列 = 「条件なし」で常に展開。空の
`when_command_exists` は「失敗」ではなく「条件なし」扱い。

---

## 9. `sudo` 後でも展開される

**ユースケース:** `sudo` の後でも略語を効かせたい。runex のコマンド位
置判定は `sudo <token>` を行頭の `<token>` と同等に扱う。

```toml
[[abbr]]
key    = "apt-up"
expand = "apt update && apt upgrade"
```

**動作:**

```
sudo apt-up<Space>
```

→ `sudo apt update && apt upgrade `。同様に `|`, `||`, `&&`, `;` の直
後もコマンド位置と認識される。`runex which <token> --why` でマッチを
確認できる。

---

## 10. Docker / kubectl コマンド集

**ユースケース:** コンテナ管理の頻出コマンドを 2-4 文字に短縮。

```toml
[[abbr]]
key    = "dps"
expand = "docker ps"

[[abbr]]
key    = "dpsa"
expand = "docker ps -a"

[[abbr]]
key    = "dimg"
expand = "docker images"

[[abbr]]
key    = "dexec"
expand = "docker exec -it"

[[abbr]]
key    = "kg"
expand = "kubectl get"

[[abbr]]
key    = "kgp"
expand = "kubectl get pods"

[[abbr]]
key    = "kga"
expand = "kubectl get all"

[[abbr]]
key    = "kdp"
expand = "kubectl describe pod"

[[abbr]]
key    = "klog"
expand = "kubectl logs -f"
```

**動作:** ツールごとに識別可能なプレフィックス (docker は `d`、
kubectl は `k`) でスコープを切ると、他カテゴリの略語と衝突しにくい。

---

## 11. 既存 alias と衝突したとき

**ユースケース:** `key = "ls"` を書いたのにシェル側に既に `alias ls=...`
があってルールが発火しない (シェルが runex のフックより先に alias を
展開する)。

`runex doctor` がこれを警告する:

```
[WARN]  shell:bash:key:ls: conflicts with existing alias 'ls' -> ls --color=auto
```

回避策は 2 つ:

```toml
# A: 略語の key 名を変える
[[abbr]]
key    = "ll"
expand = "lsd"
when_command_exists = ["lsd"]
```

```bash
# B: rcfile から衝突する alias を外す
unalias ls 2>/dev/null
```

**動作:** どちらかを行ってから再度 `runex doctor` を実行。警告が消え
れば OK。

---

## 12. 「コマンドが見つからない」と言われたとき

**ユースケース:** `runex doctor` が `command:foo: 'foo' not found` と
警告するが、対話シェルでは `which foo` で見える。

doctor 出力を読む:

```
[OK]    effective_search_path: 116 entries (process=101, +user=0, +system=15)
[WARN]  command:foo: 'foo' not found (required by 'bar')
```

照合ポイント:

- **Windows で `+user=0`**: レジストリの User-scope `Environment\Path`
  が継承 PATH に何も追加していない。`foo` が
  `~/AppData/Local/...` 配下にある場合、親プロセスの PATH が縮退して
  いる可能性。[setup.ja.md → トラブルシューティング](setup.ja.md#トラブルシューティング)
  を参照。
- **PATH が完全に未設定**: `effective_search_path` 行が `WARN` で
  `process=0` になる。クリーンな環境でシェルを再起動。
- **`foo` が PATH 上のどこにもない**: インストールするか、シェルの
  PATH に追加してから `runex doctor` を再実行。

---

## 次に読む

- 全フィールドリファレンス: [config-reference.md](config-reference.md) (英語のみ)
- シェル別セットアップ詳細: [setup.ja.md](setup.ja.md)
- トラブルシューティング: [setup.ja.md → トラブルシューティング](setup.ja.md#トラブルシューティング)
- 設定変更後は `runex doctor` で検証する習慣をつける
