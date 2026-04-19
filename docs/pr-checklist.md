# Pull Request Checklist

## Purpose

Use this checklist to confirm that a cleanup-daemon change is safe, documented, and ready for review.

## Checklist

- [ ] The branch matches the planned phase or release scope.
- [ ] The PR targets `main`.
- [ ] The change scope is small and focused.
- [ ] The phase documentation is present and current.
- [ ] The matching flowchart is present and current.
- [ ] The release runbook is present and current.
- [ ] The PR checklist itself is present and current.
- [ ] Default runtime mode is documented accurately (including whether dry-run must be explicitly enabled).
- [ ] Fail-closed behavior is documented for all safety-sensitive paths.
- [ ] Any skip/no-op decision is explained in the docs.
- [ ] Any runtime guard or preflight behavior is explained in the docs.
- [ ] Any release limitation or follow-up is called out explicitly.
- [ ] The reviewer does not need private context to understand the safety model.

## Review Notes

- If a box cannot be checked, treat the PR as not ready.
- Do not rely on verbal assurances when the docs are missing or unclear.
- If the code and docs disagree, the safer interpretation applies until the mismatch is fixed.

## Release Notes Template

Use this structure in the PR description:

- What changed
- Why it changed
- What safety checks protect the behavior
- Whether runtime defaults changed and how dry-run is enabled when needed
- Whether any follow-up work remains

## Safety Rationale

- A checklist makes release readiness auditable.
- Explicit runtime-default, dry-run, and fail-closed checks prevent reviewers from assuming unsafe defaults.
- Requiring documentation parity reduces the risk of merging code with incomplete operator guidance.
