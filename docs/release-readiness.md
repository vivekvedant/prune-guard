# Release Readiness

## Purpose

Finalize the documentation set required to ship cleanup-daemon changes safely.

This phase aligns:

- `docs/`
- `flowcharts/`
- the release runbook
- the PR checklist

The goal is to make the release path explicit, repeatable, and fail-closed before a change is merged to `main`.

## Release Readiness Behavior

### Documentation Alignment

- The release docs describe the same runtime behavior as the implementation phases.
- The runbook and checklist are treated as required operating artifacts, not optional notes.
- Feature docs remain versioned alongside code so reviewers can trace the intended safety model.
- If documentation and implementation disagree, the safest interpretation wins and the release is blocked until the mismatch is resolved.

### Fail-Closed Release Policy

- Any missing safety explanation is a release blocker.
- Any unclear deletion path is treated as unsafe.
- Any inconsistency between docs, flowcharts, and plan scope is treated as a failure condition.
- The release process must prefer skipping a merge over publishing incomplete operational guidance.

### Runtime Default and Dry-Run Guidance

- Real execution is the default operational mode unless `dry_run = true` is set explicitly.
- The runbook must call out when dry-run validation is required before destructive release steps.
- PR review must verify that documentation states runtime defaults clearly and does not imply hidden safety modes.
- If documented defaults are ambiguous, the release is fail-closed and requires revision.

## Finalized Runbook Usage

### When to Use the Runbook

- Use `docs/release-runbook.md` when preparing a branch for merge to `main`.
- Use it after implementation, before approval, and again if release scope changes.
- Use it to confirm that operational guidance matches the current branch contents.

### What the Runbook Must Verify

- Branch name and target PR are correct.
- Release scope matches the phase plan.
- Safety defaults are still fail-closed, with dry-run available as an explicit validation mode.
- Relevant tests and checks are documented as passing or intentionally deferred.
- Any deviation from the release path is explicitly recorded.

## Finalized PR Checklist Usage

### When to Use the Checklist

- Use `docs/pr-checklist.md` before opening a PR and again before merge.
- Use it to confirm that the release package is complete, reviewable, and safe.
- Use it as the source of truth for reviewer-facing release readiness.

### What the Checklist Must Confirm

- Documentation updates are included.
- Flowcharts reflect the current runtime and release workflow.
- Safety rationale is present for any deletion-sensitive behavior.
- Runtime defaults, dry-run usage, and fail-closed behavior are explicit.
- The branch can be merged without requiring hidden tribal knowledge.

## Safety Rationale

- Release readiness is a documentation problem as much as a code problem in a safety-critical system.
- Explicit runbooks and checklists reduce the chance that a reviewer assumes unsafe defaults.
- Fail-closed release rules prevent unclear or partial guidance from being treated as approved operating practice.
- Clear runtime-default and dry-run documentation keeps operator behavior auditable unless a reviewer can prove otherwise.

## Expected Documentation Set

- `docs/release-runbook.md`
- `docs/pr-checklist.md`
- `docs/release-readiness.md`
- `flowcharts/release-readiness.md`
- updated `docs/README.md`
- updated `flowcharts/README.md`
