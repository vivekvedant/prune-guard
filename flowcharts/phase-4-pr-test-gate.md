# Phase 4 PR Test Gate Flowchart

This diagram captures CI gating for pull requests targeting `main`.

## PR-to-Main Test Gate

```mermaid
flowchart TD
    A[PR targets main] --> B[GitHub Actions workflow starts]
    B --> C[Run cargo test --all-targets --all-features --locked]
    C --> D{All tests passed?}
    D -- No --> E[Status check fails]
    E --> F[Do not merge]
    D -- Yes --> G[Status check passes]
    G --> H{Branch protection requires check?}
    H -- Yes --> I[Merge allowed]
    H -- No --> J[Merge policy not enforced]
```

Notes:

- The workflow produces the test result signal.
- Branch protection on `main` enforces merge blocking on failed checks.

## CircleCI Image Resolution Guard

```mermaid
flowchart TD
    A[Pipeline event received] --> B{event == pull_request?}
    B -- No --> C[Skip workflow]
    B -- Yes --> D{PR base.ref == main?}
    D -- No --> C
    D -- Yes --> E[Start CircleCI test job]
    E --> F[Pull cimg/rust:1.94]
    F --> G{Image manifest available?}
    G -- No --> H[Fail job early and stop execution]
    G -- Yes --> I[Run cargo test --all-targets --all-features --locked]
    I --> J{Tests passed?}
    J -- No --> K[Fail gate]
    J -- Yes --> L[Pass gate]
```

Notes:

- Pinning the image tag avoids non-deterministic alias resolution failures.
- If image pull cannot be resolved, the pipeline stops without running partial validation.
- Workflow conditions scope CircleCI execution to pull requests targeting `main`.
