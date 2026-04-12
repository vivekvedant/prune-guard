# Phase 7 Reliability and Error Handling

## Purpose

Define reliability controls that preserve fail-closed cleanup behavior under backend outages, transient command errors, and concurrent daemon execution.

## Reliability Behaviors

### Retries with Backoff

- Retry only transient backend operations (discovery, inspect, and delete command invocation failures).
- Use bounded exponential backoff with jitter to reduce synchronized retry spikes.
- Keep retry count finite; when retry budget is exhausted, stop retries and mark the operation failed.
- Never treat retry exhaustion as success.

### Partial-Failure Continuation

- Continue processing other independent backends/actions when one backend/action fails.
- Isolate failure domains so one failing backend does not crash the whole cycle.
- Record per-backend and per-action outcomes in the run summary for operator visibility.
- Preserve per-action safety checks before every real deletion attempt.

### All-Backends-Fail No-Op

- If every backend is unavailable or fails during a cycle, perform a safe no-op cycle.
- No-op means no deletion commands are executed.
- Return/report the run as unsuccessful or degraded, but with zero destructive actions.

### Single-Instance Locking

- Acquire a single-instance lock at cycle start.
- If lock acquisition fails because another instance holds it, skip the cycle safely.
- Do not run overlapping cleanup cycles; avoid duplicate or racing deletions.
- Always release lock on cycle completion and on error paths.

### Fail-Closed on Repeated Failures

- Track consecutive backend/cycle failures.
- When repeated failures exceed configured threshold, enter fail-closed mode for affected backend/cycle.
- In fail-closed mode, skip deletion execution and surface explicit operator-facing failure status.
- Resume normal execution only after successful health/recovery checks reset the failure streak.

## Safety Rationale

- Bounded retries prevent infinite loops while still tolerating transient failures.
- Partial-failure continuation improves availability without weakening deletion safety gates.
- All-backends-fail no-op guarantees uncertainty never results in accidental deletion.
- Single-instance locking prevents concurrent plans from acting on stale shared state.
- Repeated-failure fail-closed behavior prevents escalating unsafe actions during unstable backend conditions.

## Testing Coverage

Phase 7 test coverage should validate:

- retry policy correctness (attempt count, backoff growth, and stop at retry budget)
- continuation semantics when one backend fails and others succeed
- no-op behavior when all backends fail (zero executed deletions)
- single-instance lock behavior (second instance skips safely)
- repeated-failure threshold transitions into fail-closed mode
- recovery path that exits fail-closed mode only after successful checks

