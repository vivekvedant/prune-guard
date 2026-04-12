# Phase 4 PR Test Gate

## Purpose

Add a CI gate that runs the complete Rust test suite for every pull request
targeting `main`.

## Workflow Added

- `.github/workflows/pr-main-tests.yml`

Trigger behavior:

- Runs on `pull_request` events for `main`
- Covers `opened`, `synchronize`, `reopened`, and `ready_for_review`
- Cancels in-progress runs for the same PR when a new commit is pushed

Execution behavior:

- Uses stable Rust toolchain
- Uses cargo cache for faster repeat runs
- Runs `cargo test --all-targets --all-features --locked`

## Safety Rationale

- Safety-critical deletion logic must not merge without full test validation.
- Running all targets and features prevents partial validation blind spots.
- `--locked` prevents accidental dependency drift in CI.

## Merge Gate Requirement

GitHub Actions provides the status check, but merge blocking is enforced by
branch protection rules on `main`.

Required status check name:

- `cargo-test-all-targets`
