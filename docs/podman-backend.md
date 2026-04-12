# Podman Backend

## Purpose

Deliver a Podman adapter with Docker-parity contract behavior and graceful degradation.

The implementation is in `src/podman_backend.rs`.

## Backend Behavior

### Health Check

- Runs `podman version --format {{.Server.Version}}`.
- Returns healthy when a non-empty version is available.
- Gracefully degrades (returns unhealthy report) when Podman is unavailable or returns empty version output.
- Avoids hard errors for unavailable Podman so scheduler can no-op safely.

### Usage Collection

- Reads Podman graph root via `podman info --format {{.Store.GraphRoot}}`.
- Reads byte usage from `df -B1 --output=used,size <graph-root>`.
- Emits `UsageSnapshot` with backend `Podman` and computed `used_percent`.
- Fails closed with `CleanupError::UsageCollectionFailed` if usage data is missing or unparsable.

### Candidate Discovery

- Discovers containers, images, and volumes via Podman inspect/list commands.
- Converts resources into `CandidateArtifact` with safety metadata:
  - `in_use`
  - `referenced`
  - `age_days`
  - `metadata_complete`
  - `metadata_ambiguous`
- Safety mapping:
  - Running containers are `in_use = true`.
  - Images used by containers are `referenced = true`.
  - Volumes attached to containers are `referenced = true`.
  - Volumes attached to running containers are `in_use = true`.
- Ambiguous metadata is emitted as incomplete/ambiguous so policy fails closed.

### Action Execution

- Honors dry-run by returning a synthetic response without delete commands.
- On real execution, re-validates safety before delete:
  - block running container deletion
  - block referenced image deletion
  - block attached volume deletion
- Safety blocks return `CleanupError::SafetyViolation`.
- Delete command failures return `CleanupError::ExecutionFailed`.

## Safety Rationale

- Pre-delete re-validation prevents stale plans from deleting resources that became active or referenced after planning.
- Metadata ambiguity is treated as non-actionable, preserving fail-closed behavior.
- Podman unavailability is reported as unhealthy instead of a hard failure to support graceful backend degradation.

## Test Coverage Added

`tests/podman_backend_tests.rs` covers:

- healthy Podman path
- unavailable Podman graceful degradation path
- usage collection via Podman graph root and `df`
- discovery safety tagging for running/referenced/attached resources
- ambiguous metadata handling
- execution blocks for running containers, referenced images, attached volumes
- dry-run execution safety (no delete commands)
