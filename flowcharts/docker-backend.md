# Docker Backend Flowchart

This document captures Docker adapter control flow and safety guards.

## Discovery Safety Flow

```mermaid
flowchart TD
    A[Collect Docker containers/images/volumes] --> B[Build candidate metadata]
    B --> C{Metadata complete and unambiguous?}
    C -- No --> D[Mark metadata_ambiguous / metadata_complete=false]
    C -- Yes --> E[Mark candidate with in_use and referenced flags]
    D --> F[Policy layer skips candidate fail-closed]
    E --> G[Planner may create delete action]
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
    X --> K[Return executed=true]
    Y --> K
    W --> K
```

Notes:

- Safety checks are re-run immediately before delete to prevent stale-plan unsafe removals.
- Any uncertainty in safety checks stops execution rather than proceeding optimistically.
