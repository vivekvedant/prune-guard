# Core Architecture Flowcharts

This document captures the architecture-level flow. It represents contracts and safety decisions only; runtime cleanup integration is implemented later.

## 1) Data and Control Skeleton

```mermaid
flowchart TD
    A[Load Config] --> B{Config Valid?}
    B -- No --> C[Return Validation Error]
    B -- Yes --> D[Run Backend Health Check]
    D --> E{Backend Healthy?}
    E -- No --> F[Skip Backend + Report]
    E -- Yes --> G[Collect Usage Snapshot]
    G --> H[Discover Candidates]
    H --> I[Plan Actions]
    I --> J{Dry Run?}
    J -- Yes --> K[Log Planned Actions]
    J -- No --> L[Execute Planned Actions]
```

## 2) Candidate Safety Gate (Fail-Closed)

```mermaid
flowchart TD
    A[CandidateArtifact] --> B{metadata_complete == true?}
    B -- No --> Z[Reject Candidate]
    B -- Yes --> C{metadata_ambiguous == false?}
    C -- No --> Z
    C -- Yes --> D{protected == false?}
    D -- No --> Z
    D -- Yes --> E{in_use == Some false?}
    E -- No --> Z
    E -- Yes --> F{referenced == Some false?}
    F -- No --> Z
    F -- Yes --> G{age_days present?}
    G -- No --> Z
    G -- Yes --> H[Candidate Actionable]
```

## 3) Backend Interface Contract

```mermaid
flowchart LR
    A[HealthCheck] --> B[UsageCollector]
    B --> C[CandidateDiscoverer]
    C --> D[ActionPlanner]
    D --> E[ExecutionContract]
```
