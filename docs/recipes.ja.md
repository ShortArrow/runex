# レシピ

[English](recipes.md) | 日本語

コピーしてそのまま使える `config.toml` の設定例です。やりたいことに合うレシピを選び、`[[abbr]]` ブロックを設定ファイルに貼り付け、トリガーキーを押してください。

貼り付けた `[[abbr]]` は次のキー押下から効きます。フックはキー押下のたびに設定を読むためです。`[keybind]` を変えるレシピ（5、6、6b）では、キー束縛がシェル連携に埋め込まれているので、`runex config reload` を実行してから新しいシェルを開いてください。

設定ファイルは `$XDG_CONFIG_HOME/runex/config.toml` にあります。この環境変数が無いときは `~/.config/runex/config.toml` です。`RUNEX_CONFIG=<path>` または `runex --config <path>` で上書きできます。

フィールドの一覧は [config-reference.md](config-reference.md)（英語）に、トリガーキーの設定は [setup.ja.md](setup.ja.md) にあります。

---

## 1. よく使う Git コマンドの省略

**用途:** Git の頻出コマンドを 2〜3 文字にする。

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

**動作:** `gst<Space>` で `git status ` になります。トークンの直後にトリガーキーを押すと、トークンが置き換わり、トリガーキーが本来入れるはずだったスペースも付きます。

---

## 2. `bat` があるときだけ `cat` を置き換える

**用途:** ページャが入っている環境では `bat` を使い、無い環境では `cat` のままにする。

```toml
[[abbr]]
key    = "cat"
expand = "bat"
when_command_exists = ["bat"]
```

**動作:** PATH に `bat` があれば `cat<Space>file.rs` は `bat file.rs` になります。無ければルールは飛ばされ、`cat` のままです。`runex doctor` に `command:bat: 'bat' found (required by 'cat')` と出れば、`bat` は見つかっています。

---

## 3. 3 段のフォールバックチェーン

**用途:** `eza` があれば `eza`、無ければ `lsd`、それも無ければ `ls` を使う。runex は `[[abbr]]` を上から順に評価し、`when_command_exists` を満たす最初のルールを採用します。条件の無いルールは常に満たします。

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

**動作:** `runex which ll --why` で、どのルールが採用され、どのルールが飛ばされたかを確認できます。最後の `ls -la` には条件が無いので、`eza` も `lsd` も無いときの最終候補になります。

---

## 4. カーソル位置を指定する展開

**用途:** 引用符の内側など、あとで入力したい位置にカーソルを置いた状態で展開する。`{}` がカーソルの位置になります。

```toml
[[abbr]]
key    = "prc"
expand = 'gh pr create --title "{}" --body ""'

[[abbr]]
key    = "issn"
expand = 'gh issue create --title "{}" --body ""'
```

**動作:** `prc<Space>` で `gh pr create --title "" --body ""` になり、カーソルはタイトルの引用符の内側に置かれます。タイトルを入力し、右矢印キーで body に移ります。`{}` が無い展開では、カーソルは末尾に置かれます。

---

## 5. シェルごとに違うトリガーキー

**用途:** bash では Alt+Space、それ以外のシェルでは Space で展開する。

```toml
[keybind.trigger]
default = "space"
bash    = "alt-space"
```

**動作:** bash ではトークンの後に Alt+Space を押すと展開します。zsh、pwsh、nu、clink では Space のままです。`default` は個別の指定が無いシェルすべてに使われます。

指定できる値は `"space"`、`"tab"`、`"alt-space"`、`"shift-space"` です。`"shift-space"` は pwsh と nu でだけ使えます（レシピ 6 を参照）。

---

## 6. 展開せずにスペースを入れる

**用途:** 普段は Space で展開するが、ときどき展開せずにスペースだけ入れたい。Shift+Space を「展開しないスペース」に割り当てる。

```toml
[keybind.trigger]
default = "space"

[keybind.self_insert]
pwsh = "shift-space"
nu   = "shift-space"
```

**動作:** `gst<Shift+Space>` は `gst ` のまま残り、`<Space>` は展開します。bash と zsh は Shift+Space を確実に検出できないので、この 2 つで使うなら `"alt-space"` を割り当ててください。

---

## 6b. nu で貼り付けが途中で切れる問題を避ける

