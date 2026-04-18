# Docker Backend

## Purpose

Deliver a production-facing Docker adapter that implements backend contract stages:

- Health check
- Usage collection
- Candidate discovery
- Action execution

The implementation is in `src/docker_backend.rs`.

## Backend Behavior

### Docker Endpoint Selection

- The daemon reads optional Docker CLI connection overrides from config:
  - `[docker].host`
  - `[docker].context`
- Exactly one may be set; setting both is rejected during config validation (fail-closed).
- When neither override is set, startup attempts host auto-detection from known local socket paths:
  - `unix:///var/run/docker.sock`
  - `unix:///run/docker.sock`
  - `unix://$HOME/.docker/desktop/docker.sock`
  - `unix:///home/*/.docker/desktop/docker.sock`
- Each discovered candidate is probed with `docker --host <candidate> version --format {{.Server.Version}}`.
- Auto-detection is applied only when exactly one candidate is reachable.
- If multiple candidates are reachable, startup fails closed and requires explicit `docker.host` or `docker.context` in TOML.
- When set, the backend prepends the corresponding global CLI flag to every Docker command:
  - `--host <value>`
  - `--context <value>`
- This keeps `systemd` unit files static while still allowing users to target non-default Docker daemons (for example Docker Desktop sockets) through `/etc/prune-guard/prune-guard.toml`.

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

- Discovers containers, images, volumes, and aggregate build-cache cleanup candidates through Docker CLI inspect/list commands.
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
- Image discovery has a fail-closed template fallback:
  - First attempt inspects labels via `.Config.Labels`.
  - If Docker returns the known template error (`map has no entry for key "Labels"`), discovery retries with a labels-free inspect template.
  - Default behavior is fail-closed: when label-based safety cannot be proven, fallback candidates are emitted with `metadata_complete = false` / `metadata_ambiguous = true` so policy skips deletion.
  - If `protected_labels` is empty, fallback candidates remain actionable because label-based protection is not required.
  - Optional compatibility mode: set `allow_missing_image_labels = true` to run a second labels check with `{{json .Config.Labels}}`.
  - In compatibility mode, labels are treated as safely empty only when that JSON output is exactly `null`; any other output or command failure remains fail-closed.
- Any ambiguous or missing critical metadata is emitted as incomplete/ambiguous, so policy/planner fail closed and skip deletion.
- Volume discovery enriches `size_bytes` using `docker system df -v` volume output so delete-cap enforcement can remain bounded.
- Build cache discovery uses `docker system df -v` build-cache output:
  - only non-in-use entries at/above `min_unused_age_days` are considered
  - candidate size is aggregated from eligible entries
  - if any eligible entry has ambiguous size/age metadata, candidate is marked incomplete and skipped fail-closed

### Action Execution

- Honors dry-run first: returns synthetic non-executed response and does not call delete commands.
- On real execution, re-validates safety before delete:
  - Container delete is blocked if container is running.
  - Image delete is blocked if image is referenced by any container.
  - Image reference detection first uses `docker ps -a --format {{.ImageID}}`.
  - If Docker does not support `.ImageID` in that template, execution falls back to:
    - `docker ps -a -q --no-trunc`
    - `docker container inspect --format {{.Image}} <container-id>`
  - Fallback inspect output must include an image reference for every container; otherwise execution fails closed.
  - Volume delete is blocked if volume is attached to any container.
  - Build cache delete uses `docker builder prune -f` and adds an `until=<hours>` filter when age metadata is available.
  - When planner caps a build-cache action size for delete-budget enforcement, execution applies reclaim budget controls (`--max-used-space`, with `--keep-storage` fallback) so oversized cache sets can be reduced incrementally across runs.
- Execution handles stale-plan races idempotently:
  - If a container disappears between `docker ps -a` and `docker container inspect` during image/volume guards, that container is ignored as stale.
  - If delete returns explicit "No such container/image/volume", action is treated as safe no-op success instead of a hard backend failure.
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
  - image inspect fallback when labels are unavailable
- Execution safety guards:
  - running containers are never deleted
  - referenced images are never deleted
  - referenced images are still blocked when `docker ps` template output does not provide `.ImageID`
  - build-cache prune execution path and age-filtered prune path
  - dry-run performs no delete command
