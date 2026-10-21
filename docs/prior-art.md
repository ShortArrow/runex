# Prior art

[English](prior-art.md) | [日本語](prior-art.ja.md)

runex reinvents an idea that fish shell introduced and zsh-abbr carried to zsh: a word you type is replaced on the command line, before you run it, by the text it stands for. What runex adds is reach. The same rules, from one config file, expand in bash, zsh, PowerShell, Nushell and cmd with clink.

## Three layers of the command line

Each layer below changes something different about what you type.

| Layer | What it changes | Examples |
|---|---|---|
| Keys | what a key press does | emacs and vi key bindings |
| Buffer | how the typed line is edited | GNU Readline in bash, ZLE in zsh, PSReadLine in PowerShell |
| Meaning | what a typed word stands for | fish `abbr`, zsh-abbr, runex |

A line editor's macro inserts the same text wherever it is invoked. An abbreviation is looked up from the word under the cursor and its place on the line, and the expansion is written into the buffer, where it can still be edited before it runs. Unlike an alias, the expanded command is what the shell runs and what its history records.

runex works through the buffer layer of each shell. Its integration is a key handler of that shell's line editor (`bind -x` in bash, a ZLE widget in zsh, a PSReadLine handler in PowerShell, a keybinding in Nushell, a Lua script in clink), which sends the line to `runex hook` and writes back the result.

## fish

fish 2.2.0 (July 2015) added the `abbr` command, "interactively-expanded abbreviations" ([fish-shell#731](https://github.com/fish-shell/fish-shell/issues/731), opened in May 2013). fish 3.6.0 (January 2023) let an abbreviation match a regular expression, expand anywhere on the line rather than only as a command, produce its text from a function, and place the cursor inside the expansion. fish 4.0.0 (February 2025) added abbreviations restricted to the arguments of one command.

## zsh-abbr

[zsh-abbr](https://github.com/olets/zsh-abbr) by olets brings auto-expanding abbreviations to zsh, and its README names fish shell as the inspiration. Its repository dates from January 2019.

## runex

The expansion in runex follows the same model, and much of its rule syntax has a counterpart in fish: regex keys with captures, and cursor placement with `{}`. Two choices differ.

- **One config for every shell.** Rules live in `~/.config/runex/config.toml` and are not tied to the shell that reads them. Where shells differ, a rule gives a per-shell `expand`, or expands only when `when_command_exists` finds its commands.
- **The decision is made by one program.** Each shell's integration is a thin template that hands the buffer to the `runex` binary, written in Rust, and applies its answer. The exception is bash under MSYS2 or Cygwin, where the generated script expands in the shell itself.

## Sources

Read on 2026-10-21.

- fish shell `CHANGELOG.rst`, entries for 2.2.0, 3.6.0 and 4.0.0: <https://github.com/fish-shell/fish-shell/blob/master/CHANGELOG.rst>
- fish shell `abbr` documentation: <https://fishshell.com/docs/current/cmds/abbr.html>
- fish-shell issue #731, "Abbreviations": <https://github.com/fish-shell/fish-shell/issues/731>
- zsh-abbr repository: <https://github.com/olets/zsh-abbr>
