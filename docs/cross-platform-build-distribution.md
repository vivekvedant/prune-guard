# Cross-Platform Build + Distribution

## Purpose

Define a repeatable build and release path for Linux and macOS so the daemon can be distributed with the same safety model on every supported platform.

This feature makes the CircleCI build matrix, packaging rules, artifact integrity checks, and release gate explicit before any binary is published.

## Cross-Platform Build Matrix

- Linux builds produce the primary server binary and any release-side helpers required for packaging.
- macOS builds verify the daemon compiles and packages cleanly on Apple hosts.
- The `.circleci/config.yml` workflow `cross-platform-build-distribution` defines these targets as required jobs.
- Each target in the matrix must be treated as required unless the release scope explicitly narrows the supported platforms.
- The matrix is complete only when every declared OS target has a successful build result and a recorded smoke test result.

## Artifact Packaging and Upload

- Build outputs must be packaged per platform so the release artifact is easy to install and verify.
- Linux artifacts are packaged as `.deb` packages with stable filenames.
- Linux `.deb` package includes:
  - `/usr/bin/prune-guard` daemon executable
  - `/etc/prune-guard/prune-guard.toml` config template
  - `/lib/systemd/system/prune-guard.service` oneshot service unit
  - `/lib/systemd/system/prune-guard.timer` recurring timer unit
- Linux `.deb` packaging must exclude the recursive `target/release` build tree to keep artifacts small, deterministic, and resilient in CI.
- macOS artifacts remain packaged as archive files with stable filenames.
- Packaging must keep the release payload minimal and deterministic.
- Artifact upload must happen only after the packaged bytes and checksum manifest are ready.
- CircleCI stores packaged artifacts per platform job so each target’s output is auditable independently.
- Any missing artifact is treated as an incomplete release, not a partial success.

## Checksums and Integrity

- Every published artifact must have a checksum entry.
- Checksum generation must happen after packaging so the published digest matches the final artifact bytes.
- Release notes should point operators to the checksum file or checksum block associated with each artifact.
- A checksum mismatch blocks publication and forces the release back into fail-closed review.

## Smoke Test Gate

- Each platform build must run a smoke test before the artifact can be considered releasable.
- Smoke tests verify that the packaged binary starts and performs the minimum safe runtime checks expected on that platform.
- A failing smoke test invalidates that target even if compilation succeeded.
- A release cannot proceed while any target remains unverified.

## Fail-Closed Release Policy

- The release gate must refuse publication if any matrix entry fails, times out, or produces an incomplete artifact.
- The release gate must refuse publication if checksums are missing or inconsistent.
- The release gate must refuse publication if smoke tests do not complete on every supported target.
- Dry-run remains the default review mode; real release publication requires an explicit, fully passing build report.
- If the build or packaging state is ambiguous, the safest behavior is to stop and skip publication.

## Safety Rationale

- Cross-platform distribution is a release-risk problem, not just a portability problem.
- Explicit matrix requirements prevent one platform from silently drifting out of the supported set.
- Checksums and smoke tests reduce the chance that a corrupt or partially built artifact reaches users.
- Fail-closed gating keeps the default outcome conservative when any target is missing, broken, or under-specified.

## Expected Release Artifacts

- Platform build summary covering Linux and macOS
- Linux `.deb` package and checksum
- Packaged release archive for macOS target
- Artifact upload summary showing where each packaged binary was published
- Checksum manifest for all packaged artifacts
- Smoke test results for each matrix entry
- Release gate summary showing whether publication was approved or blocked
