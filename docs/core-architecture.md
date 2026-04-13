# Core Architecture

## Purpose

This document establishes a compile-ready, safety-first Rust foundation for the Cleanup Daemon.
It does not perform real cleanup against Docker/Podman yet. It defines the shared model, configuration contract, backend interfaces, and baseline tests that future work builds on.

## Scope Delivered

- Rust crate scaffold and module layout
- Shared domain model for cleanup planning and execution
- Backend trait contracts for all pipeline stages
- Unified error model and `Result<T>` alias
- Config parsing/loading with conservative validation
- Baseline tests for config and fail-closed candidate behavior

## Module Map

- `src/lib.rs`: crate entrypoint and public exports
- `src/main.rs`: daemon entrypoint (config loading, backend selection, scheduler loop)
- `src/config.rs`: `Config` model, TOML subset parser, validation rules
- `config/prune-guard.toml`: install template config (deploy to `/etc/prune-guard/prune-guard.toml`)
- `packaging/systemd/prune-guard.service`: oneshot systemd unit for one scheduler tick execution
- `packaging/systemd/prune-guard.timer`: recurring timer that triggers the oneshot service
- `src/domain.rs`: runtime-safe domain structs/enums
- `src/backend.rs`: backend pipeline contracts
- `src/error.rs`: shared error enum for all stages
- `tests/config_tests.rs`: config default, parsing, and validation tests
- `tests/model_tests.rs`: fail-closed candidate actionability tests

## Safety Defaults

This foundation intentionally defaults to fail-closed behavior.

- `dry_run` defaults to `true`
- unknown or ambiguous candidate metadata is treated as non-actionable
- invalid watermark relationships are rejected at config load time
- unknown config keys are rejected to avoid silent misconfiguration

## Configuration Contract

The current config model supports:

- `interval_secs`
- `high_watermark_percent`
- `target_watermark_percent`
- `min_unused_age_days`
- `max_delete_per_run_gb`
- `dry_run`
- `enabled_backends`
- `protected_images`
- `protected_volumes`
- `protected_labels`

Accepted TOML forms include top-level keys and selected section aliases such as `[runtime]`, `[thresholds]`, `[cleanup]`, `[safety]`, and `[allowlists]`.

For installed deployments, use `config/prune-guard.toml` as the baseline template and copy it to `/etc/prune-guard/prune-guard.toml`.

## Backend Contract (Pipeline Shape)

Any backend adapter is expected to implement these stages:

1. `HealthCheck`
2. `UsageCollector`
3. `CandidateDiscoverer`
4. `ActionPlanner`
5. `ExecutionContract`

The composite `CleanupBackend` trait is a convenience alias requiring all stages.

## Validation and Tests

Baseline tests verify:

- safety-first config defaults
- explicit config overrides from TOML
- sectioned TOML parsing
- invalid threshold relationship rejection
- fail-closed candidate behavior for incomplete/ambiguous metadata

## Out of Scope

- scheduler watermark loop
- real backend adapters (Docker/Podman)
- deletion execution against runtime APIs
- retry logic and advanced observability

## Next Phase Handoff

Policy filtering should build on top of these models/contracts.
The critical rule remains unchanged: when uncertain, skip deletion.
