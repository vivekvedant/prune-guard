# Docker Backend Flowchart

This document captures Docker adapter control flow and safety guards.

## Discovery Safety Flow

```mermaid
flowchart TD
    A[Collect Docker containers/images/volumes/build-cache] --> A1[Load volume sizes via docker system df -v]
    A1 --> B[Inspect image with labels template]
    B --> C{Labels inspect hit known missing-labels template error?}
    C -- Yes --> D[Retry image inspect with labels-free template]
    C -- No --> E[Use primary inspect output]
    D --> F[Mark labels unknown -> metadata_complete=false]
    E --> G[Build candidate metadata]
    F --> G
    G --> H{Metadata complete and unambiguous?}
    H -- No --> I[Mark metadata_ambiguous / metadata_complete=false]
    H -- Yes --> J[Mark candidate with in_use and referenced flags]
    I --> K[Policy layer skips candidate fail-closed]
    J --> L[Planner may create delete action]
```

## Execution Safety Re-Validation

```mermaid
flowchart TD
    A[Planned delete action] --> B{Dry-run mode?}
    B -- Yes --> C[Return synthetic dry-run response]
    B -- No --> D{Resource kind}
    D -- Container --> E{Container running now?}
    E -- Yes --> Z[SafetyViolation: block delete]
    E -- No --> X[Run docker container rm]
    D -- Image --> H{Image referenced now?}
    H -- Yes --> Z
    H -- No --> Y[Run docker image rm]
    D -- Volume --> J{Volume attached now?}
    J -- Yes --> Z
    J -- No --> W[Run docker volume rm]
    D -- BuildCache --> L{Build cache candidate id valid?}
    L -- No --> Z
    L -- Yes --> M[Run docker builder prune -f and optional until filter]
    X --> K[Return executed=true]
    Y --> K
    W --> K
    M --> K
```

Notes:

- Safety checks are re-run immediately before delete to prevent stale-plan unsafe removals.
- Any uncertainty in safety checks stops execution rather than proceeding optimistically.
