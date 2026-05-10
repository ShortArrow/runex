# ADR 0002: Containerized Linux CI (single GHCR image, digest pin)

- **Status**: Accepted
- **Phase**: H (0.1.15 candidate, develop branch)
- **Supersedes**: the implicit `apt-get install zsh` + reliance on
  `ubuntu-latest` having pwsh / nu for the `test-linux` job in
  0.1.13–0.1.14.
- **Authors**: ShortArrow, with Claude Code collaboration on
  alternatives analysis (and Codex-driven review).
- **Date**: 2026-05-10

---

## Context

Phase A–G left the test suite reproducible in the small but
non-reproducible in the large:

- The `test-linux` job on `ubuntu-latest` `apt install`-ed `zsh`
  inline and depended on `pwsh` / `nu` being on the runner image
  for the rest. Each GHA runner respin could shift bash to a new
  patch, swap `nu` between 0.105 and 0.111, or quietly remove
  `xclip`. We had no record of the tooling matrix the gate ran on.
- Local hand-checks on WSL Arch / Fedora / openSUSE showed
  diverging behaviour from CI for shell-integration and PTY tests
  (`expectrl` against bash 4 vs 5, `xclip` vs `wl-paste` vs `xsel`,
  `clipboard.rs` provider chain order). When CI broke, isolating
  whether the cause was runex, the runner image, or the developer's
  dotfiles cost real time.
