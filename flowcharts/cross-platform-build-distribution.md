# Cross-Platform Build and Distribution Flowchart

This flowchart captures the build matrix, artifact packaging, checksum generation, smoke tests, and fail-closed release gate for Linux, macOS, and Windows.

## Build Matrix Flow

```mermaid
flowchart TD
    A[Start release build request] --> B[Select Linux, macOS, and Windows targets]
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
    C --> D[Run platform smoke tests]
    D --> E{Did every smoke test pass?}
    E -- No --> F[Block release publication]
    E -- Yes --> G[Mark build set releasable]
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
