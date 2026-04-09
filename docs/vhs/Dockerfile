FROM ghcr.io/charmbracelet/vhs

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl build-essential pkg-config \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /src
ENTRYPOINT ["/bin/bash", "-c", "cargo build --release -p runex && cp target/release/runex /usr/local/bin/runex && vhs docs/vhs/demo.tape"]
