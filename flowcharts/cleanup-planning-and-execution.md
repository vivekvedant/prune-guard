# Cleanup Planning and Execution Flowcharts

This document captures planning and execution safety behavior.

## 1) Planning Gate (Fail-Closed + Delete Cap)

```mermaid
flowchart TD
    A[Input Candidates] --> B[PolicyEngine Evaluate]
    B --> C{Policy Accepted?}
    C -- No --> R1[Skip: policy reject reason]
    C -- Yes --> D{Candidate Backend Matches Plan Backend?}
    D -- No --> R2[Skip: candidate_backend_mismatch]
    D -- Yes --> E{size_bytes known?}
    E -- No --> E1{unknown-size fallback budget available?}
    E1 -- No --> R3[Skip: deletion_cap_reached]
    E1 -- Yes --> E2[Reserve full remaining delete budget]
    E2 --> G
    E -- Yes --> F{size_bytes <= remaining_delete_budget?}
    F -- No --> F1{build-cache candidate and remaining budget > 0?}
    F1 -- Yes --> F2[Cap action size to remaining budget]
    F2 --> G
    F1 -- No --> R4[Skip: deletion_cap_reached]
    F -- Yes --> G[Create PlannedAction Delete]
    G --> H[Subtract size from remaining budget]
```

## 2) Execution Gate (Dry-Run + Timeout Wrapper)

```mermaid
flowchart TD
    A[PlannedAction] --> B{Plan dry_run OR Action dry_run?}
    B -- Yes --> C[Emit Synthetic Dry-Run Response]
    B -- No --> D[Call backend.execute in worker thread]
    D --> E{Result before timeout?}
    E -- Yes + Success --> F[Append to completed]
    E -- Yes + Error --> G[Append failure and continue]
    E -- No --> H[Append timeout failure and continue]
```

## 3) Batch Continuation Behavior

```mermaid
flowchart LR
    A[Action 1] --> B[Execute]
    B --> C[Record completed/failure]
    C --> D[Action 2]
    D --> E[Execute]
    E --> F[Record completed/failure]
    F --> G[Repeat to End]
    G --> H[Emit ExecutionReport]
```

Notes:

- Dry-run paths never invoke backend delete.
- Timeout or backend error for one action never aborts remaining actions.
- Planner and executor both preserve deterministic input order.
