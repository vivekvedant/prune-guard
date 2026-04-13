# Observability, Security, and Portability

## Purpose

Add auditable structured observability with fail-closed runtime guards for privilege and platform safety.

The implementation is in `src/observability.rs`.

## Observability Behavior

### Structured Logs

- `StructuredLogRecord` enforces a stable `schema_version` (`LOG_SCHEMA_VERSION`).
- Every record includes:
  - `level`
  - `event_type`
  - `reason`
  - optional backend/action context
  - sanitized detail fields
- Log details are intentionally key/value so they can be validated by tests and indexed by log systems.

### Sensitive Data Redaction

- `redact_value` masks values for sensitive keys (`token`, `password`, `secret`, `credential`, `authorization`, `auth`).
- Bearer token payloads are redacted even under non-sensitive keys.
- Redaction is default behavior for structured log details.

### Per-Run Summaries

- `AuditableRunSummary::from_report` converts `SchedulerRunReport` into an auditable summary payload.
- Summary always contains:
  - action counts
  - skip counts
  - stop reason
  - last error context (when available)
- This guarantees every deletion/skip path has explicit rationale for operators.

### Optional Metrics

- `MetricsRecorder` provides optional counters without forcing metrics in all deployments.
- `NoopMetricsRecorder` supports metrics-disabled environments safely.
- `emit_scheduler_metrics` publishes run/action counters from scheduler reports.

## Security and Portability Guards

### Least-Privilege Check

- `evaluate_least_privilege` treats:
  - `uid == 0` as unsafe for real-run mode
  - unknown effective uid as unsafe for real-run mode
- Unsafe privilege state is fail-closed and converted to dry-run enforcement by preflight.

### OS Validation

- `parse_supported_os` and `validate_supported_os` enforce platform support for:
  - Linux
  - macOS
- Unsupported OS values are treated as unsafe for real-run mode.

### Runtime Preflight Decision

- `preflight_execution` combines:
  - requested dry-run mode
  - least-privilege status
  - portability status
- If any unsafe signal exists, preflight enforces dry-run with explicit reasons.

## Safety Rationale

- Structured logs with mandatory reason fields make every skip/delete auditable.
- Redaction prevents sensitive runtime values from leaking into logs.
- Least-privilege and OS checks fail closed before real destructive execution.
- Optional metrics preserve minimal runtime footprint while keeping observability extensible.

## Test Coverage Added

`tests/observability_security_portability_tests.rs` covers:

- structured log schema field presence and backend context
- sensitive value redaction behavior
- auditable run-summary conversion from scheduler report
- optional metrics counter emission
- least-privilege fail-closed behavior
- Linux/macOS portability matrix acceptance and Windows rejection
- unsupported OS and elevated privilege preflight dry-run enforcement
