# ADR 0003: clink sends the buffer to `runex hook` hex-encoded

- **Status**: Accepted
- **Phase**: 0.1.21 candidate
- **Supersedes**: the `runex_is_safe_line` gate plus `\"` escaping in
  `templates/clink.lua` shipped in 0.1.15–0.1.20
- **Authors**: ShortArrow, with Claude Code collaboration on the
  alternatives analysis
- **Date**: 2026-09-09

---

## Context

clink's lua has one way to reach runex: `io.popen`, which on Windows
runs `cmd.exe /c <string>`. The template embedded the live buffer in
that string as `--line "<buffer>"`, escaping `"` as `\"`. Issues #22
and #23 showed what that does to a buffer such as `pwsh -nop -c "mv`:

```
C:\Users\who>pwsh -nop -c "mverror: unexpected argument '2>&1' found
```

cmd.exe has no escape for `"`. It toggles quote state at every `"`
it sees, so the embedded quote closed the `--line` argument early,
the rest of the string (`--cursor N 2>&1`) was parsed as quoted
text, the `2>&1` redirection never happened, and runex received it
as a positional argument. clap's usage error then came back on
stdout and was printed into the prompt. A pasted path with a `"`
triggers the same failure once per space in the paste.

`"` was the third cmd.exe metacharacter to bite this path. `%VAR%`
(expanded even inside quotes) and `!VAR!` (expanded under delayed
expansion) had already forced a `runex_is_safe_line` gate that made
the trigger key fall back to a literal space whenever the buffer
contained `%` or `!`, costing users the expansion on those lines.

## Decision

The buffer travels as hex of its UTF-8 bytes:

```
"<runex.exe>" hook --shell clink --line-hex 7077736820... --cursor 16 2>&1
```

- `templates/clink.lua` encodes with `string.format('%02X', byte)`
  and no longer embeds the raw buffer anywhere in the cmd.exe string.
  The `%` / `!` gate is removed because the wire alphabet `[0-9A-F]`
  contains nothing cmd.exe interprets.
- `runex hook` gains `--line-hex <HEX>` as an alternative to `--line`
  (clap group `buffer`, exactly one required). Decoding lives in
  `app::hook::decode_hex_line`, next to the other transport
  conversions, and rejects odd length, non-hex digits and invalid
  UTF-8. A rejection is a non-zero exit, which the template already
  treats as "insert a literal space".
- cmd.exe refuses command lines over 8191 characters and hex doubles
  the buffer, so the template measures the assembled string and falls
  back to a literal space above that limit. The practical ceiling for
  clink expansion is therefore a buffer of roughly 4000 bytes; the
  Rust-side cap `MAX_HOOK_LINE_BYTES` (16 KiB) still applies after
  decoding.

`runex init clink` output generated before this change keeps calling
`--line "<buffer>"`, which still works, so old installs degrade to
the old behaviour rather than breaking. `runex doctor` reports the
stale template.

## Alternatives considered

- **Double every `"` (`""`) and double backslashes before quotes.**
  Keeps cmd.exe's quote parity and matches the MSVC argv rules Rust
  follows. Rejected: it depends on two parsers' undocumented corner
  cases (a `\` before a `"`, trailing backslashes, `""` inside
  quotes) and still leaves `%` and `!` to the gate.
- **Pass the buffer through an environment variable** (`os.setenv`
  in clink, inherited by cmd.exe and runex). Rejected: mutates the
  clink process for the duration of the call, and `cmd.exe` still
  expands `%VAR%` in the command string if the value is referenced
  there, so it only moves the problem.
- **Percent-encode only the unsafe characters.** Smaller wire size
  than hex, but the unsafe set is exactly the list that keeps
  growing (`"`, `%`, `!`, `^`, `&`, `|`, `<`, `>`, control
  characters, and whatever the console code page does to non-ASCII).
  Hex has no such list.
- **Write the buffer to a temp file.** One more I/O per keystroke
  and a cleanup obligation, for no gain over hex at these sizes.

## Consequences

- clink users can expand on lines containing `"`, `%`, `!` and any
  other character; the literal-space fallback now only fires for
  buffers too long for cmd.exe.
- `hook` has two input forms. Every other shell keeps `--line`;
  `--line-hex` exists for the one shell whose IPC channel is a
  cmd.exe command line, and `docs/config-reference.md` says so.
- `runex/tests/cli_integration.rs` runs the template's exact command
  string through a real cmd.exe on Windows
  (`hook_clink_cmd_exe_roundtrip_keeps_double_quote_in_buffer`).
  That test mirrors the lua layout by hand because clink ships no
  standalone lua interpreter; the lua comment names the test so the
  two are changed together.
