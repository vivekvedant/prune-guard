# Phase 3 Planner + Executor

## Purpose

Phase 3 implements the safety-critical bridge between candidate policy filtering
and backend deletion calls.

This phase introduces:

- deterministic action planning with per-run delete-cap enforcement
- dry-run-safe execution that never calls backend delete
- timeout-wrapped real execution with per-action error capture

## Scope Delivered

- `src/planner.rs` with `CleanupPlanner`
- `src/executor.rs` with `CleanupExecutor`
- `ExecutionReport` and `ActionExecutionFailure` for batch execution outcomes
- integration tests in:
  - `tests/planner_tests.rs`
  - `tests/executor_tests.rs`

## Planner Contract

`CleanupPlanner::plan` applies these gates in order:

1. run all candidates through `PolicyEngine`
2. reject candidates whose backend does not match the plan backend
3. reject candidates with unknown `size_bytes` (fail-closed cap enforcement)
4. stop adding actions when `max_delete_per_run_gb` budget is exhausted

Planner outputs:

- `ActionPlan.actions` with deterministic order
- `ActionPlan.skipped` with one explicit reason per rejected candidate

## Executor Contract

`CleanupExecutor::execute_plan` enforces:

1. if plan-level `dry_run` is true, return synthetic dry-run responses only
2. if action-level `dry_run` is true, skip backend call for that action
3. for real-run actions, wrap backend execution in per-action timeout guard
4. capture per-action failures and continue executing remaining actions

Execution outputs:

- `ExecutionReport.completed` for successful responses (including dry-run synth responses)
- `ExecutionReport.failures` for backend errors and timeout failures

## Safety Rationale

- Unknown reclaim size is unsafe for delete-cap control and is rejected.
- Dry-run mode is enforced before backend execution to avoid accidental deletion.
- Timeout guard prevents one hanging backend operation from blocking the full run.
- Per-action error capture avoids unsafe partial-abort logic and preserves auditability.

## Tests Added

`tests/planner_tests.rs` covers:

- deterministic cap enforcement and order preservation
- unknown-size fail-closed rejection
- backend mismatch rejection
- policy rejection propagation into skipped list

`tests/executor_tests.rs` covers:

- plan-level dry-run no-delete behavior
- action-level dry-run no-delete behavior
- real-run success behavior
- per-action error capture with continuation
- timeout failure capture

## Out of Scope in Phase 3

- scheduler watermark loop and stop conditions
- real Docker/Podman backend adapter behavior
- retry/backoff strategy across run cycles

## Next Phase Handoff

Phase 4 can consume:

- `CleanupPlanner::plan` to generate bounded action plans
- `CleanupExecutor::execute_plan` to run those plans safely inside the scheduler loop
