# Phase 4 PR Test Gate

## Purpose

Add a CI gate that runs the complete Rust test suite for every pull request
targeting `main`.

## Workflow Added

- `.github/workflows/pr-main-tests.yml`
- `.circleci/config.yml`

Trigger behavior:

- Runs on `pull_request` events for `main`
- Covers `opened`, `synchronize`, `reopened`, and `ready_for_review`
- Cancels in-progress runs for the same PR when a new commit is pushed

Execution behavior:

- Uses stable Rust toolchain
- Uses cargo cache for faster repeat runs
- Runs `cargo test --all-targets --all-features --locked`

CircleCI behavior:

- Uses `cimg/rust:1.94` instead of the floating `cimg/rust:stable` alias.
- Avoids `manifest unknown` pull failures caused by missing alias tags.
- Keeps runtime predictable by pinning to a concrete Rust image line.
- Uses an OAuth-safe guard step that:
  - halts when `CIRCLE_PULL_REQUEST` is missing
  - falls back to `CIRCLE_PULL_REQUESTS` when present
  - fetches PR metadata from GitHub API
  - halts unless PR `base.ref` is `main`
- Exits cleanly (`step halt` + `exit 0`) for non-PR or non-targeted pipelines so skipped runs do not appear as failures.
- Fails fast (`exit 1`) when PR metadata cannot be fetched or parsed for an actual PR context.
- Avoids unsupported `pipeline.event.*` variables on OAuth projects.
- Avoids `branches: only: main` because that is push-based and can skip PR branch pipelines.
- Adds `branches.ignore: main` to avoid direct main push pipelines for this PR-only workflow.

## Safety Rationale

- Safety-critical deletion logic must not merge without full test validation.
- Running all targets and features prevents partial validation blind spots.
- `--locked` prevents accidental dependency drift in CI.
- A versioned container image fails closed more safely than a floating alias because pull behavior is explicit and reproducible.

## Regression Test Coverage

- `tests/circleci_config_tests.rs` enforces that CircleCI uses a versioned `cimg/rust` tag.
- The test fails if `:stable` is reintroduced, preventing recurrence of alias-related pull outages.
- `tests/circleci_config_tests.rs` enforces OAuth-safe PR-to-main guard logic and rejects unsupported `pipeline.event.*` variables.
- Regression checks also enforce clean skip behavior (`exit 0`) and `main` push filtering.

## Merge Gate Requirement

GitHub Actions provides the status check, but merge blocking is enforced by
branch protection rules on `main`.

Required status check name:

- `cargo-test-all-targets`