- WSL spawn latency from inside a developer's terminal added ~3-5 s
  to every Linux-only test invocation, and `mise` shim drift
  (Phase G's root cause) demonstrated that PATH resolution alone
  could change behaviour between two ostensibly-identical machines.

The user's framing — "次のタスクは ci や e2e を container でも実行
可能にすること" — was scoped explicitly:

- **Linux only.** macOS / Windows stay on native runners; Apple
  Silicon containers and Server Core are not workable as a CI
  baseline.
- **Debian/Ubuntu LTS as the starting distro.** Future Arch /
  Fedora / openSUSE images come later (Phase H5, separate ADR).
- **`apt`-installed shell tooling.** Don't reach for non-distro
  package sources unless the upstream forces it (pwsh from MS apt,
  `nu` from GitHub releases — both pinned).
- **Use GitHub Actions' `jobs.<id>.container` field.** Don't roll
  a custom runner image or spin up a sidecar.

---

## Decision

Build one `ghcr.io/shortarrow/runex-ci:<tag>` image from
`containers/ci/ubuntu.Dockerfile`, push it from a dedicated
`build-ci-image.yml` workflow, and consume it from `ci.yml`'s
`test-linux` job by **manifest-list digest**. Hand-checks on
developer machines use the same image via
`docker run --user 1001 ...`.

Concretely:

1. **Single image, two consumers.** The same image runs CI and dev
   hand-checks. The only difference is the volume mount (`/workspace`
   bind-mounts the repo) and the host caches. There is no separate
   `dev.Dockerfile`.
2. **GHCR, public, OCI manifest-list.** `ghcr.io/shortarrow/runex-ci`
   pushed by `build-ci-image.yml`. `latest` tag for unversioned
   consumers (developer `docker run`); `sha-<git_sha>` tag for
   traceability back to the source commit; `@sha256:<index_digest>`
   pin in `ci.yml` so a re-built image cannot silently change what
   the gate runs against.
3. **Build-time pinning, not runtime.** Every floating input is
   either a `Dockerfile` ARG (`NU_VERSION=0.112.2`,
   `RUST_TOOLCHAIN=stable`, `NODE_MAJOR=20`) or a digest
   (`FROM ubuntu:24.04@sha256:...`). The pwsh apt feed, NodeSource
   apt repo, and rustup installer are validated by GPG / TLS at
   build time and the resulting binaries land in the layer cache.
   Bumping any of these is a one-line commit visible in `git log -p`.
4. **Sanity check in the build, not at CI runtime.**
   `containers/ci/sanity.sh` runs as the last `RUN` of the
   Dockerfile and asserts every tool the test suite reaches for
   (bash 4+, zsh, pwsh, nu, xclip, wl-paste, xsel, git, curl, node,
   cargo, rustc) is present and prints its version. A missing tool
   fails `docker build`, not `cargo test`.
5. **Non-root user, fixed UID.** `useradd --uid 1001 runex` matches
   the `--user 1001` GitHub Actions passes when a container is
   declared on a `ubuntu-latest` runner. `RUSTUP_HOME` and
   `CARGO_HOME` are pinned to absolute paths under `/home/runex` so
   they remain discoverable even when the consumer overrides
   `$HOME` (which GHA does — `HOME=/github/home`).
6. **PR-trigger build-only path.** `build-ci-image.yml` runs on
   `pull_request` paths as a build-only check (`push: false`,
   GHCR login skipped). A broken Dockerfile fails CI before it
   can land on `develop` or `main`.

---

## Considered alternatives

### A. Stay on `ubuntu-latest`, just `apt install` more in `ci.yml`

The path of least resistance. We were already doing this for `zsh`.

Rejected:
- Doesn't pin upstream tool versions. A `nu` minor bump on the
  runner image silently changes test behaviour.
- Doesn't help dev hand-checks at all; the runner image is opaque
  to developers.
- Each `ci.yml` step incurs network and apt-cache cost on every
  CI run, even though the tools have not changed.

### B. Use a multi-stage matrix of distros from day one

Spin up `ubuntu`, `arch`, `fedora`, `opensuse` Dockerfiles in
parallel right away and matrix `ci.yml` over them.

Rejected for now:
- Triples the surface area on the very first roll-out, before we
  know whether a containerized Linux gate even works for our PTY
  tests.
- The package-name and repo-source differences across distros
  warrant their own per-distro design discussion. Land Phase H on
  Ubuntu first; Phase H5 (separate ADR) adds the matrix.
- Single-distro is enough to get the reproducibility win. The
  matrix is a follow-on, not a prerequisite.

### C. Track latest by tag in `ci.yml` (no digest pin)

Use `image: ghcr.io/shortarrow/runex-ci:latest` and let the rebuild
cycle in `build-ci-image.yml` propagate.

Rejected:
- Defeats the reproducibility goal. A re-built image is a silent
  behavioural change to every running PR. We already saw the
  failure mode where a CI run kicks off before the image push
  completes, leaves the consumer pulling an unrelated image
  version, and the failure looks like a runex regression.
- The two-tag scheme (`:latest` for dev, `@sha256:...` for CI)
  splits the responsibility cleanly: developers get convenience,
  CI gets determinism.

### D. Reach for a managed CI service (BuildKite, CircleCI) for
container-native runners

Genuinely considered as a way to avoid GHA's HOME-override quirk.

Rejected:
- New auth surface, new billing surface, new YAML dialect. The
  GHA quirk is a one-line `ENV RUSTUP_HOME=/home/runex/.rustup`
  fix in the Dockerfile (and now ADR-documented), not a vendor
  switch.
- Loses the cross-cut with `release.yml` and `build-ci-image.yml`
  that share the same SHA-pinned-actions / `persist-credentials:
  false` posture.

---

## Implementation contract (load-bearing details)

These are the choices a future maintainer must not silently undo.

- **`FROM` line includes a digest.** Floating tags re-introduce
  the reproducibility hole. Re-resolve with
  `docker buildx imagetools inspect ubuntu:24.04` when bumping.
- **`ARG NU_VERSION=...` / `ARG RUST_TOOLCHAIN=...` / `ARG
  NODE_MAJOR=...` are explicit `=`-defaulted.** A bump must be a
  commit, not a workflow input override.
- **Sanity script runs as the last `RUN` of the Dockerfile.** Move
  it earlier and a missing tool only fails at CI runtime, after a
  push to GHCR.
- **`RUSTUP_HOME` / `CARGO_HOME` are `ENV` lines, not `ARG`.** The
  GHA HOME-override depends on these being set in the image's
  environment, not just at build time.
- **`build-ci-image.yml` `context: containers/ci`.** With
  `context: .` Docker reads the repo-root `.dockerignore` (absent),
  ships the entire workspace to buildx, and ignores the local
  `containers/ci/.dockerignore`. The local context keeps build
  cache hits stable across unrelated edits.
- **`ci.yml` test-linux pins by `@sha256:...` digest, not tag.**
  See alternative C above.
- **Both workflows use `persist-credentials: false` on
  `actions/checkout`.** Matches `release.yml`. The `GITHUB_TOKEN`
  must not linger on disk for build / test code to read.
- **`build-ci-image.yml` `pull_request` trigger uses `push: false`
  and skips GHCR login.** Fork PRs lack `packages: write` and we
  do not want untrusted code pushing to the registry image.

---

## Bump procedure (image digest update)

1. Edit `containers/ci/ubuntu.Dockerfile` and / or its sanity
   script. Commit on `develop` (or a feature branch). The path
   filter triggers `build-ci-image.yml` on push to `develop` /
   `main`.
2. Open the resulting workflow run. Its **Step Summary** prints
   the image's `@sha256:...` digest.
3. Commit a one-line edit to `.github/workflows/ci.yml` replacing
   the `image:` digest under `test-linux.container`. Commit
   message format: `ci(linux): bump pinned runex-ci digest to ...`.
4. Push. The next CI run uses the new image; any regression in
   the bump is bisectable to that single commit.

For PR validation of a Dockerfile change, the `pull_request`
trigger on `build-ci-image.yml` runs the build with
`push: false` — a green PR build is sufficient evidence that
the Dockerfile compiles and the sanity script passes.

---

## Out of scope (explicitly deferred)

- **arch / fedora / opensuse Dockerfiles** (Phase H5, separate ADR).
- **arm64 image build.** linux/amd64 only for now.
- **macOS / Windows containerization.** Apple Silicon / Server
  Core are not workable as CI baselines.
- **Pinning rust to a specific minor (e.g. `1.83.0`) via
  `rust-toolchain.toml`.** `RUST_TOOLCHAIN=stable` matches
  `dtolnay/rust-toolchain@stable` on the native runners; tightening
  this would also force the macOS / Windows jobs to drop the
  `dtolnay` action, which is its own design discussion.
- **Caching `target/` inside the image.** `Swatinem/rust-cache@v2`
  on the consumer side already covers this and stays
  branch / commit aware in a way an image-baked cache cannot.

---

## Verification

The Phase H roll-out validated the design end-to-end on `develop`:

- `docker build -f containers/ci/ubuntu.Dockerfile -t runex-ci-test
  containers/ci` finishes locally with the sanity script printing
  the full tool version banner.
- `cargo test --locked --workspace` inside the image (matching the
  CI invocation) on WSL Arch matches the green CI gate (run
  25623276155 on `develop`): all jobs (linux/macos/windows) pass.
- A simulated GHA HOME override (`docker run -e HOME=/github/home
  --user 1001 ...`) prints `cargo --version` cleanly, confirming
  the `RUSTUP_HOME` / `CARGO_HOME` pin works.
