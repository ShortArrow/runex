# ADR 0004: pwsh sends the buffer to `runex hook` hex-encoded

- **Status**: Accepted
- **Phase**: 0.1.21 candidate
- **Supersedes**: the joined `--line=<value>` token in
  `templates/pwsh.ps1` shipped for issue #18
- **Authors**: ShortArrow, with Claude Code collaboration on the
  alternatives analysis
- **Date**: 2026-09-17

---

## Context

The pwsh bootstrap reads the live buffer with
`PSConsoleReadLine::GetBufferState` and hands it to `runex hook` as a
native-command argument. Two PowerShell hosts run that same bootstrap,
and both rewrite the argument before runex sees it.

PowerShell rewrites an argument that begins with `~` to `$HOME`, even
when the value arrives through a variable. That was issue #18, and the
fix shipped in 0.1.20 was to join the option and the value into one
`--line=<value>` token, so the first character of the argument is `-`
and the tilde is no longer in the leading position.

Windows PowerShell 5.1 then showed a second rewrite that joining does
not cover. It builds the child process command line by re-quoting each
argument, and an argument that contains `"` comes out with the quote
parity broken, so the child's CRT splits it again. Measured on
5.1.26100.8894 against runex 0.1.20:

| buffer | 5.1 result | pwsh 7.6.6 result |
|---|---|---|
| `echo "a b" c` | clap: `unexpected argument 'b c'`, no expansion | correct |
| `echo "" c` | one `"` lost, cursor off by one | correct |
| `~\.local x` (#18) | correct (joined token) | correct |

The hook exits non-zero on the clap error, and the bootstrap's fallback
inserts a literal space — so under 5.1 the trigger key silently stopped
expanding on any line holding a `"`, which is most quoted paths.

clink already had the same class of problem for a different reason
(a cmd.exe command line cannot carry `"`, `%` or `!`) and solved it in
ADR 0003 by sending hex of the UTF-8 bytes. `runex hook --line-hex` is
shell-agnostic in clap, so pwsh can use the same wire form.

## Decision

The pwsh bootstrap passes the buffer as hex of its UTF-8 bytes, joined
to the option with `=`:

```powershell
$hookArgs = @('hook', '--shell', 'pwsh', "--line-hex=$([System.BitConverter]::ToString([System.Text.Encoding]::UTF8.GetBytes($line)) -replace '-','')", '--cursor', "$cursor")
```

- The alphabet on the wire is `[0-9A-F]`, which contains no `"`, no
  `~` and nothing else either host rewrites. Measured byte-identical
  and correct under both hosts for `echo "a b" c`, `echo "" c`,
  `echo "a\" c`, `~\.local x`, `echo %PATH% !x! "q"` and
  `echo "日本 語" c`.
- The value stays joined with `=`. An empty buffer produces an empty
  hex string; 5.1 drops a separate empty argument, which would leave
  clap without a value for `--line-hex` on every Space at an empty
  prompt. The joined form survives, so unlike clink the pwsh template
  needs no early return for the empty buffer.
- The encoding is computed inline, in a `$()` subexpression on the
  `$hookArgs = @(` line itself. `BitConverter::ToString` plus a
  `-replace` is the one-expression form available in both hosts, and
  keeping it on that line is also what lets
  `runex/tests/pwsh_integration.rs` lift the line out of the exported
  bootstrap and evaluate it, so the tests exercise the template's real
  argument form rather than a hand-written copy.
- `--cursor` is unchanged: pwsh still counts UTF-16 code units and
  `app::hook::shell_cursor_to_byte` still converts.

## Alternatives considered

- **`$PSNativeCommandArgumentPassing = 'Standard'`.** PowerShell 7.3+
  passes arguments without re-quoting under this preference. Rejected:
  the variable does not exist in 5.1, so it fixes only the host that
  was already correct.
- **`--%` (stop-parsing token).** Hands the rest of the line to the
  child verbatim. Rejected: it also disables variable expansion, so
  `$line` and `$cursor` would arrive as their own literal text.
- **Pre-quote the buffer for the MSVCRT rules** (double the `"`,
  escape the backslashes before them) before interpolating. Rejected:
  it is correct only for the host that re-quotes. pwsh 7 does not, so
  the escapes reach runex as part of the buffer and the same test
  fails on the other host. Two parsers, one string.
- **Keep `--line=` and document the 5.1 limitation**, as
  `docs/setup.md` did. Rejected: the cost is the whole feature on any
  buffer with an odd `"`, and the user sees a literal space with no
  explanation.

## Consequences

- Under Windows PowerShell 5.1 a buffer containing `"` now expands,
  with the cursor where the shell measured it. pwsh 7 behaviour is
  unchanged (the pwsh 7 case was already correct and is now pinned by
  `double_quoted_argument_round_trips`).
- pwsh and clink now share the transport. ADR 0003's statement that
  "clink is the only template that uses it" no longer holds; the flag
  is the general answer for a host that rewrites argument text.
- `--line` remains the form for bash, zsh and nu, whose integrations
  call the binary without an intermediate command-line re-quote.
- The empty buffer is sent to the hook from pwsh (as `--line-hex=`),
  where clink returns early instead. The two templates differ here
  because cmd.exe collapses the empty value and 5.1 does not.
- Pre-0.1.21 `runex init pwsh` caches keep sending `--line=<value>`
  and keep the 5.1 behaviour until regenerated; `runex config reload`
  or `runex init pwsh` refreshes them, and `runex doctor` reports a
  stale cache.
