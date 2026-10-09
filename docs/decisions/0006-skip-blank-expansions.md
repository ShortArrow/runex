# ADR 0006: A rule that renders blank is skipped, not rejected

- **Status**: Accepted
- **Phase**: 0.1.21 candidate
- **Issue**: #52
- **Authors**: ShortArrow, with Claude Code collaboration on the
  alternatives analysis
- **Date**: 2026-10-09

---

## Context

`expand = ""` and a whitespace-only `expand` are rejected at load time,
because firing such a rule would silently delete the typed token. An
`expand` that is only the cursor placeholder, `"{}"`, or `" {} "`,
passes that check: it is not empty as written. Once `{}` is removed it
renders to nothing (or to spaces), and in 0.1.20 `runex hook`
replaced the token with that on every shell it serves. The Git Bash
bake path instead took an empty rendering for no match and inserted a
space.

Glob and regex rules already skipped a rendering that was empty, but
not one that was whitespace. Exact and `{number}` rules never skipped
it.

## Decision

A rule whose rendering is empty or whitespace once `{}` is removed
(Rust `str::trim`) is skipped, in every phase: exact, `{number}`, glob
and regex. Skipping works like a failed `when_command_exists`: the
next rule with a matching key is tried, and when none fires the
trigger key inserts its plain space. `runex which --why` reports the
rule as `blank_expansion`, and `runex doctor` warns about a rule that
renders blank for every token (`abbr[N].blank_expand`).

On the Git Bash bake path, exact and `{number}` rules that always
render blank are left out of the tables at export time; a glob
rendering is tested at key press time against the same set of
whitespace characters Rust uses.

## Alternatives

**Keep firing the rule.** It is the 0.1.20 behaviour, and a user
could use it to delete a word. Rejected: it is the effect the load-time
check on empty `expand` exists to prevent, reached through a spelling
that check does not see. Deleting a word is what the shell's own
editing keys are for.

**Reject the rule at load time.** It would match how an empty `expand`
is treated. Rejected because config validation fails the whole file:
a 0.1.20 config with one such rule would stop expanding everything on
every shell, which is worse than the rule never firing. `runex doctor`
reports the rule instead.

## Consequences

A config that relied on `{}` to erase a token now leaves the token in
place. The CHANGELOG lists this under Changed.
