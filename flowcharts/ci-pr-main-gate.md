# CI PR Main Gate Flowchart

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
    A[CircleCI job starts] --> B{Branch is main push?}
    B -- Yes --> C[Workflow filtered out]
    B -- No --> D{PR URL available via CIRCLE_PULL_REQUEST or CIRCLE_PULL_REQUESTS?}
    D -- No --> E[step halt + exit 0]
    D -- Yes --> F[Fetch PR metadata from GitHub API]
    F --> G{Metadata fetch/parse succeeded?}
    G -- No --> H[exit 1 fail-closed]
    G -- Yes --> I{PR base.ref == main?}
    I -- No --> E
    I -- Yes --> J[Pull cimg/rust:1.94]
    J --> K{Image manifest available?}
    K -- No --> L[Fail job early and stop execution]
    K -- Yes --> M[Run cargo test --all-targets --all-features --locked]
    M --> N{Tests passed?}
    N -- No --> O[Fail gate]
    N -- Yes --> P[Pass gate]
```

Notes:

- Pinning the image tag avoids non-deterministic alias resolution failures.
- If image pull cannot be resolved, the pipeline stops without running partial validation.
- OAuth-safe guard logic scopes execution to pull requests targeting `main`.
- Non-targeted runs halt cleanly, while ambiguous PR metadata fails the job for real PR contexts.
