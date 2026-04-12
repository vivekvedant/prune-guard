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
- `phase-3-planner-executor.md`: Phase 3 planning, delete-cap enforcement, and safe execution behavior
- `phase-4-scheduler-watermark-loop.md`: Phase 4 scheduler contract, watermark stop conditions, and fail-closed behavior
- `phase-4-pr-test-gate.md`: PR-to-main CI test gate and merge-protection guidance
- `phase-5-docker-backend.md`: Phase 5 Docker adapter behavior, safety checks, and test coverage
- `phase-6-podman-backend.md`: Phase 6 Podman adapter parity behavior, graceful degradation, and test coverage
- `phase-7-reliability-error-handling.md`: Phase 7 retry/backoff, partial-failure continuation, locking, and fail-closed reliability behavior
- `phase-8-observability-security-portability.md`: Phase 8 structured logs, redaction, metrics hooks, least-privilege checks, and OS validation

## Rules

- Keep documents scoped and versioned with code changes.
- Safety-critical behavior changes MUST include matching docs updates.
- Safety guidance should remain fail-closed and default to dry-run behavior unless explicitly overridden.
