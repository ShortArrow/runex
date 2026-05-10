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

# (`runex` is the only crate in the workspace as of 0.1.14; the
# previous `runex-core` crate was absorbed into `runex/src/{domain,
# app, infra}/` and removed from the workspace member list.)
cargo test -p runex
```

Some tests are skipped at runtime when their prerequisites are missing (e.g. `pwsh` for PowerShell tests, bash 4+ for bash tests).

#### Linux-specific tests (WSL)

A small number of tests exercise UNIX-only behaviour (named pipes, `/dev/zero`, `mkfifo`). These are compiled only on Unix and require a Linux environment. On Windows, run them via WSL:

```bash
wsl -e bash -c 'cd /mnt/path/to/runex && cargo test --workspace'
```

Replace `/mnt/path/to/runex` with the WSL path to your checkout. The tests are gated with `#[cfg(unix)]` and are automatically skipped on Windows.

#### Linux-specific tests (Container, recommended)

Phase H (0.1.15+) ships a Linux CI container image with bash 4+, zsh, pwsh, nu, xclip, wl-paste, xsel, and the rust toolchain pinned at known versions. CI's `test-linux` job runs inside this image; you can run exactly the same environment locally:

```bash
# Pull the latest CI image and run the full Linux test suite
# against your working tree. The bind-mount on /workspace makes
# target/ and Cargo.lock changes show up in your checkout.
docker run --rm -it \
  -v "$(pwd)":/workspace -w /workspace \
  --user 1001 \
  ghcr.io/shortarrow/runex-ci:latest \
  cargo test --locked --workspace
```

Or build the image locally if you've changed the Dockerfile. Note
the build context is `containers/ci`, not the repo root — the
`.dockerignore` next to the Dockerfile only applies when the
context is scoped that way:

```bash
docker build -t runex-ci -f containers/ci/ubuntu.Dockerfile containers/ci
docker run --rm -it -v "$(pwd)":/workspace -w /workspace --user 1001 \
  runex-ci cargo test --locked --workspace
```

The container is the same one CI uses, so a green `cargo test --workspace` here is the strongest pre-push signal short of pushing to a feature branch. PTY-based E2E tests (`bash_pty_integration.rs`, `zsh_pty_integration.rs`, `pwsh_pty_integration.rs`, `nu_pty_integration.rs`) and the Phase G shell-integration cache tests (`shell_integration.rs`) all exercise the same toolchain inside the container as in CI.

The image is amd64-only as of Phase H. macOS / Windows hosts can still use it via Docker Desktop's built-in emulation but PTY tests may be slow under emulation; running on a native Linux host (or WSL2) is preferred for development.

#### Bumping the pinned image digest in `ci.yml`

CI consumes the image by `@sha256:...` digest, not by the `:latest`
tag. After changing anything under `containers/ci/`, the new digest
must land in `.github/workflows/ci.yml` for CI to pick the change up.

1. **Trigger a build.** Push your `containers/ci/**` change to a
   branch. The `Build CI image` workflow runs on push to
   `main` / `develop`; for feature branches, open a PR (the
   `pull_request` trigger runs the build with `push: false` so you
   can verify the Dockerfile compiles without yet uploading to
   GHCR). Once the PR merges to `develop`, the push trigger
   uploads the image.
2. **Copy the digest.** Open the green `Build CI image` run on
   `develop` (or trigger one manually with
   `gh workflow run build-ci-image.yml`). The run's **Step
   Summary** prints a "Pin in `.github/workflows/ci.yml`:" block
   containing the line you need.
3. **Update `ci.yml`.** Replace the `image:` value under
   `test-linux.container` with the new
   `ghcr.io/shortarrow/runex-ci@sha256:<new>`. Commit on
   `develop` with a message like
   `ci(linux): bump pinned runex-ci digest to <reason>`.
4. **Push and verify.** The next CI run should consume the new
   image. Mismatch between Dockerfile changes and the pinned
   digest will manifest as missing-tool failures in `Verify image
   tooling` or behavioural drift in `cargo test --locked`.

