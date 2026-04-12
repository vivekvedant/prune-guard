# Policy Engine Flowcharts

This document captures the candidate policy gate and deterministic decision order used by `PolicyEngine`.

## 1) Candidate Selection Gate (Fail-Closed)

```mermaid
flowchart TD
    A[CandidateArtifact] --> B{metadata_complete == true?}
    B -- No --> R1[Skip: metadata_incomplete]
    B -- Yes --> C{metadata_ambiguous == false?}
    C -- No --> R2[Skip: metadata_ambiguous]
    C -- Yes --> D{protected == false?}
    D -- No --> R3[Skip: candidate_marked_protected]
    D -- Yes --> E{Image/Volume Allowlist Match?}
    E -- Yes --> R4[Skip: protected_image_or_volume_allowlist]
    E -- No --> F{Protected Label Match?}
    F -- Yes --> R5[Skip: protected_label_allowlist]
    F -- No --> G{in_use == Some false?}
    G -- No --> R6[Skip: candidate_in_use_or_unknown]
    G -- Yes --> H{referenced == Some false?}
    H -- No --> R7[Skip: candidate_referenced_or_unknown]
    H -- Yes --> I{age_days present?}
    I -- No --> R8[Skip: candidate_age_unknown]
    I -- Yes --> J{age_days >= min_unused_age_days?}
    J -- No --> R9[Skip: candidate_too_new]
    J -- Yes --> K[Accept Candidate]
```

## 2) Batch Evaluation Determinism

```mermaid
flowchart LR
    A[Input Candidates in Discovery Order] --> B[Evaluate Candidate 1]
    B --> C[Append to accepted or skipped]
    C --> D[Evaluate Candidate 2]
    D --> E[Append to accepted or skipped]
    E --> F[Repeat Until End]
    F --> G[Emit PolicyEvaluation]
```

Notes:
- Accepted and skipped lists preserve original input order.
- Exactly one reject reason is emitted per skipped candidate.
