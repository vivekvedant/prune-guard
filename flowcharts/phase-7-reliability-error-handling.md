# Phase 7 Reliability and Error Handling Flowchart

This document captures retry, continuation, locking, and fail-closed reliability behavior.

## Cycle Reliability Flow

```mermaid
flowchart TD
    A[Start cleanup cycle] --> B{Acquire single-instance lock?}
    B -- No --> C[Skip cycle safely<br/>No-op]
    B -- Yes --> D[Run backend health/discovery]
    D --> E{Any backend succeeded?}
    E -- No --> F[All-backends-fail<br/>No-op cycle]
    E -- Yes --> G[Process actions per backend]
    G --> H{Action failed?}
    H -- No --> I[Record success]
    H -- Yes --> J[Retry with bounded backoff]
    J --> K{Retry budget left?}
    K -- Yes --> G
    K -- No --> L[Record action failure]
    L --> M{Other backends/actions remain?}
    M -- Yes --> G
    M -- No --> N[Finalize run summary]
    I --> M
    F --> N
    C --> N
    N --> O[Release lock]
```

## Repeated-Failure Fail-Closed Flow

```mermaid
flowchart TD
    A[Record cycle/backend result] --> B{Failure?}
    B -- No --> C[Reset failure streak]
    B -- Yes --> D[Increment failure streak]
    D --> E{Streak exceeds threshold?}
    E -- No --> F[Next cycle normal eligibility]
    E -- Yes --> G[Enter fail-closed mode]
    G --> H[Skip deletion execution]
    H --> I[Expose degraded status]
    I --> J{Recovery checks successful?}
    J -- No --> H
    J -- Yes --> K[Exit fail-closed and reset streak]
```

Notes:

- Lock contention and all-backend failure both end in safe no-op outcomes.
- Retry exhaustion never bypasses safety checks or converts failure into success.
- Fail-closed mode blocks destructive actions until explicit recovery conditions pass.

