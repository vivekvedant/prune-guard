# Flowcharts

This folder stores standalone flowchart documents for runtime, safety, and workflow visualization.

## Contents

- Mermaid diagrams for daemon runtime flow
- Safety decision trees
- Agent orchestration workflows
- PR and release process diagrams
- `core-architecture.md`: architecture contracts, config loading, backend interfaces, and fail-closed baseline flowcharts
- `policy-engine.md`: candidate filtering and deterministic policy flowcharts
- `cleanup-planning-and-execution.md`: planning, execution, and dry-run safety flowcharts
- `scheduler-watermark-loop.md`: scheduler watermark loop and fail-closed exit flowcharts
- `ci-pr-main-gate.md`: PR-to-main CI gate and merge rule flowchart
- `docker-backend.md`: Docker backend discovery and execution safety flowcharts
- `podman-backend.md`: Podman backend parity and graceful-degradation flowcharts
- `reliability-and-error-handling.md`: retry/backoff, lock, and fail-closed reliability flowcharts
- `observability-security-portability.md`: structured logging, preflight security, and portability flowcharts
- `release-readiness.md`: documentation alignment, release runbook, and PR checklist flowcharts
- `cross-platform-build-distribution.md`: cross-platform build matrix, artifact packaging, checksums, and fail-closed release gate flowcharts
- `docker-storage-growth-helper.md`: local `test_bin.sh` loop that intentionally increases Docker storage with fail-closed preflight checks

## Rules

- Keep diagrams synchronized with AGENTS.md and requirement.md.
- Any behavior change MUST update the related flowchart.
- Safety diagrams should reflect fail-closed behavior and dry-run as the default path.
- Release-readiness diagrams should show the review gate, documentation alignment, and abort path explicitly.
