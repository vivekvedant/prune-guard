# Build and Test

This guide covers the minimal steps to build and validate `prune-guard` locally.

## Prerequisites

- Rust toolchain installed (`rustup`, `cargo`)
- Linux or macOS shell environment
- Optional backend CLIs for runtime checks:
  - Docker (`docker`)
  - Podman (`podman`)

## Build

1. Build debug binaries during development:

```bash
cargo build --locked
```

2. Build optimized release artifacts:

```bash
cargo build --release --locked
```

Release binary path:

```text
target/release/prune-guard
```

## Test

Run the full automated test suite:

```bash
cargo test --locked
```

Run one test file while iterating:

```bash
cargo test --test docker_backend_tests --locked
```

## Smoke Test

After a release build, run a smoke test to confirm the daemon binary starts and argument parsing works:

```bash
./target/release/prune-guard --help
```

Optional one-shot dry-run smoke test using the install config template:

```bash
./target/release/prune-guard --config config/prune-guard.toml --once
```

## Safety Notes

- Keep `dry_run = true` while validating in shared or production-like environments.
- If backend metadata is ambiguous, prune-guard is designed to fail closed and skip deletion.
