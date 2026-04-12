# Docs Folder

This folder contains project documentation beyond top-level requirement and process files.

## Contents

- Architecture notes
- Runbooks
- Decision records
- Operational procedures
- Phase 1 implementation notes: project layout, config loading, backend interfaces, baseline tests
- `phase-1-core-skeleton.md`: Phase 1 architecture, safety defaults, contracts, and test coverage
- `phase-2-policy-engine.md`: Phase 2 fail-closed policy filtering, reject reasons, and test coverage

## Rules

- Keep documents scoped and versioned with code changes.
- Safety-critical behavior changes MUST include matching docs updates.
- Safety guidance should remain fail-closed and default to dry-run behavior unless explicitly overridden.
