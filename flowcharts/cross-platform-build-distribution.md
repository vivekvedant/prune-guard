# Cross-Platform Build and Distribution Flowchart

This flowchart captures the CircleCI build matrix, Linux `.deb` packaging, checksum generation, smoke tests, and fail-closed release gate for Linux and macOS.

## Build Matrix Flow

```mermaid
flowchart TD
    A[Start CircleCI release workflow] --> B[Select Linux and macOS targets]
    B --> C[Run per-platform build jobs]
    C --> D{Did every target build succeed?}
    D -- No --> E[Stop and fail closed]
    D -- Yes --> F[Package platform artifacts]
```

## Integrity and Smoke Test Flow

```mermaid
flowchart TD
    A[Package artifacts] --> B[Generate checksums for every artifact]
    B --> C[Artifact upload for packaged binaries and checksum manifest]
    C --> D[Verify Linux .deb includes binary config and systemd service]
    D --> E[Run platform smoke tests]
    E --> F{Did every smoke test pass?}
    F -- No --> G[Block release publication]
    F -- Yes --> H[Mark build set releasable]
```

## Release Gate Flow

```mermaid
flowchart TD
    A[Review build summary] --> B{Are artifacts complete and verified?}
    B -- No --> C[Keep dry-run only]
    B -- Yes --> D{Is publication explicitly approved?}
    D -- No --> E[Remain in dry-run mode]
    D -- Yes --> F[Publish release]
    F --> G[Record checksums and smoke test results]
```

## Safety Notes

- Dry-run is the default path until every supported platform has passed build, packaging, checksum, upload, and smoke test steps.
- Any missing target or missing checksum blocks publication.
- Any ambiguity in artifact integrity or smoke-test status must be treated as a release stop, not a warning.
- Fail-closed release gating is required so partial platform coverage cannot be mistaken for a complete distribution.
