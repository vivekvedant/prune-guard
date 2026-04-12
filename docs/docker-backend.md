# Docker Backend

## Purpose

Deliver a production-facing Docker adapter that implements backend contract stages:

- Health check
- Usage collection
- Candidate discovery
- Action execution

The implementation is in `src/docker_backend.rs`.

## Backend Behavior

### Health Check

- Runs `docker version --format {{.Server.Version}}`.
- Reports healthy only when a non-empty server version is returned.
- Fails closed with `CleanupError::HealthCheckFailed` on command or parse failures.

### Usage Collection

- Reads Docker root dir via `docker info --format {{.DockerRootDir}}`.
- Reads byte-accurate used/total values using `df -B1 --output=used,size <docker-root>`.
- Emits `UsageSnapshot` with backend `Docker` and computed `used_percent`.
- Fails closed with `CleanupError::UsageCollectionFailed` if any signal is missing or unparsable.

### Candidate Discovery

- Discovers containers, images, and volumes through Docker CLI inspect/list commands.
- Converts each resource into `CandidateArtifact` with safety metadata:
  - `in_use`
  - `referenced`
  - `age_days`
  - `metadata_complete`
  - `metadata_ambiguous`
- For safety:
  - Running containers are marked `in_use = true`.
  - Images used by containers are marked `referenced = true`.
  - Volumes mounted by any container are marked `referenced = true`.
  - Volumes mounted by running containers are marked `in_use = true`.
- Any ambiguous or missing critical metadata is emitted as incomplete/ambiguous, so policy/planner fail closed and skip deletion.

### Action Execution

- Honors dry-run first: returns synthetic non-executed response and does not call delete commands.
- On real execution, re-validates safety before delete:
  - Container delete is blocked if container is running.
  - Image delete is blocked if image is referenced by any container.
  - Volume delete is blocked if volume is attached to any container.
- Safety blocks return `CleanupError::SafetyViolation`.
- Delete command failures return `CleanupError::ExecutionFailed`.

## Safety Rationale

- Safety validation runs both during discovery and again immediately before deletion.
- Pre-delete re-validation prevents stale-plan races from deleting resources that became active/referenced after planning.
- Metadata parsing is fail-closed: uncertain fields become non-actionable candidates rather than optimistic deletes.
- Command execution uses direct process args (not shell interpolation), reducing command injection risk.

## Test Coverage Added

`tests/docker_backend_tests.rs` covers:

- Healthy and unavailable Docker daemon paths.
- Usage collection parsing from Docker root + `df`.
- Discovery marking for:
  - running containers
  - referenced images
  - attached volumes
  - ambiguous metadata
- Execution safety guards:
  - running containers are never deleted
  - referenced images are never deleted
  - dry-run performs no delete command
