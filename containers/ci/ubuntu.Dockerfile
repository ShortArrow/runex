# syntax=docker/dockerfile:1.7
#
# runex CI / dev container — Ubuntu LTS edition.
#
# Single image used by both `.github/workflows/ci.yml` (`test-linux` job)
# and developer hand-checks. Every shell tool the test suite reaches for
# (bash 4+, zsh, pwsh, nu, xclip, wl-paste) is pinned at image build time
# so a CI failure can be reproduced byte-for-byte locally with:
#
#   docker run --rm -v "$(pwd)":/workspace -w /workspace --user 1001 \
#     ghcr.io/shortarrow/runex-ci:latest cargo test --workspace
#
# The image build runs `containers/ci/sanity.sh` at the very end so a
# missing or broken tool is caught here, not at CI runtime.

FROM ubuntu:24.04

# ---- Build-time arguments ---------------------------------------------------
# Tool versions are explicit so a bump shows up in `git log -p`. Bumping
# `NU_VERSION` triggers an image rebuild via the path filter on
# `.github/workflows/build-ci-image.yml`.
ARG NU_VERSION=0.112.2
ARG TARGETARCH=amd64

# Don't prompt during apt installs.
ENV DEBIAN_FRONTEND=noninteractive
# Force UTF-8 so test output and shell quoting stay deterministic.
ENV LANG=C.UTF-8 LC_ALL=C.UTF-8

# ---- System packages --------------------------------------------------------
# Core toolchain + the shells / clipboard providers / build deps that the
# test suite spawns. nodejs is required for `actions/checkout@v6` to work
# inside a `jobs.<id>.container` runner.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        bash \
        zsh \
        xclip \
        xsel \
        wl-clipboard \
        build-essential \
        pkg-config \
        libssl-dev \
        curl \
        ca-certificates \
        git \
        gnupg \
        nodejs \
        locales \
    && locale-gen C.UTF-8 \
    && rm -rf /var/lib/apt/lists/*

# ---- PowerShell (via Microsoft apt repo) ------------------------------------
# Dynamically derive the Ubuntu code-name so this Dockerfile keeps working
# across LTS bumps (24.04 -> 26.04 etc.). The MS apt feed is the upstream-
# recommended install path for pwsh on Linux.
RUN UBUNTU_CODENAME=$(. /etc/os-release && echo "$VERSION_ID") \
    && curl -fsSL "https://packages.microsoft.com/config/ubuntu/${UBUNTU_CODENAME}/packages-microsoft-prod.deb" \
        -o /tmp/ms-prod.deb \
    && dpkg -i /tmp/ms-prod.deb \
    && rm /tmp/ms-prod.deb \
    && apt-get update \
    && apt-get install -y --no-install-recommends powershell \
    && rm -rf /var/lib/apt/lists/*

# ---- nushell (pinned tarball from GitHub Releases) --------------------------
# nu is not in the Ubuntu apt repos. Pinning the version here keeps CI
# behaviour reproducible; bumping NU_VERSION via ARG forces a rebuild.
RUN NU_ARCH="$( [ "$TARGETARCH" = "arm64" ] && echo aarch64 || echo x86_64 )-unknown-linux-gnu" \
    && curl -fsSL \
        "https://github.com/nushell/nushell/releases/download/${NU_VERSION}/nu-${NU_VERSION}-${NU_ARCH}.tar.gz" \
        -o /tmp/nu.tgz \
    && tar -xzf /tmp/nu.tgz -C /tmp \
    && install -m 0755 "/tmp/nu-${NU_VERSION}-${NU_ARCH}/nu" /usr/local/bin/nu \
    && rm -rf /tmp/nu.tgz "/tmp/nu-${NU_VERSION}-${NU_ARCH}"

# ---- Non-root user ----------------------------------------------------------
# CI and dev runs both bind-mount the repo into /workspace and run as uid
# 1001 so generated files (target/, .cache/, …) stay owned by the host
# user. uid 1001 is the standard mapping for the first non-root user on
# Ubuntu and matches what GitHub Actions uses when `options: --user 1001`
# is passed.
RUN useradd --create-home --uid 1001 --shell /bin/bash runex \
    && mkdir -p /workspace \
    && chown -R runex:runex /workspace

USER runex
WORKDIR /home/runex

# ---- Rust toolchain (rustup default stable, minimal profile) ----------------
# Installed under the `runex` user so cargo registry / target dirs land in
# its $HOME without sudo. Components: clippy + rustfmt for parity with
# `dtolnay/rust-toolchain@stable` defaults.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal \
            --component clippy --component rustfmt
ENV PATH=/home/runex/.cargo/bin:$PATH

# ---- Sanity check (run last so build fails on a missing tool) ---------------
# The script lives in the repo so contributors can also run it locally
# (`bash containers/ci/sanity.sh` inside an interactive shell of this
# image). COPY happens after USER so the file is owned by `runex`.
COPY --chown=runex:runex containers/ci/sanity.sh /usr/local/bin/runex-ci-sanity
RUN chmod +x /usr/local/bin/runex-ci-sanity && /usr/local/bin/runex-ci-sanity

# ---- Default command --------------------------------------------------------
# CI overrides this with explicit `cargo test ...` steps; the bash shell
# is here so an interactive `docker run -it ... bash` lands in something
# usable.
WORKDIR /workspace
CMD ["bash"]