The two-step (build → pin) is deliberate. A single-step approach
where CI tracks `:latest` would let a re-built image silently
change every running PR's gate. The digest pin makes a re-build
visible as a one-line commit reviewable like any other change.

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
The shell-side templates in `runex/src/domain/templates/*.{sh,zsh,ps1,lua,nu}`
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
  that don't depend on live buffer state should be added under
  `runex/src/domain/` (or `app/`, depending on whether they're pure)
  and exposed through `runex hook`'s output.

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
  config schema. The `runex` crate ships only a binary (`[[bin]]`),
  not a library — every internal symbol is `pub(crate)`, so there is
  no library API to break. External callers should embed the bin
  rather than depending on internal types.

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
- [ ] **crates.io Trusted Publisher is registered for `runex`.**
  Sanity check at https://crates.io/crates/runex/settings/ — the
  crate needs a GitHub Actions trusted publisher entry pointing at
  this repo + `release.yml`. One-time setup, but worth re-confirming
  if you haven't released in a while (crates.io occasionally
  invalidates unused tokens). The legacy `runex-core` Trusted
  Publisher entry can stay registered (harmless; the crate publishes
  nothing as of 0.1.14). See `### crates.io (OIDC Trusted
  Publishing)` below for the exact field values.

### Cut the release

All commands from the repo root.

- [ ] **Merge develop → main**:

  ```bash
  git checkout main
  git pull
  git merge --no-ff develop
  ```

- [ ] **Bump the version.** Edit `runex/Cargo.toml` `version` to
  the new value. (As of 0.1.14 there is only one crate; the
  pre-0.1.14 three-place bump across `runex-core/Cargo.toml`,
  `runex/Cargo.toml`, and the `runex-core` path-dep line is no
  longer needed.)

- [ ] **Promote the CHANGELOG `[Unreleased]` heading** to
  `## [X.Y.Z] - YYYY-MM-DD`, then add a fresh empty `## [Unreleased]`
  block above it for next time.

- [ ] **Refresh `Cargo.lock`** with `cargo check`.

- [ ] **Single bump commit:**

  ```bash
  git add runex/Cargo.toml Cargo.lock CHANGELOG.md
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

- [ ] **crates.io.** Automated. The `publish-crates` job in
  `release.yml` fires on the same tag push as `build` / `release` and
  publishes `runex` via OIDC Trusted Publishing. (Pre-0.1.14 it
  also published `runex-core`, but that crate was absorbed into
  `runex` in 0.1.14 and no longer exists in the workspace.) See
  `### crates.io (OIDC Trusted Publishing)` below for the one-time
  setup.

  - To **skip** publishing on a particular tag (e.g. when re-tagging
    a previously-published version because of a release-process
    glitch), include `[skip publish]` in the bump commit's message.
    The job's `if:` condition checks for it.
  - There is **no `CARGO_REGISTRY_TOKEN` to manage** — the workflow
    exchanges a short-lived GitHub OIDC token for an equally
    short-lived crates.io token at job start, scoped to the
    workflow + repository registered with crates.io.

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

### crates.io (OIDC Trusted Publishing)

`runex` is published to crates.io via OIDC Trusted Publishing — no
long-lived `CARGO_REGISTRY_TOKEN` is stored as a GitHub secret or in
any local environment. The `publish-crates` job in
`.github/workflows/release.yml` exchanges the workflow's GitHub OIDC
token for a short-lived crates.io token at job start
(`rust-lang/crates-io-auth-action@v1.0.4`), runs `cargo publish
--dry-run` as a packaging sanity check, then publishes the real
crate.

(Pre-0.1.14 the same workflow also published `runex-core` first and
waited for the sparse index to propagate before `runex`. Phase C
absorbed `runex-core` into `runex`, so the workflow now publishes a
single crate. The legacy `runex-core` Trusted Publisher entry on
crates.io stays registered — harmless because the workflow no longer
calls `cargo publish -p runex-core`.)

#### One-time setup

