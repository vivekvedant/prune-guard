# Phase 2 Policy Engine (Fail-Closed Selection)

## Purpose

Phase 2 implements deterministic, fail-closed candidate filtering on top of the
Phase 1 domain contracts.

The policy engine is responsible for deciding whether each discovered candidate
is safe to move forward into planning. If any required safety signal is missing
or unsafe, the candidate is skipped.

## Scope Delivered

- `src/policy.rs` policy engine with explicit reject reasons
- single-candidate and batch candidate evaluation APIs
- allowlist checks for protected images, volumes, and labels
- age threshold gate based on `min_unused_age_days`
- deterministic evaluation order and stable output ordering
- full reject/accept path tests in `tests/policy_tests.rs`

## Policy Contract

`PolicyEngine` enforces the following ordered checks for each candidate:

1. metadata must be complete
2. metadata must not be ambiguous
3. candidate must not already be marked protected
4. candidate must not match image/volume allowlists
5. candidate must not match protected labels
6. candidate must be `in_use == Some(false)`
7. candidate must be `referenced == Some(false)`
8. candidate age must be known
9. candidate age must be at least `min_unused_age_days`

## Safety Rationale

- The check order is stable so every rejection emits one deterministic reason.
- Unknown `in_use`, `referenced`, or `age_days` values are treated as unsafe and
  rejected immediately (fail-closed).
- Protected resources are rejected before runtime state checks so allowlist and
  protection intent always win.

## Tests Added

`tests/policy_tests.rs` covers:

- every reject path with a specific reason assertion
- one explicit accept path
- deterministic ordering for batch evaluation results

## Out of Scope in Phase 2

- action planning and deletion cap enforcement
- dry-run/real-run execution behavior
- scheduler watermark loop

## Next Phase Handoff

Phase 3 can consume `PolicyEvaluation.accepted` for planning and
`PolicyEvaluation.skipped` for audit/report output.
