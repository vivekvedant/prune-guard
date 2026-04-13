# Docs Folder

This folder contains feature-oriented documentation beyond top-level requirement and process files.

## Contents

- Architecture notes
- Runbooks
- Decision records
- Operational procedures
- Core architecture notes: project layout, config loading, backend interfaces, baseline tests
- `core-architecture.md`: architecture, safety defaults, contracts, and test coverage
- `policy-engine.md`: fail-closed policy filtering, reject reasons, and test coverage
- `cleanup-planning-and-execution.md`: planning, delete-cap enforcement, and safe execution behavior
- `scheduler-watermark-loop.md`: scheduler contract, watermark stop conditions, and fail-closed behavior
- `ci-pr-main-gate.md`: PR-to-main CI test gate and merge-protection guidance
- `docker-backend.md`: Docker adapter behavior, safety checks, and test coverage
- `podman-backend.md`: Podman adapter parity behavior, graceful degradation, and test coverage
- `reliability-and-error-handling.md`: retry/backoff, partial-failure continuation, locking, and fail-closed reliability behavior
- `observability-security-portability.md`: structured logs, redaction, metrics hooks, least-privilege checks, and OS validation
- `release-readiness.md`: documentation alignment, release readiness rules, dry-run default, and fail-closed release guidance
- `cross-platform-build-distribution.md`: Linux/macOS build matrix, Linux `.deb` packaging with daemon+systemd install payload, checksums, and fail-closed release gating
- `release-runbook.md`: Release operator runbook for merge readiness, documentation checks, and abort conditions
- `pr-checklist.md`: Reviewer-facing PR checklist for safety-critical release readiness

## Rules

- Keep documents scoped and versioned with code changes.
- Safety-critical behavior changes MUST include matching docs updates.
- Safety guidance should remain fail-closed and default to dry-run behavior unless explicitly overridden.
- Release-ready changes should include the runbook and checklist entries used to verify the merge path.