Register this repository's `release.yml` as a Trusted Publisher on
crates.io for the `runex` crate:

1. Sign in at https://crates.io as a crate owner.
2. Go to https://crates.io/crates/runex/settings.
3. Under **Trusted Publishers**, click **Add** and choose
   **GitHub Actions**.
4. Fill in:
   - **Repository owner**: `ShortArrow`
   - **Repository name**: `runex`
   - **Workflow filename**: `release.yml`
   - **Environment**: leave blank (we don't gate publishing behind a
     GitHub Environment for runex).
5. Save.

After saving, the next tag-pushed release will publish without any
further manual action.

#### Sanity check before relying on it

If the `publish-crates` job has been failing at the
`Exchange OIDC token` step or the `Publish runex` step, the usual
cause is one of:

- **Trusted Publisher not registered** for `runex` yet. Re-run the
  setup above.
- **Workflow filename mismatch.** If you renamed the workflow file
  the registered entry no longer matches and crates.io won't issue a
  token. Update the entry to the new filename.
- **Repository was renamed/transferred.** Update the
  `Repository owner` / `Repository name` fields to the current ones.

If you ever need to publish manually as a fallback (e.g. crates.io
OIDC issuer outage), you can still run
`RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex`
from a checkout at the release tag, using a personal API token via
`cargo login --registry crates-io`. **Don't commit the token
anywhere; revoke it from your crates.io account once the manual run
is complete.**

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

Pre-submission checks (the PR template asks for these — do them
*before* opening the PR):

- [ ] **wingetcreate is current** — `wingetcreate --version` should
  match a recent release (1.12+ at time of writing). Older
  wingetcreate defaults to schema 1.10, which `winget-pkgs` master
  has moved past; check with
  `winget upgrade Microsoft.WingetCreate`.
- [ ] **Schema matches the previously-merged version's schema.**
  Inspect the existing manifest in
  `microsoft/winget-pkgs:manifests/s/ShortArrow/runex/<previous>/`
  for its `ManifestVersion`, and confirm the regenerated manifest
  uses the same or newer. The 0.1.11 PR landed at 1.12.0 — a
  regression to 1.10.0 will get pushed back.
- [ ] **No other open PRs for the same manifest:**
  `gh search prs --repo microsoft/winget-pkgs "is:pr is:open ShortArrow.runex"`.
- [ ] **Manifest validates locally:**
  `winget validate --manifest <path>`.
- [ ] **Local install attempt** with
  `winget install --manifest <path> --accept-source-agreements --accept-package-agreements`.
  *Be aware:* on Defender-active machines this may stall at the
  "applying motw" step due to the same false-positive that hits
  the official validation pipeline. A successful local install is
  nice-to-have but not required — the PR still gets the same
  Defender treatment regardless.

Submission steps:

1. **Generate the manifest update** with
   `wingetcreate update ShortArrow.runex --version X.Y.Z --urls <release-zip-url>`,
   passing `--out <dir>` so the files land somewhere obvious. The
   tool produces three YAMLs:
   `ShortArrow.runex.{installer,locale.en-US,version}.yaml`.
2. **Open a PR against `microsoft/winget-pkgs`** by adding `--submit`
   to the same `wingetcreate update` invocation, or by manually
   committing on a `winget-pkgs` fork and opening the PR with `gh pr
   create`. Title format: `New version: ShortArrow.runex version X.Y.Z`.
3. **Post the checklist confirmation as a PR comment** so reviewers
   don't have to verify each box themselves. Mention which boxes
   are blocked by Defender (the local-install row).
4. **Watch the validation pipeline.** Status is reported as PR
   comments from `@microsoft-github-policy-service` and tags like
   `Validation-Defender-Error`.
5. **If Defender rejects:** post the WDSI submission ID on the PR,
   ask for revalidation after the analyst clears the file.
6. **If validation hangs:** the validation pipeline sometimes uses
   stale Defender definitions; retry by closing/reopening the PR or
   pushing an empty commit to the branch.

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
