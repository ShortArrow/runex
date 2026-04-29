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

### Shell templates: what stays in the shell

The `runex hook` migration moved every per-keystroke decision into Rust.
The shell-side templates in `runex-core/src/templates/*.{sh,zsh,ps1,lua,nu}`
are now thin wrappers (244 lines total across five shells) that read
buffer state, call `runex hook`, and apply the eval-able output. When
deciding whether new logic belongs in Rust or in a template, ask
whether the work *requires* state only the live shell process holds:

- **Stays in the shell** — anything that touches the live readline
  buffer (`READLINE_LINE`, `LBUFFER/RBUFFER`, `commandline`, clink's
  `rl_buffer`, PSReadLine `Replace`/`SetCursorPosition`); anything
  that introspects the shell's internal state at runtime (PSReadLine
  `_queuedKeys` reflection for paste detection); pre-RPC sanitisers
  that exist specifically to *avoid* spawning a Rust subprocess
  (clink's `runex_is_safe_line` rejects control characters before the
  cmd.exe roundtrip).
- **Belongs in Rust** — everything else. Token extraction,
  command-position detection, cursor placeholder substitution, shell
  escaping, output formatting, command-existence checks. New rules
  that don't depend on live buffer state should be added to
  `runex-core` and exposed through `runex hook`'s output.

The remaining shell code is small, stable, and self-justifying. Avoid
rewriting it for the sake of consistency — read the comment at the top
of each template (e.g. `templates/pwsh.ps1` explains why paste
detection lives in pwsh; `templates/clink.lua` explains why the line
safety regex stays in lua) and trust the existing rationale unless
there's a concrete observation suggesting otherwise.

### Security

Any value that originates from user-controlled data (config fields, command names, file paths) and is later rendered to the terminal or embedded in a shell string must be sanitised before use.

**Terminal output** — strip unsafe characters (ASCII control characters, Unicode visual-deception characters such as RLO, BOM, and zero-width spaces) before including user-controlled values in any human-readable output. Use the sanitisation utilities in the core crate.

**Shell string embedding** — use the quoting helpers provided in the core crate. Never interpolate raw user data into a shell string literal.

**Config validation** — new config fields must follow the same rules as existing ones: reject control characters, deceptive Unicode, and enforce a byte-length limit. Field limits are documented in `docs/config-reference.md`.

**Subprocess output** — any new subprocess call must cap both the total output size and the wall-clock execution time. Use the existing helpers; do not call `Command::output()` directly.

## Releasing

A release ships runex through five channels — GitHub Releases (tag-driven
binaries), crates.io, AUR `runex-bin`, the Homebrew tap, and winget. The
big risk is that *any* of these going stale leaves users running the old
binary while everything else looks fine. Follow the checklist in order.

### Versioning policy

- `0.x.y` — current phase; no stability guarantees.
- Bump **patch** (`0.1.x`) for bug fixes, docs, additive features.
- Bump **minor** (`0.x.0`) for breaking changes to the CLI surface or
  config schema. (`runex-core` is treated as an internal API; library
  callers should pin exact versions.)

### Pre-flight

Before touching any version number:

- [ ] **CHANGELOG `[Unreleased]` is complete.** Every notable change
  since the last tag has an entry under Added / Changed / Deprecated /
  Removed / Fixed / Security. Skim `git log v<previous>..HEAD --oneline`
  and reconcile.
- [ ] **`cargo test --workspace` is green on Windows.** This is the
  baseline; nothing else matters if this fails.
- [ ] **`cargo test --workspace` is green on Linux.** Run via
  `wsl -d archlinux -e bash -lc 'cd /path/to/runex && cargo test --workspace'`
  (a few `#[cfg(unix)]` tests don't compile on Windows).
- [ ] **`develop` is in sync with `origin/develop`.** No unpushed
  commits, no dangling working-tree changes.
- [ ] **A clean `runex doctor` run on a real machine.** Catches
  integration drift that unit tests don't see (e.g. clink lua
  outdated). Especially important if the release touches shell
  templates.

### Cut the release

All commands from the repo root.

- [ ] **Merge develop → main**:

  ```bash
  git checkout main
  git pull
  git merge --no-ff develop
  ```

- [ ] **Bump the version in three places.** `runex-core/Cargo.toml`
  `version`, `runex/Cargo.toml` `version`, and the
  `runex-core = { version = "..." }` dependency line in
  `runex/Cargo.toml`. All three must match.

