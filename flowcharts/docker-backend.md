# Docker Backend Flowchart

This document captures Docker adapter control flow and safety guards.

## Command Routing

```mermaid
flowchart TD
    A[Load docker.host / docker.context from config] --> B{Exactly one set?}
    B -- No, both set --> C[Config validation fails closed]
    B -- Yes --> D[Build docker CLI global args]
    B -- None --> F[Probe known local socket endpoints with docker version]
    F --> G{Exactly one reachable socket host?}
    G -- Yes --> H[Use detected --host endpoint]
    G -- No, none reachable --> D
    G -- No, multiple reachable --> I[Fail closed and require explicit docker.host or docker.context]
    D --> E[Prefix every docker command with --host or --context when configured]
    H --> E
```

## Discovery Safety Flow

```mermaid
flowchart TD
    A[Collect Docker containers/images/volumes/build-cache] --> A1[Load volume sizes via docker system df -v]
    A1 --> B[Inspect image with labels template]
    B --> C{Labels inspect hit known missing-labels template error?}
    C -- Yes --> D[Retry image inspect with labels-free template]
    C -- No --> E[Use primary inspect output]
    D --> F{protected_labels configured?}
    F -- No --> F2[Proceed without label-based safety requirement]
    F -- Yes --> F3{allow_missing_image_labels enabled?}
    F3 -- No --> F1[Mark labels unknown -> metadata_complete=false]
    F3 -- Yes --> F4[Inspect json labels and require exact `null`]
    F4 --> F5{labels json exactly null?}
    F5 -- No --> F1
    F5 -- Yes --> F6[Treat labels as safe empty set]
    E --> G[Build candidate metadata]
    F1 --> G
    F2 --> G
    F6 --> G
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
    L -- Yes --> M[Run docker builder prune -f with optional until filter and prune-budget flags]
    X --> K[Return executed=true]
    Y --> K
    W --> K
    M --> K
    N[Guard inspect says No such container] --> O[Treat as stale and continue guard evaluation]
    O --> H
    O --> J
    P[Delete says No such target] --> K
```

Notes:

- Safety checks are re-run immediately before delete to prevent stale-plan unsafe removals.
- Explicitly missing resources are treated as idempotent no-ops; ambiguous safety metadata still fails closed.
- Image reference guards use `docker ps --format {{.ImageID}}` first, with per-container inspect fallback when that template field is unsupported.