**用途:** nu（0.111 以降の reedline）でスペースを含むテキストを貼り付けると、貼り付けの途中で runex の束縛が発火します。nu がキー束縛に使う `executehostcommand` は発火時にコマンドラインをリセットするため、最初のスペース以降が消えます。これは nu 側の挙動で、runex に固有のものではありません。

**第 1 の方法（0.1.14 以降）:** Ctrl+V に、キー押下ごとのトリガーを通さずにクリップボードを直接読む束縛を割り当てます。

```toml
[keybind.trigger]
default = "space"

[keybind.paste_intercept]
nu = "ctrl-v"
```

**動作:** `echo a b c d` を Ctrl+V で貼り付けると、行全体がそのまま入ります。`gst<Space>` は通常どおり展開します。Space はトリガーのままで、Ctrl+V は別経路です。束縛が非表示のサブコマンド `runex paste-clipboard` を呼び、クリップボードの内容をバッファに挿入します。

クリップボードの読み取り元は次の順で探します。Windows はネイティブの `OpenClipboard`、Linux は `wl-paste`、`xclip`、`xsel` の順、WSL はその後に `powershell.exe Get-Clipboard`、macOS は `pbpaste` です。`runex paste-clipboard` が "no clipboard provider found" と出したら、いずれかを入れてください。

**注意点:**
- マウスの中ボタンやターミナルの右クリックによる貼り付けは、Ctrl+V ではなくキーマップ経由で文字を送ります。そのため nu 側の制限を受けたままです。キーボードの Ctrl+V を使うか、下の第 2 の方法を使ってください。
- **Windows Terminal は Ctrl+V を nu に渡す前に横取りします。** 既定で Ctrl+V が `paste` に割り当てられているためです。Ctrl+V を素通しするターミナル（WezTerm と Alacritty で確認済み）に替えるか、Windows Terminal の `settings.json` でその束縛を変更してください。macOS の Terminal.app と Linux の主要なターミナルは Ctrl+V をそのまま nu に渡します。

**第 2 の方法:** トリガーを、貼り付けの文字列には含まれないキーに変えます。

```toml
[keybind.trigger]
default = "space"
nu      = "shift-space"
```

これで `echo a b c d` を貼り付けても切れません。展開したいときは `gst<Shift+Space>` と押します。bash、zsh、pwsh、clink は Space のままで問題ありません。pwsh は貼り付け中フラグを立ててフックを飛ばし、clink の lua 束縛は単独のキー押下でしか発火せず、bash と zsh には貼り付け中に発火する経路がありません。

---

## 6c. WSL と mise の組み合わせでキー押下が遅い

**用途:** WSL の Linux で Space を押すたびにプロンプトが 1 秒ほど消えてから展開される。原因は、キー押下のたびに mise が起動していること。現在の `runex init` が書く静的キャッシュで解消します。

**症状（0.1.14 以前）:**

- `.bashrc` に `runex init` が書いた `eval "$(runex export bash)"` がある。
- `$PATH` で `~/.local/share/mise/shims` が `~/.cargo/bin` より前にある（`mise activate` の標準的な配置）。
- `mise install` が `~/.local/share/mise/shims/runex` に shim を置いている。
- Space を押すたびに `__runex_expand` が `'runex' hook ...` を呼ぶ。PATH は `runex` を mise の shim に解決し、shim が `mise` 本体を起動し、`mise` が本物の runex を `exec` する。WSL の Arch Linux で `time runex hook --shell bash --line ls --cursor 2` を 1 回ずつ計ると、shim 経由で 0.474 秒、バイナリ直接で 0.002 秒でした（2026-05-10、ADR 0001 に記録）。

**対処:** `runex init <shell>` を一度実行し直します。

```bash
runex init bash --yes
exec bash    # または新しいターミナルを開く
```

これで `~/.cache/runex/integration.bash` に runex の絶対パスが埋め込まれ、rc ファイルの `eval $(...)` 行はそのキャッシュを `source` する行に置き換わります。キー押下ごとのフックはバイナリを直接呼ぶので、shim も PATH の検索も通りません。

**確認:**

```sh
runex doctor
# integration:bash:cache: cache up-to-date at ~/.cache/runex/integration.bash
```

この行が WARN なら、本文に直すためのコマンドが書かれています。通常は上の再実行です。

