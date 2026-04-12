# Prune Guard

Prune Guard is a safety-first cleanup daemon for container runtimes.
It reclaims disk space by removing unused artifacts while defaulting to conservative behavior.

## Safety Model

- `dry_run` is the default behavior.
- Fail-closed decisions are mandatory on uncertainty.
- Never delete active resources (running, referenced, or protected artifacts).
- Every cleanup/skip path should be auditable through structured summaries.

## What It Supports

- Policy-based candidate filtering
- Cleanup planning with per-run deletion caps
- Safe execution with timeout/error capture
- Watermark-driven scheduler loop
- Docker and Podman backend adapters
- Reliability controls (retries, lock, no-op on full backend failure)
- Observability/security preflight checks

## Project Structure

- `src/`: core runtime modules
- `tests/`: unit and integration coverage
- `docs/`: user-facing feature documentation and operational guides
- `flowcharts/`: visual behavior and safety workflows
- `.circleci/config.yml`: PR-to-main CI gate

## Quick Start

1. Build and run tests:

```bash
cargo test --locked
```

2. Review documentation:

- [docs/README.md](docs/README.md)
- [flowcharts/README.md](flowcharts/README.md)

3. Use release guidance before merging:

- [docs/release-runbook.md](docs/release-runbook.md)
- [docs/pr-checklist.md](docs/pr-checklist.md)

## Feature Docs

- [Core Architecture](docs/core-architecture.md)
- [Policy Engine](docs/policy-engine.md)
- [Cleanup Planning and Execution](docs/cleanup-planning-and-execution.md)
- [Scheduler Watermark Loop](docs/scheduler-watermark-loop.md)
- [CI PR Main Gate](docs/ci-pr-main-gate.md)
- [Docker Backend](docs/docker-backend.md)
- [Podman Backend](docs/podman-backend.md)
- [Reliability and Error Handling](docs/reliability-and-error-handling.md)
- [Observability Security Portability](docs/observability-security-portability.md)
- [Release Readiness](docs/release-readiness.md)

## Flowcharts

- [Core Architecture](flowcharts/core-architecture.md)
- [Policy Engine](flowcharts/policy-engine.md)
- [Cleanup Planning and Execution](flowcharts/cleanup-planning-and-execution.md)
- [Scheduler Watermark Loop](flowcharts/scheduler-watermark-loop.md)
- [CI PR Main Gate](flowcharts/ci-pr-main-gate.md)
- [Docker Backend](flowcharts/docker-backend.md)
- [Podman Backend](flowcharts/podman-backend.md)
- [Reliability and Error Handling](flowcharts/reliability-and-error-handling.md)
- [Observability Security Portability](flowcharts/observability-security-portability.md)
- [Release Readiness](flowcharts/release-readiness.md)

## CI Policy

The CircleCI workflow is intended to run for pull requests targeting `main`, with guardrails that halt non-targeted runs and fail closed on ambiguous PR metadata.
