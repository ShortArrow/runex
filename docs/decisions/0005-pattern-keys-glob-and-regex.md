# ADR 0005: Pattern keys via a `match` field (glob now, regex next)

- **Status**: Accepted
- **Phase**: 0.1.22 candidate
- **Issues**: #19 (wildcard), #20 (regex)
- **Authors**: ShortArrow, with Claude Code collaboration on the
  alternatives analysis
- **Date**: 2026-09-30

---

## Context

A rule's `key` is matched against one token: the whitespace-free word
before the cursor, in command position. Two kinds of key exist today:

- an exact key (`gst`), and
- a key containing `{number}` (`up{number}`), which captures trailing
  digits and repeats the rule's `number` unit in `expand`.

Exact rules always win over `{number}` rules, whatever the order in the
config. Captures travel in `domain::expand::Bindings`, which was built
so that further placeholders could be added.

Issues #19 and #20 ask for wildcard and regular-expression keys. A key
may already contain `*`, `?`, `.` or `^` today and is then matched
literally, so giving those characters a meaning inside `key` would
silently change existing configs.

## Decision

1. **Opt-in `match` field.** `[[abbr]]` gains `match = "glob"`; `match
   = "regex"` follows in a separate change (#20). Without `match` a key
   keeps its current meaning, including `{number}`.

   ```toml
   [[abbr]]
   key    = "g*"
   match  = "glob"
   expand = "git {*}"
   ```

2. **Glob syntax is `*` and `?` only.** `*` matches zero or more
   characters, `?` exactly one. A glob key holds at most one `*` and at
   least one wildcard. `[`, `]`, `{`, `}`, `(`, `)` and `\` are rejected
   in a glob key: the Git Bash bake dispatcher hands the key to a bash
   `case` pattern, where those characters would acquire meanings the
   Rust matcher does not share.

3. **Captures.** In a glob rule, `{*}` in `expand` is replaced by the
   text the `*` matched. Regex rules will expose `{1}`…`{9}` and
   `{name}` for their capture groups.

4. **Precedence by kind, then by order.** Exact > `{number}` > glob >
   regex. Within one kind the first rule in the config wins. This keeps
   the existing guarantee that an exact rule beats any pattern.

5. **Self-loop guard.** A pattern rule whose rendered expansion equals
   the token is skipped, like an exact rule whose `expand` equals its
   `key`.

6. **Order of work.** Glob ships first; it needs no dependency. Regex
   waits for the `regex` crate decision and a measurement of compiling
   the patterns on every key press (the hook re-reads the config each
   time).

## Alternatives considered

- **Prefix in `key` (`glob:g*`, `re:^k`).** No new field, but a key that
  already starts with `glob:` would change meaning, and the prefix is
  easy to miss when reading a config.
- **Separate `glob` / `regex` fields instead of `key`.** Clear, but every
  consumer of `key` (list, which, doctor, bake mode) would have to learn
  a second and third name for the same role.
- **Config order only.** Simpler to explain, but would drop the rule
  that an exact key always wins, which users rely on to carve special
  cases out of `{number}` rules.
- **Full glob syntax (`[abc]`, `**`, braces).** Tokens have no path
  structure, and every extra construct has to be reproduced in the bash
  bake dispatcher; `*` and `?` cover the requests in #19.

## Consequences

- A broad glob such as `g*` captures every token starting with `g` that
  no exact or `{number}` rule claims, including real commands like
  `grep`. The config reference says so next to the example.
- `runex add` gains no `--match` flag in this change; glob rules are
  written by hand.
- `runex doctor --strict` accepts `match` as a known field.
