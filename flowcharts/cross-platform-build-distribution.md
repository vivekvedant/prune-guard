# Cross-Platform Build and Distribution Flowchart

This flowchart captures the CircleCI build matrix, Linux `.deb` packaging, Windows `.zip` plus `.exe` installer packaging, checksum generation, smoke tests, and fail-closed release gate for Linux, macOS, and Windows.

## Build Matrix Flow

```mermaid
flowchart TD
    A[Start CircleCI release workflow] --> B[Select Linux macOS and Windows targets]
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
    C --> D[Verify Linux .deb payload and Windows .zip and .exe payloads]
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
    D -- Yes --> F[Create or update GitHub release for tag]
    F --> G[Upload Linux macOS and Windows artifacts plus checksums]
    G --> H[Record checksums and smoke test results]
```

## Safety Notes

- Dry-run is the default path until every supported platform has passed build, packaging, checksum, upload, and smoke test steps.
- Any missing target or missing checksum blocks publication.
- Linux `.deb` packaging must include only install-time payload files and must not embed the full `target/release` build tree.
- Windows `.zip` and `.exe` installer packaging must include non-empty release binaries and checksum output.
- Windows installer workflow must use classic wizard navigation and explicitly prompt whether to add the install binary path to system PATH.
- Windows packaging must canonicalize installer output paths before verification so ISCC output location and CI checks stay aligned.
- GitHub release publication must run only for version tags and must fail closed when any asset is missing.
- Release publication should use explicit CircleCI project metadata for repository selection instead of depending on local git checkout state.
- Any ambiguity in artifact integrity or smoke-test status must be treated as a release stop, not a warning.
- Fail-closed release gating is required so partial platform coverage cannot be mistaken for a complete distribution.
