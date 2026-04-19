# Cross-Platform Build + Distribution

## Purpose

Define a repeatable build and release path for Linux, macOS, and Windows so the daemon can be distributed with the same safety model on every supported platform.

This feature makes the CircleCI build matrix, packaging rules, artifact integrity checks, and release gate explicit before any binary is published.

## Cross-Platform Build Matrix

- Linux builds produce the primary server binary and any release-side helpers required for packaging.
- macOS builds verify the daemon compiles and packages cleanly on Apple hosts.
- Windows builds verify the daemon compiles and packages cleanly on Windows hosts.
- The `.circleci/config.yml` workflow `cross-platform-build-distribution` defines these targets as required jobs.
- Each target in the matrix must be treated as required unless the release scope explicitly narrows the supported platforms.
- The matrix is complete only when every declared OS target has a successful build result and a recorded smoke test result.

## Artifact Packaging and Upload

- Build outputs must be packaged per platform so the release artifact is easy to install and verify.
- Linux artifacts are packaged as `.deb` packages with stable filenames.
- Linux `.deb` package includes:
  - `/usr/bin/prune-guard` daemon executable
  - `/etc/prune-guard/prune-guard.toml` config template
  - `/lib/systemd/system/prune-guard.service` long-running daemon service unit
  - `/lib/systemd/system/prune-guard.timer` bootstrapping timer unit
- Linux `.deb` packaging must exclude the recursive `target/release` build tree to keep artifacts small, deterministic, and resilient in CI.
- macOS artifacts remain packaged as archive files with stable filenames.
- Windows artifacts are packaged as both `.zip` archives and `.exe` installers using `scripts/release/package-artifacts.ps1`.
- Windows `.exe` installers are compiled from `packaging/windows/prune-guard-installer.iss`.
- Windows installer flow must use the classic wizard style so users get explicit Next-button navigation.
- Windows installer flow must show an explicit optional task to add the install directory (binary path) to system `PATH`.
- Windows packaging must include `prune-guard.exe`, release metadata, installer payload, and checksum files.
- Packaging must keep the release payload minimal and deterministic.
- Artifact upload must happen only after the packaged bytes and checksum manifest are ready.
- CircleCI stores packaged artifacts per platform job so each target’s output is auditable independently.
- Any missing artifact is treated as an incomplete release, not a partial success.

## GitHub Release Publication

- Release publication is tag-driven (`v*`) and runs only after Linux, macOS, and Windows packaging jobs complete successfully.
- CircleCI collects `.deb`, `.tar.gz`, `.zip`, and Windows installer `.exe` artifacts plus matching `.sha256` files from all platform jobs.
- The `github-release-publish` job creates a GitHub release when the tag is new, or uploads with overwrite semantics when the release already exists.
- Publication commands resolve the repository from CircleCI project metadata and do not rely on local `.git` checkout state.
- GitHub release publication is fail-closed:
  - missing artifacts block publication
  - missing checksums block publication
  - missing GitHub token blocks publication
- Publication must never proceed on branch-only pipelines without an explicit version tag.

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

- Platform build summary covering Linux, macOS, and Windows
- Linux `.deb` package and checksum
- Packaged release archive for macOS target
- Packaged release `.zip` for Windows target
- Packaged installer `.exe` for Windows target
- Artifact upload summary showing where each packaged binary was published
- GitHub release URL containing Linux/macOS/Windows binaries and checksums
- Checksum manifest for all packaged artifacts
- Smoke test results for each matrix entry
- Release gate summary showing whether publication was approved or blocked
