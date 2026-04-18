# Docker Storage Growth Helper

This flowchart documents `test_bin.sh`, a local test helper that intentionally grows Docker storage for cleanup validation scenarios.

Safety baseline:

- Abort immediately if Docker is unavailable (fail-closed).
- Default prune-guard daemon behavior remains dry-run by default.

```mermaid
flowchart TD
    A[Start test_bin.sh] --> B{docker available and daemon reachable?}
    B -- No --> C[Exit immediately fail-closed]
    B -- Yes --> D[Validate numeric env controls]
    D --> E[Pull alpine base image]
    E --> F[Begin loop]
    F --> G{Max iterations reached?}
    G -- Yes --> H[Exit cleanly]
    G -- No --> I[Generate random payload file]
    I --> J[Build unique image no-cache]
    J --> K[Create stopped container]
    K --> L[Create named volume]
    L --> M[Write random bytes into volume]
    M --> N[Print docker system df]
    N --> O[Sleep configured seconds]
    O --> F
```
