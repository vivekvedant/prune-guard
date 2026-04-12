# Scheduler + Watermark Loop Flowcharts

This document captures scheduler start/stop behavior and fail-closed exit paths.

## 1) Watermark Loop Start and Stop

```mermaid
flowchart TD
    A[run_once start] --> B[Health check]
    B --> C{Healthy?}
    C -- No --> S1[Stop: BackendUnhealthy or HealthCheckFailed]
    C -- Yes --> D[Collect usage]
    D --> E{usage percent known?}
    E -- No --> S2[Stop: UsagePercentUnknown]
    E -- Yes --> F{used < high watermark?}
    F -- Yes --> S3[Stop: BelowHighWatermark]
    F -- No --> G[cleanup_started = true]
    G --> H{used <= target watermark?}
    H -- Yes --> S4[Stop: TargetWatermarkReached]
    H -- No --> I[Discover candidates]
    I --> J[Plan actions]
    J --> K{actions empty?}
    K -- Yes --> S5[Stop: NoActionableCandidates or DeletionCapReached]
    K -- No --> L[Execute plan]
    L --> L1{Any execution failure?}
    L1 -- Yes --> S7[Stop: ExecutionFailuresDetected]
    L1 -- No --> M[Recollect usage]
    M --> N{iterations < max?}
    N -- Yes --> H
    N -- No --> S6[Stop: IterationLimitReached]
```

## 2) Fail-Closed Exit Paths

```mermaid
flowchart TD
    A[Scheduler tick] --> B{Health check error?}
    B -- Yes --> X1[Stop: HealthCheckFailed]
    B -- No --> C{Health check unhealthy?}
    C -- Yes --> X2[Stop: BackendUnhealthy]
    C -- No --> D{Usage collection error?}
    D -- Yes --> X3[Stop: UsageCollectionFailed]
    D -- No --> E{Usage percent unknown?}
    E -- Yes --> X4[Stop: UsagePercentUnknown]
    E -- No --> F{Candidate discovery error?}
    F -- Yes --> X5[Stop: CandidateDiscoveryFailed]
    F -- No --> G{Execution failure?}
    G -- Yes --> X6[Stop: ExecutionFailuresDetected]
    G -- No --> H[Proceed with bounded planner/executor loop]
```

Notes:

- Every unsafe or uncertain branch exits the current cycle immediately.
- No exit path forces deletion when required safety signals are missing.
