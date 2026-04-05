# Contributing to runex

## Development

### Prerequisites

- Rust (stable)
- PowerShell 7+ (`pwsh`) — required for the pwsh integration tests; tests are skipped at runtime if `pwsh` is not found
- bash 4+ — required for the bash integration tests; tests are skipped at runtime if bash < 4.0 is found. macOS ships bash 3.2; install a newer version via Homebrew (`brew install bash`)

### Build

```bash
cargo build
```

### Test

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p runex-core
cargo test -p runex
```

Some tests are skipped at runtime when their prerequisites are missing (e.g. `pwsh` for PowerShell tests, bash 4+ for bash tests).

#### Linux-specific tests (WSL)

A small number of tests exercise UNIX-only behaviour (named pipes, `/dev/zero`, `mkfifo`). These are compiled only on Unix and require a Linux environment. On Windows, run them via WSL:

```bash
wsl -e bash -c 'cd /mnt/path/to/runex && cargo test --workspace'
```

Replace `/mnt/path/to/runex` with the WSL path to your checkout. The tests are gated with `#[cfg(unix)]` and are automatically skipped on Windows.

## Coding guidelines

### Language and style

- Source code, comments, doc comments (`///`), and commit messages are written in **English**.
- Use `///` doc comments for public-facing items and for `fn` declarations inside `#[cfg(test)]` blocks when the *why* is non-obvious from the name alone.
- Avoid `//` inline comments inside function bodies. If an explanation is needed, move it to a `///` docstring, extract a named helper function, or restructure the code so the intent is clear without prose.
- State the *why*, not the *what* — never restate what the code already says.
- Keep functions small and single-purpose. Prefer flat code over deep nesting.
- Do not add error handling, fallbacks, or validation for scenarios that cannot occur. Trust internal invariants; validate only at system boundaries (user input, external processes, file I/O).

### Test discipline (TDD)

- Write a failing test first, confirm it is red, then write the minimal code to make it green.
- Tests are organised into nested `mod` blocks inside `#[cfg(test)]`, grouped by theme:

  ```rust
  mod parsing { use super::*; /* ... */ }
  mod sanitization { use super::*; /* ... */ }
  ```

- Helper functions (`test_config`, `abbr`, …) live at the `mod tests` level so all sub-mods can access them via `use super::*`.
- Each test function tests exactly one behaviour. Name it after what it asserts, not how it does it (`read_rc_content_returns_empty_for_oversized_file`, not `test_size_limit`).
- Do not mock subsystems that can be exercised cheaply (filesystem via `tempfile`, subprocess via a fake binary). Integration-level tests that touch real syscalls are preferred over unit tests with mocks.

### Functional programming

Keep business logic pure — no I/O, no global state, no side effects.

- **Pure functions first.** New logic should be pure by default: given the same inputs, always return the same output. If a function needs to query the environment (filesystem, PATH, processes), that dependency should be injected, not called directly.
- **Push I/O to the boundary.** Parsing, validation, expansion, and formatting are pure. I/O (file reads, subprocess calls, terminal output) belongs in the outermost layer. When adding a feature, write the logic as a pure function first, then wire up I/O in the caller.
- **Inject dependencies as closures.** Use `Fn` trait bounds (e.g. `command_exists: impl Fn(&str) -> bool`) to pass in environment-querying behaviour. This keeps the function pure and makes it testable with a trivial closure.
- **Prefer iterators over mutation.** `.map()`, `.filter()`, `.partition()`, `.flat_map()`, `.collect()` are idiomatic. Avoid mutating values in-place.
- **Use `Result` and `Option` idiomatically.** Propagate errors with `?`. Convert between them with `.ok()`, `.map_err()`, `and_then`. Do not panic on recoverable conditions.

### Architecture

The workspace is split into two crates with a deliberate boundary:

- **Core crate** — pure business logic: config parsing, expansion, diagnostics, shell script generation, sanitisation. No subprocess calls, no terminal output, no global state.
- **CLI crate** — side effects: argument parsing, file I/O, subprocess execution, terminal output. Calls into the core crate for all logic.

The rule: if new code does not need to spawn a process or write to stdout, it belongs in the core crate. Formatting helpers are an exception — they live in the CLI crate but remain pure (data in, string out, no printing).

**Dependency injection at the boundary.** Environment-querying closures (command existence checks, PATH resolution) are constructed once in the CLI layer from user-supplied flags, then passed down into core functions. Core functions never reach into the environment themselves.

**Testability follows from the architecture.** A function that accepts an injected closure can be tested without touching the filesystem or PATH. Design for this — it is not an afterthought.

### Security

Any value that originates from user-controlled data (config fields, command names, file paths) and is later rendered to the terminal or embedded in a shell string must be sanitised before use.

**Terminal output** — strip unsafe characters (ASCII control characters, Unicode visual-deception characters such as RLO, BOM, and zero-width spaces) before including user-controlled values in any human-readable output. Use the sanitisation utilities in the core crate.

**Shell string embedding** — use the quoting helpers provided in the core crate. Never interpolate raw user data into a shell string literal.

**Config validation** — new config fields must follow the same rules as existing ones: reject control characters, deceptive Unicode, and enforce a byte-length limit. Field limits are documented in `docs/config-reference.md`.

**Subprocess output** — any new subprocess call must cap both the total output size and the wall-clock execution time. Use the existing helpers; do not call `Command::output()` directly.

## Releasing

### Merge develop → main

Version bumps and publishing happen on `main`. Before bumping, merge `develop`:

```bash
git checkout main
git merge develop
```

### Version bump

Update the version in 3 places:

1. `runex-core/Cargo.toml` — `version`
2. `runex/Cargo.toml` — `version`
3. `runex/Cargo.toml` — `runex-core = { version = "..." }` dependency

All 3 must match. Run `cargo test` after bumping to verify the workspace builds cleanly.

### Versioning policy

- `0.x.y` — current phase; no stability guarantees
- Bump patch (`0.1.x`) for bug fixes, docs, and additive features
- Bump minor (`0.x.0`) for breaking changes to CLI interface or config schema

### Publishing

```bash
# Embed the current commit hash for cargo install users
RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex-core
RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex
```

`runex-core` must be published before `runex`.
