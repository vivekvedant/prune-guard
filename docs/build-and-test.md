# Build and Test

This guide covers the minimal steps to build, validate, and package `prune-guard` locally.

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

## Create Debian Package (`.deb`)

Debian packaging is supported on Linux hosts only.

1. Install packaging tools:

```bash
sudo apt-get update
sudo apt-get install -y dpkg-dev
```

2. Build release artifacts (required before packaging):

```bash
cargo build --release --locked
```

3. Create the `.deb` package and checksum:

```bash
./scripts/release/package-artifacts-deb.sh
```

Default outputs are written to `dist/`:

```text
dist/prune-guard_<version>_<arch>.deb
dist/prune-guard_<version>_<arch>.deb.sha256
```

4. Verify package metadata before install:

```bash
dpkg-deb --info dist/prune-guard_*.deb
dpkg-deb --contents dist/prune-guard_*.deb
```

5. Install package locally (optional):

```bash
sudo dpkg -i dist/prune-guard_*.deb
```

6. Confirm service/timer state after install:

```bash
systemctl status prune-guard.service
systemctl status prune-guard.timer
```

7. Interval control comes from TOML:

```bash
grep -n "interval_secs" /etc/prune-guard/prune-guard.toml
```

`prune-guard.service` runs continuously and sleeps between ticks using `interval_secs`.

8. Docker daemon targeting also comes from TOML (no systemd override required):

```toml
[docker]
# Choose exactly one
host = "unix:///home/<user>/.docker/desktop/docker.sock"
# context = "desktop-linux"
```

Use this when service execution context differs from your interactive shell Docker context.

## Smoke Test

After a release build, run a smoke test to confirm the daemon binary starts and argument parsing works:

```bash
./target/release/prune-guard --help
```

Optional one-shot dry-run smoke test using the install config template:

```bash
./target/release/prune-guard --config config/prune-guard.toml --once
```

## Docker Storage Growth Helper

Use `test_bin.sh` when you want a local Docker host to keep consuming disk in a controlled loop.

```bash
PG_STRESS_CHUNK_MB=64 PG_STRESS_MAX_ITERATIONS=0 ./test_bin.sh
```

Key behavior:

- Creates a unique image every iteration (non-cacheable random payload).
- Creates a stopped container from that image.
- Creates a volume and writes random data into it.
- Prints `docker system df` each iteration so growth is visible.

Important controls:

- `PG_STRESS_MAX_ITERATIONS=0` means run forever.
- Set a positive `PG_STRESS_MAX_ITERATIONS` to stop automatically.
- Lower `PG_STRESS_CHUNK_MB` for slower growth.

Safety notes:

- This helper intentionally increases local Docker storage; do not run on shared or production Docker hosts.
- If Docker is unavailable, the script exits immediately (fail-closed behavior).
- Dry-run for prune-guard daemon validation remains the default behavior in `config/prune-guard.toml`.

## logs
journalctl -u prune-guard -n 50 --no-pager
journalctl -u prune-guard -f

## Safety Notes

- Keep `dry_run = true` while validating in shared or production-like environments.
- If backend metadata is ambiguous, prune-guard is designed to fail closed and skip deletion.
