# 先行事例

[English](prior-art.md) | 日本語

runex は、fish shell が導入し、zsh-abbr が zsh に持ち込んだ仕組みの再発明です。打った単語を、実行する前にコマンドライン上で、その単語が表す文字列に置き換えます。runex が新しく加えたのは、使えるシェルの範囲です。1 つの設定ファイルに書いた同じルールが、bash、zsh、PowerShell、Nushell、clink を入れた cmd で展開されます。

## コマンドラインの 3 つの層

次の 3 つの層は、入力のそれぞれ別の部分を変えます。

| 層 | 変えるもの | 例 |
|---|---|---|
| キー | キーを押したときの動作 | emacs と vi のキーバインド |
| バッファ | 打った行の編集 | bash の GNU Readline、zsh の ZLE、PowerShell の PSReadLine |
| 意味 | 打った単語が表すもの | fish の `abbr`、zsh-abbr、runex |

行エディタのマクロは、呼び出した場所にいつも同じ文字列を入れます。略語は、カーソル下の単語と行の中での位置から展開先を引き、結果をバッファに書き込みます。展開後の文字列は、実行する前に編集できます。エイリアスと違って、シェルが実行し、履歴に残るのは展開後のコマンドです。

runex は各シェルのバッファの層を通して動きます。シェル連携の中身は、そのシェルの行エディタのキーハンドラーです。bash では `bind -x`、zsh では ZLE ウィジェット、PowerShell では PSReadLine のハンドラー、Nushell ではキーバインド、clink では Lua スクリプトを使います。ハンドラーは行を `runex hook` に渡し、返ってきた結果を書き戻します。

## fish

fish 2.2.0（2015 年 7 月）で `abbr` コマンドが入りました。対話的に展開される略語です（[fish-shell#731](https://github.com/fish-shell/fish-shell/issues/731)、2013 年 5 月に起票）。fish 3.6.0（2023 年 1 月）では、略語が正規表現でマッチできるようになりました。同じ版で、コマンドの位置に限らず行のどこでも展開する指定、関数で展開先を作る指定、展開後のカーソル位置の指定も入りました。fish 4.0.0（2025 年 2 月）では、特定のコマンドの引数でだけ展開する略語が加わりました。

## zsh-abbr

olets による [zsh-abbr](https://github.com/olets/zsh-abbr) は、自動で展開される略語を zsh で使えるようにします。README には fish shell に着想を得たと書かれています。リポジトリは 2019 年 1 月に作られました。

## runex

runex の展開も同じモデルに従っています。キャプチャ付きの正規表現キーや、`{}` によるカーソル位置の指定など、ルールの書き方の多くには fish に対応するものがあります。違うのは次の 2 点です。

- **全シェルで 1 つの設定。** ルールは `~/.config/runex/config.toml` に書き、読み込むシェルには縛られません。シェルごとに違えたいときは、ルールに `expand` をシェル別に書くか、`when_command_exists` で必要なコマンドがあるときだけ展開させます。
- **判定するのは 1 つのプログラム。** 各シェルの連携は薄いテンプレートで、バッファを Rust で書いた `runex` バイナリに渡し、その答えを適用するだけです。例外は MSYS2 と Cygwin の bash で、生成したスクリプトがシェルの中で展開します。

## 出典

2026-10-21 に確認しました。

- fish shell の `CHANGELOG.rst`、2.2.0、3.6.0、4.0.0 の項: <https://github.com/fish-shell/fish-shell/blob/master/CHANGELOG.rst>
- fish shell の `abbr` のドキュメント: <https://fishshell.com/docs/current/cmds/abbr.html>
- fish-shell の issue #731「Abbreviations」: <https://github.com/fish-shell/fish-shell/issues/731>
- zsh-abbr のリポジトリ: <https://github.com/olets/zsh-abbr>
