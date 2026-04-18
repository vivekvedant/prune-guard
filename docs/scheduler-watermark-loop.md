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

`src/main.rs` logs one summary line per tick and now includes usage/reclaim fields:

- `initial_used_bytes`
- `final_used_bytes`
- `reclaimed_bytes`
- `usage_percent_before`
- `usage_percent_after`

When usage snapshots are unavailable for a field, output prints `unknown` to preserve fail-closed observability.

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
