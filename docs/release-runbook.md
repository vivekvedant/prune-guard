# Release Runbook

## Purpose

Provide the release operator with a concrete, repeatable path for shipping a cleanup-daemon branch safely.

This runbook is intentionally conservative. If any step is incomplete or ambiguous, stop and treat the release as not ready.

## Scope

Use this runbook for branches that are intended to merge into `main` and change runtime behavior, docs, or operational guidance.

## Pre-Release Checks

1. Confirm the branch name matches the phase or release scope.
1. Confirm the PR target is `main`.
1. Confirm the release is still aligned with `plan.md`.
1. Confirm the docs set includes the relevant phase note, flowchart, runbook, and PR checklist.
1. Confirm that runtime defaults are documented accurately, including how dry-run is enabled.
1. Confirm that fail-closed behavior is documented for every safety-critical decision point.

## Release Procedure

### 1. Review the Phase Docs

- Read the phase documentation for the branch scope.
- Confirm the docs explain what changed, why it changed, and how safety is preserved.
- If the docs do not explain a safety decision, stop and revise them before proceeding.

### 2. Validate the Flowcharts

- Read the matching flowchart page.
- Confirm the diagram shows the actual runtime or release path.
- Confirm unsafe paths terminate in skip, no-op, or fail-closed outcomes.
- Confirm runtime defaults and dry-run guidance are explicit for the reviewed flow.

### 3. Use the PR Checklist

- Walk the checklist before opening the PR.
- Mark any missing item as blocking.
- Do not convert a blocking item into a comment without actually fixing or documenting it.

### 4. Prepare the PR

- Summarize what changed.
- Explain why the change is needed.
- State the safety considerations explicitly.
- Call out any release constraint, follow-up, or known limitation.

### 5. Re-check Before Merge

- Re-run the checklist if the branch changes after PR creation.
- Verify that the docs and flowcharts still match the code and plan.
- If the release scope changed, restart the review rather than reusing an outdated approval.

## Release Exit Criteria

- Documentation is complete and aligned with the implementation phase.
- Flowcharts represent the current fail-closed behavior.
- Runtime defaults and dry-run guidance are explicit in the docs.
- No unresolved safety question remains.
- The PR is ready to merge without relying on hidden context.

## Abort Conditions

Stop the release if any of the following is true:

- The branch is not the intended release branch.
- The target is not `main`.
- The phase docs and flowcharts disagree.
- Runtime default or dry-run behavior is unclear.
- Fail-closed behavior is unclear.
- A safety-sensitive decision is undocumented.

## Safety Rationale

- A release runbook prevents operators from improvising unsafe steps during merge preparation.
- Re-checking docs and flowcharts guards against drift between implementation and release guidance.
- Abort conditions are necessary because silence or ambiguity is unsafe in a deletion-capable system.
- Explicit runtime-default and dry-run documentation reduces operator ambiguity.
