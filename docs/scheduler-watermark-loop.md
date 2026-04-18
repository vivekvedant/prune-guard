# Scheduler Watermark Loop

## Purpose

This document adds a bounded scheduler tick that starts cleanup only when storage usage reaches the high watermark and then loops safely until a stop condition is met.

This scheduler orchestrates policy filtering and planning/execution behavior under explicit fail-closed stop rules.

## Scope Delivered

- `src/scheduler.rs` with `CleanupScheduler`
- per-tick report model: `SchedulerRunReport`
- explicit cycle stop enum: `SchedulerStopReason`
- one-shot tick API: `run_once`
- periodic wrapper API: `run_for_ticks`
- scheduler safety tests in `tests/scheduler_tests.rs`

## Scheduler Contract

`CleanupScheduler::run_once` enforces this order:

1. run backend health check
2. collect usage snapshot
3. fail closed if usage percent is unknown
4. skip cleanup when usage is below `high_watermark_percent`
5. when at/above high watermark, run bounded cleanup iterations:
   - re-check backend health before each later iteration
   - discover candidates
   - build bounded plan via planner
   - execute plan via executor
   - recollect usage before next iteration
6. terminate with one explicit `SchedulerStopReason`

`CleanupScheduler::run_for_ticks` repeats `run_once` for a fixed number of ticks,
with `interval_secs` sleep between ticks. Each tick remains bounded by the same
fail-closed gates.

## Stop Conditions

The cycle always exits with one explicit stop reason:

- `BelowHighWatermark`
- `TargetWatermarkReached`
- `NoActionableCandidates`
- `DeletionCapReached`
- `IterationLimitReached`
- `BackendUnhealthy`
- `HealthCheckFailed`
- `UsageCollectionFailed`
- `UsagePercentUnknown`
- `CandidateDiscoveryFailed`
- `ExecutionFailuresDetected`

## Safety Rationale

- Cleanup does not start unless current usage is at/above high watermark.
- Unknown usage percent is treated as unsafe and stops the cycle.
- Planner returning zero actions ends the loop immediately to prevent spin.
- A per-cycle iteration cap prevents runaway work when usage does not converge.
- Discovery/health/usage errors stop the cycle instead of forcing execution.
- Any execution failure stops the cycle immediately to avoid repeated unsafe retries in the same tick.
- Stop reasons and counters are captured in `SchedulerRunReport` for auditability.

## CLI Tick Output

`src/main.rs` logs one summary line per tick and includes usage/reclaim fields:

- `tick`
- `backend`
- `dry_run`
- `cleanup_started`
- `stop_reason`
- `actions_planned`
- `actions_completed`
- `action_failures`
- `skipped_candidates`
- `initial_used_bytes`
- `final_used_bytes`
- `reclaimed_bytes`
- `reclaimed_source` (`observed`, `estimated`, `unknown`)
- `usage_percent_before`
- `usage_percent_after`
- `last_error` (emitted as a second log line only when present)

### Output Key Reference

- `tick`: 1-based scheduler tick number within the current process run.
- `backend`: active backend for the tick (`Docker` or `Podman`).
- `dry_run`: whether execution mode was dry-run (`true`) or real deletion (`false`).
- `cleanup_started`: whether usage reached high watermark and cleanup loop started.
- `stop_reason`: explicit bounded stop condition (`BelowHighWatermark`, `NoActionableCandidates`, `DeletionCapReached`, `ExecutionFailuresDetected`, etc.).
- `actions_planned`: total delete actions planned across loop iterations in this tick.
- `actions_completed`: total actions that executed successfully (or safe idempotent no-op for already-missing targets).
- `action_failures`: total action execution failures; non-zero indicates fail-closed early stop.
- `skipped_candidates`: candidates skipped by policy/planner safety gates (in use, referenced, protected, ambiguous metadata, delete-cap constraints).
- `initial_used_bytes`: storage used at tick start (human-readable units, or `unknown` if unavailable).
- `final_used_bytes`: storage used at tick end (human-readable units, or `unknown` if unavailable).
- `reclaimed_bytes`: reclaimed storage for the tick.
- `reclaimed_source`: how `reclaimed_bytes` was derived:
  - `observed`: computed from `initial_used_bytes - final_used_bytes`
  - `estimated`: fallback from executed action size estimates when observed delta is unavailable or zero
  - `unknown`: neither observed nor estimated reclaim is available
- `usage_percent_before`: usage percent at tick start (`unknown` if unavailable).
- `usage_percent_after`: usage percent at tick end (`unknown` if unavailable).
- `last_error`: backend/scheduler error detail string printed as `tick=<n> last_error=<message>` when the tick hit an error path.

### Example

Byte-valued fields are rendered in human-readable units (`B/KB/MB/GB/...`). When usage snapshots are unavailable for a field, output prints `unknown` to preserve fail-closed observability. If observed usage delta is zero but executed actions reported reclaimable sizes, `reclaimed_bytes` falls back to an estimated value and marks `reclaimed_source=estimated`.

`tick=42 backend=Docker dry_run=false cleanup_started=true stop_reason=NoActionableCandidates actions_planned=6 actions_completed=6 action_failures=0 skipped_candidates=24 initial_used_bytes=15.01 GB final_used_bytes=14.44 GB reclaimed_bytes=582.40 MB reclaimed_source=observed usage_percent_before=86 usage_percent_after=82`

## Tests Added

`tests/scheduler_tests.rs` covers:

- below-high watermark path: no discovery and no execution
- above-high loop: repeated iterations until target watermark is reached
- zero-action planner path: immediate fail-closed stop, no infinite loop
- unknown usage percent: fail-closed stop before cleanup pipeline

## Out of Scope in This Phase

- long-running daemon process supervision
- retry/backoff policy across multiple ticks
- backend-specific adapter guarantees beyond current contracts
