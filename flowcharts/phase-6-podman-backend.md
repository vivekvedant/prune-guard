# Phase 6 Podman Backend Flowchart

This document captures Podman adapter parity flow and graceful degradation behavior.

## Health and Degradation Flow

```mermaid
flowchart TD
    A[Run podman version] --> B{Version available and non-empty?}
    B -- Yes --> C[HealthReport healthy=true]
    B -- No --> D[HealthReport healthy=false]
    D --> E[Scheduler skips backend safely]
```

## Discovery and Execution Safety Flow

```mermaid
flowchart TD
    A[Discover Podman containers/images/volumes] --> B[Build candidate safety metadata]
    B --> C{Metadata complete and unambiguous?}
    C -- No --> D[Mark candidate non-actionable]
    C -- Yes --> E[Planner may produce delete action]
    E --> F{Dry-run?}
    F -- Yes --> G[Return synthetic non-executed response]
    F -- No --> H{Resource kind}
    H -- Container --> I{Container running now?}
    I -- Yes --> Z[SafetyViolation]
    I -- No --> J[Run podman container rm]
    H -- Image --> K{Image referenced now?}
    K -- Yes --> Z
    K -- No --> L[Run podman image rm]
    H -- Volume --> M{Volume attached now?}
    M -- Yes --> Z
    M -- No --> N[Run podman volume rm]
```

Notes:

- Podman unavailability degrades to unhealthy backend status instead of crashing the run.
- Safety checks are repeated before deletion to maintain fail-closed behavior under changing runtime state.