- [ ] **Promote the CHANGELOG `[Unreleased]` heading** to
  `## [X.Y.Z] - YYYY-MM-DD`, then add a fresh empty `## [Unreleased]`
  block above it for next time.

- [ ] **Refresh `Cargo.lock`** with `cargo check`.

- [ ] **Single bump commit:**

  ```bash
  git add runex-core/Cargo.toml runex/Cargo.toml Cargo.lock CHANGELOG.md
  git commit -m "chore: bump version to X.Y.Z"
  git push origin main
  ```

- [ ] **Tag and push** to trigger the binary build workflow:

  ```bash
  git tag -a vX.Y.Z -m "Release vX.Y.Z"
  git push origin vX.Y.Z
  ```

  `.github/workflows/release.yml` runs and takes ~10 minutes to
  build every target platform (see "Binary release workflow" below)
  and attach archives to the auto-created GitHub release.

- [ ] **Wait for the workflow to finish.** The next steps need the
  release artifacts (sha256 inputs for AUR and Homebrew). Watch
  https://github.com/ShortArrow/runex/actions.

### Publish to package registries

Run **after** the GitHub release artifacts are visible at
https://github.com/ShortArrow/runex/releases/tag/vX.Y.Z.

- [ ] **crates.io.** `runex-core` must publish before `runex` because
  the latter depends on it:

  ```bash
  RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex-core
  RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex
  ```

- [ ] **AUR `runex-bin`.** Use the helper:

  ```bash
  packaging/aur-bin/release.sh X.Y.Z ~/aur/runex-bin
  ```

  The script fetches the Linux x86_64/aarch64 release tarball
  sha256s, rewrites the PKGBUILD, regenerates `.SRCINFO`, and stages
  a commit in your AUR clone. Review with `git show`, then push:

  ```bash
  cd ~/aur/runex-bin
  GIT_SSH_COMMAND='ssh -i ~/.ssh/aur' git push origin master
  ```

  Forgetting this step is the canonical way users end up with a
  stale `runex` binary on their `PATH`. The shell template safe-fails
  to "literal space" when `runex hook` errors out as an unknown
  subcommand, and it's hard to debug from the user side. Don't skip.

- [ ] **Homebrew tap.** Use the helper:

  ```bash
  packaging/homebrew/release.sh X.Y.Z /v/homebrew-runex
  ```

  The script fetches the macOS arm64/x86_64 and Linux arm64/x86_64
  tarball sha256s, rewrites `Formula/runex.rb`, and stages a commit
  in the tap clone. Push manually:

  ```bash
  cd /v/homebrew-runex
  git push origin main
  ```

- [ ] **winget-pkgs PR.** See `### winget submission` below — the
  Defender ML pipeline can reject runex on submission and the
  recovery procedure deserves its own subsection.

### Post-release

- [ ] **Merge main → develop with `--no-ff`** so the bump commit
  shows up in develop's history with a clear merge marker:

  ```bash
  git checkout develop
  git pull
  git merge --no-ff main
  git push origin develop
  ```

- [ ] **Polish the GitHub release body.** The auto-generated body
  is bare. Fill it in using the template in `### GitHub release body`
  below — at minimum, a summary, install commands, and an
  upgrade-notice line for clink users when shell templates changed.

- [ ] **Verify each install channel resolves the new version.**
  Don't trust the publish steps to have succeeded — check:

  ```bash
  cargo search runex                      # crates.io
  pacman -Si runex-bin 2>/dev/null        # AUR
  brew info shortarrow/runex/runex        # Homebrew tap
  winget show ShortArrow.runex            # winget (after the PR merges)
  ```

### winget submission

winget validation runs the candidate manifest through Defender. A non-zero
fraction of past PRs have been blocked by ML detections like
`Trojan:Win32/Sprisky.U!cl` or `Trojan:Script/Wacatac.H!ml`. The pattern:

- `!cl` (cloud ML) — usually clears within 24-72 hours as the model
  updates. Comment on the PR asking maintainers to retry validation.
- `!ml` (local ML) — needs a false-positive submission to
  https://www.microsoft.com/en-us/wdsi/filesubmission. Pick
  "Software developer" and explain the binary is dual-licensed open-source
  Rust code with reproducible builds via GitHub Actions. Include a link
  to the failing winget PR.

