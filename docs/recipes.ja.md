# レシピ

[English](recipes.md) | 日本語

実用的でコピペ可能な `config.toml` スニペット集。やりたいことに合わせ
てレシピを選び、`[[abbr]]` ブロックを設定にコピーして、トリガーキーを
押すだけ。

設定ファイルは `$XDG_CONFIG_HOME/runex/config.toml` (未設定なら
`~/.config/runex/config.toml`)。`RUNEX_CONFIG=<path>` または
`runex --config <path>` で上書き可能。

各フィールドの完全なリファレンスは [config-reference.md](config-reference.md)
(英語のみ — フィールド名・型・バリデーション規則・`runex doctor` 出力
の意味を網羅) を、トリガーキーの設定は [setup.ja.md](setup.ja.md) を
参照。

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

**ユースケース:** 特定のプラットフォームでしか存在しないツールに依存
する場合 — 例えば `wslpath` は WSL 内でしか使えない。展開先がプラッ
トフォーム固有のシンタックス (PowerShell の `$env:USERPROFILE` など)
を使う側では precondition は不要なので、`when_command_exists` を空配
列にして「条件なし、常に展開」と表現する。

```toml
[[abbr]]
key    = "winhome"
expand = { default = "/mnt/c/Users/$USER", pwsh = "$env:USERPROFILE" }
when_command_exists = { default = ["wslpath"], pwsh = [] }
```

**動作:** WSL bash では `wslpath` が PATH 上にある時 (= 実際に WSL の
中) だけ `winhome<Space>` が展開される。pwsh では空配列が
precondition を short-circuit する = 常に展開。空の
`when_command_exists` は「失敗」ではなく「条件なし」扱い。

---

## 9. `sudo` 後でも展開される

**ユースケース:** `sudo` の後でも略語を効かせたい。runex のコマンド位
置判定は `sudo <token>` を行頭の `<token>` と同等に扱う。同様に `|`,
`||`, `&&`, `;` の直後もコマンド位置として認識する。

```toml
[[abbr]]
key    = "apt-update"
expand = "apt update"
```

**動作:**

```
sudo apt-update<Space>
```

→ `sudo apt update `。`runex which apt-update --why` でマッチ理由を
確認できる。

### 落とし穴: `sudo <abbr>` は `&&` の右側に `sudo` を伝播しない

`sudo` は直後の 1 コマンドにしか効かない。展開結果に `&&` や `;` が
含まれる場合、それらの右側のコマンドは **通常ユーザーで** 実行される。
よくある罠:

```toml
[[abbr]]
key    = "apt-up"
expand = "apt update && apt upgrade"   # NG: apt upgrade は root にならない
```

```
sudo apt-up<Space>
# 展開後: sudo apt update && apt upgrade
# `apt update` は root、`apt upgrade` は通常ユーザーで失敗する。
```

パイプライン全体を root で動かしたいなら、各コマンドに個別に `sudo`
を埋め込み、abbr 自体は `sudo` なしで呼ぶのが安全 (issue #4):

```toml
[[abbr]]
key    = "aptup"
expand = "sudo apt update && sudo apt upgrade"   # OK: 両方 root
```

```
aptup<Space>
# 展開後: sudo apt update && sudo apt upgrade
```

使い分けの目安:

- **1 コマンド** → コマンド行側に `sudo` を書き (`sudo abbr`)、`expand`
  には含めない。
- **複数コマンド (`&&`, `;`, `|`)** → 各コマンドに個別に `sudo` を
  埋め込み、abbr は素で呼ぶ。

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

## 13. 大量の登録の中から 1 件だけ表示する

**ユースケース:** 登録が増えて `runex list` の出力が画面に収まらず、
目視で目的の key を探すのが大変。

```bash
runex list ll
# ll<TAB>ls -la
```

positional 引数として key を渡すと、その key と **完全一致** する
1 件だけを表示する。大文字小文字を区別し、prefix 一致では拾わない
(`runex list ll` は `ll.` を含まない)。

ヒットしないときは exit 0 で空出力なので、`[[ -z "$(runex list X)" ]]`
のようなシェル判定にも素直に使える。

`--json` を付けても同じく filter 後の配列を返す:

```bash
runex list ll --json
# [
#   { "key": "ll", "expand": "ls -la", "when_command_exists": null }
# ]
```

prefix / 部分一致 / fuzzy 検索が欲しい場合は `runex which <token>`
の方が用途に近い (per-shell 解決結果や `when_command_exists` の
判定理由まで一緒に出る)。

---

## 14. `{number}` で数値繰り返し展開

**ユースケース:** `up`, `up2`, `up3`, …, `up10` を全部別ルールで書く
のが面倒。実態は `../` を回数ぶん繰り返したいだけ。

```toml
[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"
```

```
up3<Space>     # → cd ../../../
up10<Space>    # → cd ../../../../../../../../../../
```

`{number}` placeholder は `key` 側 (末尾の数字を捕捉) と `expand`
側 (`number * <捕捉回数>` に置換) の両方で意味を持つ。

### exact ルールとの共存

exact key ルールは pattern ルールより常に優先される。これで pattern
の上に特例を重ねられる:

```toml
[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"

[[abbr]]
key    = "up"          # 素の `up` は pattern に hit しない (数字なし)
expand = "cd .."

[[abbr]]
key    = "up3"         # `up3` だけ特別扱いしたい
expand = "cd ~/notes"
```

```
up<Space>      # → cd ..
up2<Space>     # → cd ../../
up3<Space>     # → cd ~/notes   (exact ルール優先)
up4<Space>     # → cd ../../../../
```

### 制限と注意点

- 認識される placeholder は `{number}` のみ。`{foo}` など他の
  `{...}` は parse 時点で reject。
- 捕捉できる数値は 1〜128。`up0` / `up129` は pass-through。
- `number` unit は 32 bytes 以内。これで展開結果が既存の `expand`
  上限 4096 bytes を超えないことが保証される。
- ASCII 半角数字のみ。`up3<Space>` は OK、`up３` (全角) は NG。
- cursor placeholder `{}` と同居できる。named 置換が先に走り、
  その後 `{}` が除去されてカーソルがその位置に置かれる。

---

## 次に読む

- 全フィールドリファレンス: [config-reference.md](config-reference.md) (英語のみ — `[keybind]` / `[[abbr]]` の各フィールド、バリデーション、`runex doctor` 各行の意味)
- シェル別セットアップ詳細: [setup.ja.md](setup.ja.md)
- トラブルシューティング: [setup.ja.md → トラブルシューティング](setup.ja.md#トラブルシューティング)
- 設定変更後は `runex doctor` で検証する習慣をつける
