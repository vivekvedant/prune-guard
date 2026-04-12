# Observability, Security, and Portability Flowchart

This document captures runtime observability and fail-closed preflight behavior.

## Structured Logging and Redaction Flow

```mermaid
flowchart TD
    A[Create structured log record] --> B[Attach event_type level reason]
    B --> C[Attach backend/action context if present]
    C --> D[Attach detail fields]
    D --> E{Detail key/value sensitive?}
    E -- Yes --> F[Redact value]
    E -- No --> G[Keep value]
    F --> H[Emit schema-versioned log]
    G --> H
```

## Runtime Preflight Flow (Security + Portability)

```mermaid
flowchart TD
    A[Start run preflight] --> B[Evaluate least-privilege state]
    B --> C[Validate OS support]
    C --> D{Requested dry-run?}
    D -- Yes --> E[Enforce dry-run]
    D -- No --> F{Least-privilege OK and OS supported?}
    F -- Yes --> G[Allow real-run eligibility]
    F -- No --> E
    E --> H[Record explicit fail-closed reasons]
    G --> I[Continue with execution pipeline]
```

## Per-Run Summary and Metrics Flow

```mermaid
flowchart TD
    A[SchedulerRunReport] --> B[Build AuditableRunSummary]
    B --> C[Include counts + stop reason + last error]
    C --> D[Emit structured run log]
    C --> E{Metrics enabled?}
    E -- Yes --> F[Increment run/action counters]
    E -- No --> G[No-op metrics recorder]
```

Notes:

- Unsafe privilege or unsupported OS never bypasses into real-run execution.
- Structured logs are schema-versioned and redact sensitive data by default.
- Metrics are optional and non-blocking; observability remains available without metrics backends.
