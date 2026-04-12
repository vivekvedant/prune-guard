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
    A[CircleCI test job starts] --> B[Pull cimg/rust:1.94]
    B --> C{Image manifest available?}
    C -- No --> D[Fail job early and stop execution]
    C -- Yes --> E[Run cargo test --all-targets --all-features --locked]
    E --> F{Tests passed?}
    F -- No --> G[Fail gate]
    F -- Yes --> H[Pass gate]
```

Notes:

- Pinning the image tag avoids non-deterministic alias resolution failures.
- If image pull cannot be resolved, the pipeline stops without running partial validation.