Submission steps:

1. **Generate a manifest update** with `wingetcreate update` against the
   previous PR's branch, pointing at the new x86_64-pc-windows-msvc zip
   from the GitHub release.
2. **Open a PR against `microsoft/winget-pkgs`** with the regenerated
   manifest. Title format: `New version: ShortArrow.runex version X.Y.Z`.
3. **Watch the validation pipeline.** Status is reported as PR comments
   from `@microsoft-github-policy-service` and tags like `Validation-Defender-Error`.
4. **If Defender rejects:** post the WDSI submission ID on the PR, ask
   for revalidation after the analyst clears the file.
5. **If validation hangs:** the validation pipeline sometimes uses stale
   Defender definitions; retry by closing/reopening the PR or pushing
   an empty commit to the branch.

Until the PR merges, point users at `cargo install runex` or
`brew install shortarrow/runex/runex` as the fastest install path.

### GitHub release body

Use this template for the release body, filled in from the CHANGELOG:

```markdown
## Highlights

- [3-5 bullet points of user-visible changes from CHANGELOG]

## Install

| Channel | Command |
|---------|---------|
| crates.io | `cargo install runex` |
| AUR | `paru -S runex-bin` |
| Homebrew tap | `brew install shortarrow/runex/runex` |
| winget | `winget install ShortArrow.runex` |

(AUR / Homebrew / winget can lag the GitHub release by hours;
`cargo install runex` always picks up the new version immediately.)

## Upgrade notes

[Conditional: only when shell templates changed]
- **bash / zsh / pwsh / nu users:** the integration line in your rcfile
  re-evaluates the export at every shell start; just open a fresh shell.
- **clink users:** the lua file at `%LOCALAPPDATA%\clink\runex.lua` is
  a static copy and does not auto-refresh. Run
  `runex export clink > %LOCALAPPDATA%\clink\runex.lua` (or
  `runex doctor` to confirm whether refresh is needed) and open a new
  cmd window.

[Conditional: when CLI surface or config schema changed]
- **Breaking changes:** [list]

## Full changelog

See [CHANGELOG.md](https://github.com/ShortArrow/runex/blob/vX.Y.Z/CHANGELOG.md#XYZ---YYYY-MM-DD).
```

Skip sections that don't apply. The "AUR / Homebrew / winget can lag"
line should stay even when those channels caught up — it sets
expectations for users hitting the page on day-of.

### Branch workflow reference

The branching model assumed by the checklist above:

```
develop  : feature work, bug fixes, docs
main     : release-only; bump commits and merges from develop
develop  : merge main back with --no-ff so version bumps show up
```

The `--no-ff` on the back-merge is deliberate: it preserves the merge
commit so the branch history stays clear about which commits came from
main (version bumps) vs develop (feature work).

### Binary release workflow

Pushing a `v*` tag triggers `.github/workflows/release.yml`. The
workflow runs `cargo test --workspace --locked` on Ubuntu, Windows,
and macOS as a hard gate — every native target must pass before any
binary is built or published. This closes the timing hole where a
tag push would otherwise race the bump commit's CI workflow and
could ship binaries from an unverified commit.

Once the test gate passes, the workflow builds for every supported
platform and attaches archives to the auto-created GitHub release.

| Target                         | OS runner       | Archive |
|--------------------------------|-----------------|---------|
| x86_64-pc-windows-msvc         | windows-latest  | zip     |
| x86_64-unknown-linux-gnu       | ubuntu-latest   | tar.gz  |
| aarch64-unknown-linux-gnu      | ubuntu-latest   | tar.gz  |
| x86_64-apple-darwin            | macos-latest    | tar.gz  |
| aarch64-apple-darwin           | macos-latest    | tar.gz  |
| aarch64-linux-android (Termux) | ubuntu-latest   | tar.gz  |

Workflow hardening:

- Top-level `permissions: contents: read`. Only the `release` job gets
  `contents: write` to publish the release; build jobs cannot mutate
  the repo.
- `actions/checkout` on build jobs uses `persist-credentials: false`
  so the checkout token is not left on disk for malicious build code
  to exfiltrate.
- All third-party actions are pinned to commit SHAs.
- Only `GITHUB_TOKEN` is used — no external secrets, no automatic
  `cargo publish`.
