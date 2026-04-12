# Flowcharts Folder

This folder stores standalone flowchart documents for runtime, safety, and workflow visualization.

## Contents

- Mermaid diagrams for daemon runtime flow
- Safety decision trees
- Agent orchestration workflows
- PR and release process diagrams
- Phase 1 alignment notes: project layout, config loading, backend interfaces, baseline tests
- `phase-1-core-skeleton.md`: standalone Phase 1 architecture and fail-closed Mermaid flowcharts
- `phase-2-policy-engine.md`: standalone Phase 2 candidate filtering and determinism flowcharts
- `phase-3-planner-executor.md`: standalone Phase 3 planning/execution safety flowcharts
- `phase-4-scheduler-watermark-loop.md`: standalone Phase 4 scheduler watermark loop and fail-closed exit flowcharts
- `phase-4-pr-test-gate.md`: standalone PR-to-main CI gate and merge rule flowchart
- `phase-5-docker-backend.md`: standalone Phase 5 Docker backend discovery/execution safety flowcharts

## Rules

- Keep diagrams synchronized with AGENTS.md and requirement.md.
- Any behavior change MUST update the related flowchart.
- Safety diagrams should reflect fail-closed behavior and dry-run as the default path.