**関連する小さな改善:** `runex hook` は `when_command_exists` の確認に `which::which` を使い、`$PATH` を走査します。WSL では Windows 側から継承した `/mnt/c/...` のエントリが 90 個以上あり、それぞれが 9p 越しに stat されます。WSL の中で Windows のツールを使わないなら、`.bashrc` でそれらを外せます。

```bash
PATH=$(echo "$PATH" | tr ':' '\n' | grep -v '^/mnt/c/' | paste -sd ':' -)
export PATH
```

これは `which::which` の走査を減らすだけの任意の設定です。静的キャッシュが入ったあとは、runex 自体の起動は PATH の形に左右されません。

---

## 7. Windows と Unix で違うコマンドにする

**用途:** Unix では `rm -i`、PowerShell では `Remove-Item` にする。

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
```

**動作:** bash では `rmf<Space>foo.txt` が `rm -i foo.txt` に、pwsh では `Remove-Item foo.txt` になります。個別の指定が無いシェルは `default` を使います。同じテーブル形式は `when_command_exists` にも使えます。

```toml
[[abbr]]
key    = "rmf"
expand = { default = "rm -i", pwsh = "Remove-Item" }
when_command_exists = { default = ["rm"], pwsh = ["Remove-Item"] }
```

---

## 8. プラットフォームごとに依存を確認する

**用途:** 特定のプラットフォームにしか無いツールに依存する。たとえば `wslpath` は WSL の中にしかありません。展開先が PowerShell の `$env:USERPROFILE` のようにそのシェル固有の記法だけで済む側では、`when_command_exists` を空配列にして「条件なし」と書きます。

```toml
[[abbr]]
key    = "winhome"
expand = { default = "/mnt/c/Users/$USER", pwsh = "$env:USERPROFILE" }
when_command_exists = { default = ["wslpath"], pwsh = [] }
```

**動作:** WSL の bash では、`wslpath` が PATH にあるとき（つまり本当に WSL の中にいるとき）だけ `winhome<Space>` が展開します。pwsh では空配列なので常に展開します。空の `when_command_exists` は「失敗」ではなく「条件なし」です。

---

## 9. `sudo` の後でも展開する

**用途:** `sudo` の後ろでも略語を使う。runex のコマンド位置判定は、`sudo <token>` を行頭の `<token>` と同じに扱います。`|`、`||`、`&&`、`;` の直後も同様です。

```toml
[[abbr]]
key    = "apt-update"
expand = "apt update"
```

**動作:**

```
sudo apt-update<Space>
```

これで `sudo apt update ` になります。`runex which apt-update --why` で採用理由を確認できます。

### 落とし穴: `sudo <abbr>` の `sudo` は `&&` の右側には及ばない

`sudo` が効くのは直後の 1 コマンドだけです。展開結果に `&&` や `;` があると、その右側のコマンドは通常のユーザーで実行されます。

```toml
[[abbr]]
key    = "apt-up"
expand = "apt update && apt upgrade"   # NG: apt upgrade は root にならない
```

```
sudo apt-up<Space>
# 展開後: sudo apt update && apt upgrade
# `apt update` は root で、`apt upgrade` は通常ユーザーで実行されて失敗する。
```

全体を root で動かしたいときは、各コマンドに `sudo` を書き、略語は `sudo` を付けずに使います（issue #4）。

```toml
[[abbr]]
key    = "aptup"
expand = "sudo apt update && sudo apt upgrade"   # OK: 両方 root
```

```
aptup<Space>
# 展開後: sudo apt update && sudo apt upgrade
```

使い分けは次のとおりです。

- **1 コマンド**なら、コマンド行に `sudo` を書き（`sudo abbr`）、`expand` には入れない。
- **複数コマンド（`&&`、`;`、`|`）**なら、`expand` の各コマンドに `sudo` を入れ、略語は単独で使う。

---

## 10. Docker と kubectl のコマンド集

**用途:** コンテナ操作の頻出コマンドを 2〜4 文字にする。

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

**動作:** ツールごとに先頭文字を決めておくと（docker は `d`、kubectl は `k`）、他の分類の略語と衝突しにくくなります。

---

## 11. 既存の alias と名前が衝突する

**用途:** `key = "ls"` と書いたのに、シェルに `alias ls=...` があってルールが発火しない。シェルは runex のフックより先に alias を展開します。

`runex doctor` はこの衝突を警告します。

```
[WARN]  shell:bash:key:ls: conflicts with existing alias 'ls' -> ls --color=auto
```

対処は 2 つあります。

```toml
# A: 略語の key を変える
[[abbr]]
key    = "ll"
expand = "lsd"
when_command_exists = ["lsd"]
```

```bash
# B: rc ファイルから衝突する alias を外す
unalias ls 2>/dev/null
```

**動作:** どちらかを行ってから `runex doctor` を再実行すると、警告が消えます。

---

## 12. 「コマンドが見つからない」と言われる

**用途:** 対話シェルでは `which foo` で見つかるのに、`runex doctor` が `command:foo: 'foo' not found` と警告する。

doctor の出力を読みます。

```
[OK]    effective_search_path: 116 entries (process=101, +user=0, +system=15)
[WARN]  command:foo: 'foo' not found (required by 'bar')
```

確認する点は次のとおりです。

- **Windows で `+user=0`:** レジストリのユーザー側 `Environment\Path` が、継承した PATH に何も足していません。`foo` が `~/AppData/Local/...` にあるなら、親プロセスの PATH が不完全な可能性があります。[setup.ja.md のトラブルシューティング](setup.ja.md#トラブルシューティング) を参照してください。
- **PATH がまったく無い:** `effective_search_path` の行が `WARN` になり、`process=0` と出ます。環境を整えてシェルを起動し直してください。
- **`foo` がどの PATH にも無い:** インストールするか、シェルの PATH にディレクトリを追加してから `runex doctor` を再実行してください。

---

## 13. 多数の登録から 1 件だけ表示する

**用途:** 登録が増えて `runex list` の出力が画面に収まらず、目当ての key を探しにくい。

```bash
runex list ll
# ll<TAB>ls -la
```

`runex list` に key を位置引数で渡すと、その key に**完全一致**する 1 件だけを表示します。大文字と小文字を区別し、前方一致では拾いません（`runex list ll` は `ll.` を表示しません）。

一致しないときは終了コード 0 で何も出力しません。`[[ -z "$(runex list X)" ]]` のようなシェルの判定にそのまま使えます。

`--json` を付けても、絞り込んだ配列を返します。

```bash
runex list ll --json
# [
#   { "key": "ll", "expand": "ls -la", "when_command_exists": null }
# ]
```

前方一致や部分一致で探したいときは `runex which <token>` の方が向いています。シェルごとの展開結果と `when_command_exists` の判定理由も一緒に表示されます。

---

## 14. `{number}` で回数を指定して展開する

**用途:** `up`、`up2`、`up3`、…、`up10` を別々のルールとして書くのは手間です。実際にやりたいのは `../` を回数分繰り返すことだけです。

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

`{number}` は `key` では末尾の数字を受け取り、`expand` では `number` を受け取った回数だけ繰り返した文字列に置き換わります。

### 完全一致のルールと共存させる

完全一致の key を持つルールは、パターンのルールより常に優先されます。パターンの上に特例を重ねられます。

```toml
[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"

[[abbr]]
key    = "up"          # `up` だけでは数字が無いのでパターンに一致しない
expand = "cd .."

[[abbr]]
key    = "up3"         # `up3` だけ特別扱いする
expand = "cd ~/notes"
```

```
up<Space>      # → cd ..
up2<Space>     # → cd ../../
up3<Space>     # → cd ~/notes   (完全一致が優先)
up4<Space>     # → cd ../../../../
```

### 制限と注意点

- 使えるプレースホルダは `{number}` だけです。`{foo}` など他の `{...}` は設定の読み込み時に拒否されます。
- 受け取れる数値は 1〜128 です。`up0` と `up129` はそのまま通過します。
- `number` の単位は 32 バイト以内です。この上限で、展開結果が `expand` の上限 4096 バイトを超えません。
- 数字は ASCII の半角だけです。`up3<Space>` は展開し、全角の `up３` は展開しません。
- カーソルの `{}` と同じ `expand` に書けます。先に `{number}` を置換し、その後に `{}` を取り除いてカーソルを置きます。

---

## 次に読む

- フィールドの一覧: [config-reference.md](config-reference.md)（英語。`[keybind]` と `[[abbr]]` の各フィールド、検証規則、`runex doctor` 各行の意味）
- シェル別のセットアップ: [setup.ja.md](setup.ja.md)
- トラブルシューティング: [setup.ja.md のトラブルシューティング](setup.ja.md#トラブルシューティング)
- 設定を変えたら `runex doctor` で確認する
