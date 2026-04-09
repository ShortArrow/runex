# VHS Demo

This directory contains a `vhs` tape for recording the `runex` CLI demo GIF.

## Record with Docker (recommended)

```bash
bash docs/vhs/record.sh
```

Builds a Docker image with Rust + VHS, compiles runex inside the container, and outputs `docs/vhs/demo.gif`. Only Docker is required.

## Record manually

If you have `vhs` and `runex` installed:

```bash
vhs docs/vhs/demo.tape
```
