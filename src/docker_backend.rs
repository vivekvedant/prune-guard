use crate::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use crate::domain::{
    BackendKind, CandidateArtifact, CandidateDiscoveryRequest, CandidateDiscoveryResponse,
    ExecutionMode, ExecutionRequest, ExecutionResponse, HealthReport, ResourceKind, UsageSnapshot,
};
use crate::error::{CleanupError, Result};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const CONTAINER_INSPECT_TEMPLATE: &str =
    "{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}";
const IMAGE_INSPECT_TEMPLATE: &str =
    "{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}";
const IMAGE_INSPECT_TEMPLATE_NO_LABELS: &str =
    "{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t";
const VOLUME_INSPECT_TEMPLATE: &str =
    "{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}";
const VOLUME_SIZE_TEMPLATE: &str = "{{range .Volumes}}{{println .Name \"\\t\" .Size}}{{end}}";
const BUILD_CACHE_TEMPLATE: &str =
    "{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}";
const BUILD_CACHE_CANDIDATE_ID: &str = "docker-build-cache-unused";

/// Command execution abstraction used by Docker backend operations.
///
/// A trait is used so tests can inject deterministic fake outputs without
/// requiring a live Docker daemon.
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> std::result::Result<String, String>;
}

/// OS command runner for production backend behavior.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::result::Result<String, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| format!("failed to execute `{program}`: {error}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(format!(
                "`{program}` exited with code {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

/// Docker backend adapter for Phase 5.
///
/// Safety notes:
/// - discovery marks running/referenced/attached resources explicitly
/// - uncertain metadata is flagged as incomplete/ambiguous so policy fails closed
/// - execution re-validates safety conditions before issuing delete commands
pub struct DockerBackend<R: CommandRunner = OsCommandRunner> {
    runner: R,
    docker_connection_args: Vec<String>,
}

impl DockerBackend<OsCommandRunner> {
    pub fn new() -> Self {
        Self::with_runner(OsCommandRunner)
    }

    pub fn with_connection(
        host: Option<String>,
        context: Option<String>,
    ) -> std::result::Result<Self, String> {
        let (resolved_host, resolved_context) =
            resolve_docker_connection_from_environment(&OsCommandRunner, host, context)?;
        Ok(Self::with_runner_and_connection(
            OsCommandRunner,
            resolved_host,
            resolved_context,
        ))
    }
}

impl Default for DockerBackend<OsCommandRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> DockerBackend<R> {
    pub fn with_runner(runner: R) -> Self {
        Self::with_runner_and_connection(runner, None, None)
    }

    pub fn with_runner_and_connection(
        runner: R,
        host: Option<String>,
        context: Option<String>,
    ) -> Self {
        Self {
            runner,
            docker_connection_args: docker_connection_args(host, context),
        }
    }

    fn run_docker(&self, args: &[&str]) -> std::result::Result<String, String> {
        let mut command_args = Vec::with_capacity(self.docker_connection_args.len() + args.len());
        command_args.extend(self.docker_connection_args.iter().map(String::as_str));
        command_args.extend(args.iter().copied());
        self.runner.run("docker", &command_args)
    }

    fn run_df(&self, root_dir: &str) -> std::result::Result<String, String> {
        self.runner
            .run("df", &["-B1", "--output=used,size", root_dir])
    }

    fn run_windows_drive_usage_probe(
        &self,
        drive_letter: char,
    ) -> std::result::Result<String, String> {
        // Windows does not provide `df`, so we query the drive that contains
        // DockerRootDir and compute used/total bytes fail-closed.
        let script = format!(
            "$d=Get-PSDrive -Name {drive_letter}; if ($null -eq $d) {{ exit 1 }}; Write-Output ($d.Used); Write-Output ($d.Used + $d.Free)"
        );
        self.runner.run(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", script.as_str()],
        )
    }

    fn collect_windows_usage_from_root(
        &self,
        root_dir: &str,
    ) -> std::result::Result<(u64, u64), String> {
        let drive_letter = extract_windows_drive_letter(root_dir)
            .ok_or_else(|| "docker root directory is not a windows drive path".to_string())?;
        let powershell_output = self.run_windows_drive_usage_probe(drive_letter)?;
        parse_windows_usage(&powershell_output)
            .ok_or_else(|| "could not parse windows powershell usage output".to_string())
    }

    fn collect_container_metadata_raw(
        &self,
    ) -> std::result::Result<Vec<ContainerMetadata>, String> {
        let container_ids = self
            .run_docker(&["ps", "-a", "-q", "--no-trunc"])
            .map_err(|message| format!("failed listing containers: {message}"))?;

        let mut containers = Vec::new();
        for id in non_empty_lines(&container_ids) {
            let inspect = self.run_docker(&[
                "container",
                "inspect",
                "--size",
                "--format",
                CONTAINER_INSPECT_TEMPLATE,
                id,
            ]);
            match inspect {
                Ok(output) => containers.push(parse_container_metadata(id, &output)),
                Err(message) if is_missing_container_error(&message) => {
                    // Container disappeared between listing and inspect.
                    // Treat this as stale metadata and continue safely.
                }
                Err(message) => {
                    return Err(format!("failed to inspect container `{id}`: {message}"));
                }
            }
        }

        Ok(containers)
    }

    fn ensure_container_not_running(&self, container_id: &str) -> Result<()> {
        let inspect = match self.run_docker(&[
            "container",
            "inspect",
            "--size",
            "--format",
            CONTAINER_INSPECT_TEMPLATE,
            container_id,
        ]) {
            Ok(output) => output,
            Err(message) if is_missing_container_error(&message) => {
                // Container disappeared before delete execution.
                // Allow idempotent delete behavior to proceed.
                return Ok(());
            }
            Err(message) => {
                return Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("failed to inspect container `{container_id}`: {message}"),
                });
            }
        };

        let container = parse_container_metadata(container_id, &inspect);
        match container.running {
            Some(true) => Err(CleanupError::SafetyViolation {
                message: format!("refusing to delete running container `{container_id}`"),
            }),
            Some(false) => Ok(()),
            None => Err(CleanupError::SafetyViolation {
                message: format!(
                    "refusing to delete container `{container_id}` because running-state metadata is ambiguous"
                ),
            }),
        }
    }

    fn ensure_image_not_referenced(&self, image_id: &str) -> Result<()> {
        let referenced_image_ids = self.collect_referenced_image_ids()?;

        let normalized_target = normalize_image_id(image_id);
        let referenced = referenced_image_ids.contains(&normalized_target);

        if referenced {
            Err(CleanupError::SafetyViolation {
                message: format!("refusing to delete referenced image `{image_id}`"),
            })
        } else {
            Ok(())
        }
    }

    fn collect_referenced_image_ids(&self) -> Result<HashSet<String>> {
        match self.run_docker(&["ps", "-a", "--format", "{{.ImageID}}"]) {
            Ok(output) => Ok(non_empty_lines(&output)
                .into_iter()
                .map(normalize_image_id)
                .collect()),
            Err(message) => {
                if !is_unsupported_image_id_template_error(&message) {
                    return Err(CleanupError::ExecutionFailed {
                        backend: BackendKind::Docker,
                        message: format!("failed to discover image references: {message}"),
                    });
                }

                // Compatibility fallback for Docker variants that do not expose
                // `.ImageID` in `docker ps --format`. We inspect each container
                // directly and fail closed if any image reference is ambiguous.
                let container_ids =
                    self.run_docker(&["ps", "-a", "-q", "--no-trunc"])
                        .map_err(|fallback_message| CleanupError::ExecutionFailed {
                            backend: BackendKind::Docker,
                            message: format!(
                                "failed to discover image references via fallback container listing: {fallback_message}"
                            ),
                        })?;
                let mut referenced_image_ids = HashSet::new();
                for container_id in non_empty_lines(&container_ids) {
                    let inspect = match self.run_docker(&[
                        "container",
                        "inspect",
                        "--format",
                        "{{.Image}}",
                        container_id,
                    ]) {
                        Ok(output) => output,
                        Err(fallback_message) if is_missing_container_error(&fallback_message) => {
                            // Container disappeared between fallback listing and inspect.
                            // Treat this as stale metadata and continue safely.
                            continue;
                        }
                        Err(fallback_message) => {
                            return Err(CleanupError::ExecutionFailed {
                                backend: BackendKind::Docker,
                                message: format!(
                                    "failed to inspect image reference for container `{container_id}`: {fallback_message}"
                                ),
                            });
                        }
                    };
                    let image_reference = first_non_empty_line(&inspect).ok_or_else(|| {
                        CleanupError::ExecutionFailed {
                            backend: BackendKind::Docker,
                            message: format!(
                                "failed to inspect image reference for container `{container_id}`: empty image reference output"
                            ),
                        }
                    })?;
                    referenced_image_ids.insert(normalize_image_id(image_reference));
                }
                Ok(referenced_image_ids)
            }
        }
    }

    fn ensure_volume_not_attached(&self, volume_name: &str) -> Result<()> {
        let containers = self.collect_container_metadata_raw().map_err(|message| {
            CleanupError::ExecutionFailed {
                backend: BackendKind::Docker,
                message: format!("failed collecting container mounts for volume guard: {message}"),
            }
        })?;
        let attached = containers
            .iter()
            .any(|container| container.mount_names.contains(volume_name));

        if attached {
            Err(CleanupError::SafetyViolation {
                message: format!("refusing to delete attached volume `{volume_name}`"),
            })
        } else {
            Ok(())
        }
    }

    fn prune_build_cache(&self, candidate: &CandidateArtifact) -> Result<()> {
        if candidate.identifier != BUILD_CACHE_CANDIDATE_ID {
            return Err(CleanupError::ExecutionFailed {
                backend: BackendKind::Docker,
                message: format!(
                    "unexpected build cache candidate identifier `{}`",
                    candidate.identifier
                ),
            });
        }

        let mut base_args = vec!["builder".to_string(), "prune".to_string(), "-f".to_string()];
        if let Some(age_days) = candidate.age_days {
            if age_days > 0 {
                base_args.push("--filter".to_string());
                base_args.push(format!("until={}h", age_days.saturating_mul(24)));
            }
        }

        let mut keep_bytes_budget: Option<u64> = None;
        if let Some(target_reclaim_bytes) = candidate.size_bytes {
            if target_reclaim_bytes > 0 {
                let min_age_days = candidate.age_days.unwrap_or(0);
                let prunable_bytes = self
                    .collect_build_cache_prunable_size_bytes(min_age_days)
                    .map_err(|message| CleanupError::ExecutionFailed {
                        backend: BackendKind::Docker,
                        message: format!(
                            "failed to compute build cache prune budget before execution: {message}"
                        ),
                    })?;
                if prunable_bytes > target_reclaim_bytes {
                    keep_bytes_budget = Some(prunable_bytes.saturating_sub(target_reclaim_bytes));
                }
            }
        }

        let mut args = base_args.clone();
        if let Some(keep_bytes) = keep_bytes_budget {
            args.push("--max-used-space".to_string());
            args.push(keep_bytes.to_string());
        }

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.run_docker(&arg_refs) {
            Ok(_) => Ok(()),
            Err(message)
                if keep_bytes_budget.is_some() && is_unknown_max_used_space_flag(&message) =>
            {
                let keep_bytes = keep_bytes_budget.unwrap_or(0);
                let mut fallback_args = base_args;
                fallback_args.push("--keep-storage".to_string());
                fallback_args.push(keep_bytes.to_string());
                let fallback_refs: Vec<&str> = fallback_args.iter().map(String::as_str).collect();
                self.run_docker(&fallback_refs)
                    .map(|_| ())
                    .map_err(|fallback_error| CleanupError::ExecutionFailed {
                        backend: BackendKind::Docker,
                        message: format!(
                            "build cache prune failed: primary_error={message}; fallback_error={fallback_error}"
                        ),
                    })
            }
            Err(message) => Err(CleanupError::ExecutionFailed {
                backend: BackendKind::Docker,
                message: format!("build cache prune failed: {message}"),
            }),
        }
    }

    fn delete_resource(&self, kind: &ResourceKind, identifier: &str) -> Result<()> {
        let command_error = match kind {
            ResourceKind::Container => self.run_docker(&["container", "rm", identifier]),
            ResourceKind::Image => self.run_docker(&["image", "rm", identifier]),
            ResourceKind::Volume => self.run_docker(&["volume", "rm", identifier]),
            ResourceKind::BuildCache => self.run_docker(&["builder", "prune", "-f"]),
            ResourceKind::Unknown(kind) => {
                return Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("unsupported resource kind `{kind}`"),
                });
            }
        };

        command_error.map(|_| ()).or_else(|message| {
            if is_missing_delete_target(kind, &message) {
                // Resource was already removed between planning and execution.
                // This is safe idempotent behavior, not a runtime failure.
                Ok(())
            } else {
                Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("delete command failed for `{identifier}`: {message}"),
                })
            }
        })
    }
}

fn docker_connection_args(host: Option<String>, context: Option<String>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(host) = normalize_non_empty(host) {
        args.push("--host".to_string());
        args.push(host);
    }
    if let Some(context) = normalize_non_empty(context) {
        args.push("--context".to_string());
        args.push(context);
    }
    args
}

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_docker_connection_from_environment<R: CommandRunner>(
    runner: &R,
    host: Option<String>,
    context: Option<String>,
) -> std::result::Result<(Option<String>, Option<String>), String> {
    let auto_detect_candidates = discover_auto_detect_candidate_hosts();
    resolve_docker_connection(runner, host, context, &auto_detect_candidates)
}

fn resolve_docker_connection<R: CommandRunner>(
    runner: &R,
    host: Option<String>,
    context: Option<String>,
    auto_detect_candidates: &[String],
) -> std::result::Result<(Option<String>, Option<String>), String> {
    let host = normalize_non_empty(host);
    let context = normalize_non_empty(context);
    if host.is_some() && context.is_some() {
        return Err("docker.host and docker.context cannot both be set".to_string());
    }

    if host.is_some() || context.is_some() {
        return Ok((host, context));
    }

    // Safety-critical behavior:
    // auto-detect only when exactly one reachable endpoint is proven.
    // If multiple endpoints are reachable, fail closed and require explicit config.
    let detected_host = select_reachable_auto_detected_host(runner, auto_detect_candidates)?;
    Ok((detected_host, None))
}

fn select_reachable_auto_detected_host<R: CommandRunner>(
    runner: &R,
    auto_detect_candidates: &[String],
) -> std::result::Result<Option<String>, String> {
    let mut reachable_hosts = Vec::new();
    for host in auto_detect_candidates {
        let output = runner.run(
            "docker",
            &["--host", host, "version", "--format", "{{.Server.Version}}"],
        );
        if let Ok(version) = output {
            if !version.trim().is_empty() {
                reachable_hosts.push(host.clone());
            }
        }
    }

    if reachable_hosts.is_empty() {
        return Ok(None);
    }

    let mut reachable_desktop_hosts = Vec::new();
    let mut reachable_non_desktop_hosts = Vec::new();
    for host in reachable_hosts {
        if is_docker_desktop_host(host.as_str()) {
            reachable_desktop_hosts.push(host);
        } else {
            reachable_non_desktop_hosts.push(host);
        }
    }

    // Safety-critical endpoint routing:
    // if Docker Desktop is reachable, prefer it only when a single desktop
    // endpoint is unambiguous. Any ambiguous set still fails closed.
    match reachable_desktop_hosts.len() {
        1 => return Ok(reachable_desktop_hosts.into_iter().next()),
        2.. => {
            return Err(format!(
                "multiple reachable Docker Desktop hosts were auto-detected ({}); configure exactly one via `docker.host` or `docker.context`",
                reachable_desktop_hosts.join(", ")
            ));
        }
        _ => {}
    }

    match reachable_non_desktop_hosts.len() {
        1 => Ok(reachable_non_desktop_hosts.into_iter().next()),
        _ => Err(format!(
            "multiple reachable Docker hosts were auto-detected ({}); configure exactly one via `docker.host` or `docker.context`",
            reachable_non_desktop_hosts.join(", ")
        )),
    }
}

fn is_docker_desktop_host(host: &str) -> bool {
    host.ends_with("/.docker/desktop/docker.sock")
}

fn discover_auto_detect_candidate_hosts() -> Vec<String> {
    let mut hosts = BTreeSet::new();
    push_unix_socket_host_candidate(Path::new("/var/run/docker.sock"), &mut hosts);
    push_unix_socket_host_candidate(Path::new("/run/docker.sock"), &mut hosts);

    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            let desktop_socket = Path::new(home).join(".docker/desktop/docker.sock");
            push_unix_socket_host_candidate(&desktop_socket, &mut hosts);
        }
    }

    if let Ok(home_entries) = fs::read_dir("/home") {
        for entry in home_entries.flatten() {
            let desktop_socket = entry.path().join(".docker/desktop/docker.sock");
            push_unix_socket_host_candidate(&desktop_socket, &mut hosts);
        }
    }

    hosts.into_iter().collect()
}

fn push_unix_socket_host_candidate(path: &Path, hosts: &mut BTreeSet<String>) {
    if let Some(host) = unix_socket_host_uri(path) {
        hosts.insert(host);
    }
}

fn unix_socket_host_uri(path: &Path) -> Option<String> {
    if !is_unix_socket(path) {
        return None;
    }

    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some(format!("unix://{}", canonical.display()))
}

fn is_unix_socket(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        return fs::metadata(path)
            .map(|metadata| metadata.file_type().is_socket())
            .unwrap_or(false);
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn is_missing_container_error(message: &str) -> bool {
    message.contains("No such container")
}

fn is_missing_delete_target(kind: &ResourceKind, message: &str) -> bool {
    match kind {
        ResourceKind::Container => message.contains("No such container"),
        ResourceKind::Image => message.contains("No such image"),
        ResourceKind::Volume => message.contains("No such volume"),
        ResourceKind::BuildCache | ResourceKind::Unknown(_) => false,
    }
}

fn is_unknown_max_used_space_flag(message: &str) -> bool {
    message.contains("unknown flag: --max-used-space")
}

impl<R: CommandRunner> HealthCheck for DockerBackend<R> {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Docker
    }

    fn health_check(&self) -> Result<HealthReport> {
        let output = self
            .run_docker(&["version", "--format", "{{.Server.Version}}"])
            .map_err(|message| CleanupError::HealthCheckFailed {
                backend: BackendKind::Docker,
                message,
            })?;

        if output.trim().is_empty() {
            return Err(CleanupError::HealthCheckFailed {
                backend: BackendKind::Docker,
                message: "docker server version was empty".to_string(),
            });
        }

        Ok(HealthReport::healthy(BackendKind::Docker))
    }
}

impl<R: CommandRunner> UsageCollector for DockerBackend<R> {
    fn collect_usage(&self) -> Result<UsageSnapshot> {
        let root_dir = self
            .run_docker(&["info", "--format", "{{.DockerRootDir}}"])
            .map_err(|message| CleanupError::UsageCollectionFailed {
                backend: BackendKind::Docker,
                message,
            })?;

        let root_dir = root_dir.trim();
        if root_dir.is_empty() {
            return Err(CleanupError::UsageCollectionFailed {
                backend: BackendKind::Docker,
                message: "docker root directory was empty".to_string(),
            });
        }

        let (used_bytes, total_bytes) = match self.run_df(root_dir) {
            Ok(df_output) => match parse_df_usage(&df_output) {
                Some(usage) => usage,
                None => self.collect_windows_usage_from_root(root_dir).map_err(
                    |windows_fallback_error| CleanupError::UsageCollectionFailed {
                        backend: BackendKind::Docker,
                        message: format!(
                            "could not parse df usage output for `{root_dir}` and windows fallback failed: {windows_fallback_error}"
                        ),
                    },
                )?,
            },
            Err(df_error) => self.collect_windows_usage_from_root(root_dir).map_err(
                |windows_fallback_error| CleanupError::UsageCollectionFailed {
                    backend: BackendKind::Docker,
                    message: format!(
                        "failed reading disk usage for `{root_dir}`: primary_error={df_error}; windows_fallback_error={windows_fallback_error}"
                    ),
                },
            )?,
        };

        let used_percent = if total_bytes > 0 {
            Some(((used_bytes.saturating_mul(100)) / total_bytes) as u8)
        } else {
            None
        };

        Ok(UsageSnapshot {
            backend: BackendKind::Docker,
            used_bytes,
            total_bytes: Some(total_bytes),
            used_percent,
            observed_at: Some(SystemTime::now()),
        })
    }
}

impl<R: CommandRunner> CandidateDiscoverer for DockerBackend<R> {
    fn discover_candidates(
        &self,
        request: CandidateDiscoveryRequest,
    ) -> Result<CandidateDiscoveryResponse> {
        if request.backend != BackendKind::Docker {
            return Err(CleanupError::UnsupportedBackend {
                backend: request.backend,
                message: "docker backend received non-docker discovery request".to_string(),
            });
        }

        let now = SystemTime::now();
        let containers = self.collect_container_metadata_raw().map_err(|message| {
            CleanupError::CandidateDiscoveryFailed {
                backend: BackendKind::Docker,
                message,
            }
        })?;

        let referenced_image_ids: HashSet<String> = containers
            .iter()
            .map(|container| normalize_image_id(&container.image_id))
            .collect();
        let attached_volume_names: HashSet<String> = containers
            .iter()
            .flat_map(|container| container.mount_names.iter().cloned())
            .collect();
        let running_attached_volume_names: HashSet<String> = containers
            .iter()
            .filter(|container| container.running == Some(true))
            .flat_map(|container| container.mount_names.iter().cloned())
            .collect();

        let mut candidates = Vec::new();

        for container in containers {
            candidates.push(container.into_candidate(now));
        }

        let image_ids = self
            .run_docker(&["image", "ls", "-q", "--no-trunc"])
            .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                backend: BackendKind::Docker,
                message,
            })?;

        for image_id in non_empty_lines(&image_ids) {
            let inspect = self
                .run_docker(&["image", "inspect", "--format", IMAGE_INSPECT_TEMPLATE, image_id])
                .map(|output| (output, true))
                .or_else(|message| {
                    if is_missing_image_labels_error(&message) {
                        // Fail closed: if labels cannot be inspected due template shape
                        // differences, continue discovery but mark the image metadata
                        // incomplete so policy skips deletion when label-based
                        // protection is required.
                        self.run_docker(&[
                            "image",
                            "inspect",
                            "--format",
                            IMAGE_INSPECT_TEMPLATE_NO_LABELS,
                            image_id,
                        ])
                        .map(|output| (output, false))
                        .map_err(|fallback_message| {
                            CleanupError::CandidateDiscoveryFailed {
                                backend: BackendKind::Docker,
                                message: format!(
                                    "failed to inspect image `{image_id}` after labels-template fallback: primary_error={message}; fallback_error={fallback_message}"
                                ),
                            }
                        })
                    } else {
                        Err(CleanupError::CandidateDiscoveryFailed {
                            backend: BackendKind::Docker,
                            message: format!("failed to inspect image `{image_id}`: {message}"),
                        })
                    }
                })?;

            candidates.push(parse_image_candidate(
                &inspect.0,
                now,
                &referenced_image_ids,
                inspect.1,
                !request.config.protected_labels.is_empty(),
            ));
        }

        let volume_names_raw = self
            .run_docker(&["volume", "ls", "-q"])
            .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                backend: BackendKind::Docker,
                message,
            })?;
        let volume_names: Vec<&str> = non_empty_lines(&volume_names_raw);
        let volume_sizes = if volume_names.is_empty() {
            BTreeMap::new()
        } else {
            self.collect_volume_size_map().map_err(|message| {
                CleanupError::CandidateDiscoveryFailed {
                    backend: BackendKind::Docker,
                    message,
                }
            })?
        };

        for volume_name in volume_names {
            let inspect = self
                .run_docker(&[
                    "volume",
                    "inspect",
                    "--format",
                    VOLUME_INSPECT_TEMPLATE,
                    volume_name,
                ])
                .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                    backend: BackendKind::Docker,
                    message: format!("failed to inspect volume `{volume_name}`: {message}"),
                })?;

            candidates.push(parse_volume_candidate(
                &inspect,
                now,
                &attached_volume_names,
                &running_attached_volume_names,
                &volume_sizes,
            ));
        }

        if let Some(build_cache_candidate) = self
            .build_build_cache_candidate(now, request.config.min_unused_age_days)
            .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                backend: BackendKind::Docker,
                message,
            })?
        {
            candidates.push(build_cache_candidate);
        }

        Ok(CandidateDiscoveryResponse {
            backend: BackendKind::Docker,
            candidates,
        })
    }
}

impl<R: CommandRunner> ExecutionContract for DockerBackend<R> {
    fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResponse> {
        let ExecutionRequest {
            backend,
            action,
            mode,
        } = request;

        if backend != BackendKind::Docker {
            return Err(CleanupError::UnsupportedBackend {
                backend,
                message: "docker backend received non-docker execution request".to_string(),
            });
        }

        if matches!(mode, ExecutionMode::DryRun) || action.dry_run {
            return Ok(ExecutionResponse {
                backend: BackendKind::Docker,
                candidate: action.candidate,
                executed: false,
                dry_run: true,
                message: Some("dry_run_no_delete_executed".to_string()),
            });
        }

        if !matches!(action.kind, crate::domain::CleanupActionKind::Delete) {
            return Err(CleanupError::ExecutionFailed {
                backend: BackendKind::Docker,
                message: "unsupported action kind for docker backend".to_string(),
            });
        }

        match action.candidate.resource_kind {
            ResourceKind::Container => {
                self.ensure_container_not_running(&action.candidate.identifier)?
            }
            ResourceKind::Image => {
                self.ensure_image_not_referenced(&action.candidate.identifier)?
            }
            ResourceKind::Volume => {
                self.ensure_volume_not_attached(&action.candidate.identifier)?
            }
            ResourceKind::BuildCache => self.prune_build_cache(&action.candidate)?,
            ResourceKind::Unknown(ref kind) => {
                return Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("cannot execute unknown resource kind `{kind}`"),
                });
            }
        }

        if !matches!(action.candidate.resource_kind, ResourceKind::BuildCache) {
            self.delete_resource(
                &action.candidate.resource_kind,
                &action.candidate.identifier,
            )?;
        }

        Ok(ExecutionResponse {
            backend: BackendKind::Docker,
            candidate: action.candidate,
            executed: true,
            dry_run: false,
            message: Some("delete_executed".to_string()),
        })
    }
}

impl<R: CommandRunner> DockerBackend<R> {
    fn collect_volume_size_map(&self) -> std::result::Result<BTreeMap<String, u64>, String> {
        let output = self
            .run_docker(&["system", "df", "-v", "--format", VOLUME_SIZE_TEMPLATE])
            .map_err(|message| format!("failed collecting docker volume sizes: {message}"))?;
        let mut volume_sizes = BTreeMap::new();

        for line in non_empty_lines(&output) {
            let mut fields = line.split('\t').map(str::trim);
            let Some(name) = fields.next() else {
                continue;
            };
            let Some(size_raw) = fields.next() else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            if let Some(size_bytes) = parse_human_size_bytes(size_raw) {
                volume_sizes.insert(name.to_string(), size_bytes);
            }
        }

        Ok(volume_sizes)
    }

    fn build_build_cache_candidate(
        &self,
        now: SystemTime,
        min_unused_age_days: u64,
    ) -> std::result::Result<Option<CandidateArtifact>, String> {
        let output = self
            .run_docker(&["system", "df", "-v", "--format", BUILD_CACHE_TEMPLATE])
            .map_err(|message| {
                format!("failed collecting docker build cache details: {message}")
            })?;

        let mut eligible_entries = 0usize;
        let mut eligible_unknown_metadata = false;
        let mut total_size_bytes: u64 = 0;
        let mut min_eligible_age_days: Option<u64> = None;

        for line in non_empty_lines(&output) {
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            let identifier = fields.first().copied().unwrap_or_default();
            if identifier.is_empty() {
                continue;
            }

            let size_bytes = fields
                .get(1)
                .and_then(|value| parse_human_size_bytes(value));
            let in_use = fields.get(2).and_then(|value| parse_bool(value));
            let last_used_at = fields.get(3).copied().unwrap_or_default();
            let created_at = fields.get(4).copied().unwrap_or_default();
            let age_source = if !last_used_at.is_empty() && last_used_at != "<nil>" {
                last_used_at
            } else {
                created_at
            };
            let age_days = parse_age_days(age_source, now);

            let is_eligible = matches!(in_use, Some(false))
                && age_days
                    .map(|age| age >= min_unused_age_days)
                    .unwrap_or(false);
            if !is_eligible {
                continue;
            }

            eligible_entries += 1;
            match (size_bytes, age_days) {
                (Some(size), Some(age)) => {
                    total_size_bytes = total_size_bytes.saturating_add(size);
                    min_eligible_age_days = Some(match min_eligible_age_days {
                        Some(current) => current.min(age),
                        None => age,
                    });
                }
                _ => {
                    eligible_unknown_metadata = true;
                }
            }
        }

        if eligible_entries == 0 {
            return Ok(None);
        }

        let metadata_complete =
            !eligible_unknown_metadata && total_size_bytes > 0 && min_eligible_age_days.is_some();
        Ok(Some(CandidateArtifact {
            backend: BackendKind::Docker,
            resource_kind: ResourceKind::BuildCache,
            identifier: BUILD_CACHE_CANDIDATE_ID.to_string(),
            display_name: Some("docker-build-cache".to_string()),
            labels: BTreeSet::new(),
            size_bytes: if total_size_bytes > 0 {
                Some(total_size_bytes)
            } else {
                None
            },
            age_days: min_eligible_age_days,
            in_use: Some(false),
            referenced: Some(false),
            protected: false,
            metadata_complete,
            metadata_ambiguous: !metadata_complete,
            discovered_at: Some(now),
        }))
    }

    fn collect_build_cache_prunable_size_bytes(
        &self,
        min_unused_age_days: u64,
    ) -> std::result::Result<u64, String> {
        let output = self
            .run_docker(&["system", "df", "-v", "--format", BUILD_CACHE_TEMPLATE])
            .map_err(|message| {
                format!("failed collecting docker build cache details: {message}")
            })?;
        let now = SystemTime::now();
        let mut total_size_bytes: u64 = 0;

        for line in non_empty_lines(&output) {
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            let identifier = fields.first().copied().unwrap_or_default();
            if identifier.is_empty() {
                continue;
            }

            let in_use = fields
                .get(2)
                .and_then(|value| parse_bool(value))
                .ok_or_else(|| {
                    format!("build cache entry `{identifier}` has unknown in-use metadata")
                })?;
            if in_use {
                continue;
            }

            let last_used_at = fields.get(3).copied().unwrap_or_default();
            let created_at = fields.get(4).copied().unwrap_or_default();
            let age_source = if !last_used_at.is_empty() && last_used_at != "<nil>" {
                last_used_at
            } else {
                created_at
            };
            let age_days = parse_age_days(age_source, now).ok_or_else(|| {
                format!("build cache entry `{identifier}` has unknown age metadata")
            })?;
            if age_days < min_unused_age_days {
                continue;
            }

            let size_bytes = fields
                .get(1)
                .and_then(|value| parse_human_size_bytes(value))
                .ok_or_else(|| {
                    format!("build cache entry `{identifier}` has unknown size metadata")
                })?;
            total_size_bytes = total_size_bytes.saturating_add(size_bytes);
        }

        Ok(total_size_bytes)
    }
}

#[derive(Debug, Clone)]
struct ContainerMetadata {
    id: String,
    name: Option<String>,
    running: Option<bool>,
    image_id: String,
    size_bytes: Option<u64>,
    age_days: Option<u64>,
    labels: BTreeSet<String>,
    mount_names: BTreeSet<String>,
}

impl ContainerMetadata {
    fn into_candidate(self, now: SystemTime) -> CandidateArtifact {
        let age_days = self.age_days;
        let metadata_complete = !self.id.is_empty()
            && self.running.is_some()
            && self.size_bytes.is_some()
            && age_days.is_some();
        CandidateArtifact {
            backend: BackendKind::Docker,
            resource_kind: ResourceKind::Container,
            identifier: self.id,
            display_name: self.name,
            labels: self.labels,
            size_bytes: self.size_bytes,
            age_days,
            in_use: self.running,
            referenced: Some(false),
            protected: false,
            metadata_complete,
            metadata_ambiguous: !metadata_complete,
            discovered_at: Some(now),
        }
    }
}

fn parse_container_metadata(fallback_id: &str, inspect_output: &str) -> ContainerMetadata {
    let line = first_non_empty_line(inspect_output).unwrap_or_default();
    let fields: Vec<&str> = line.split('\t').collect();

    let id = fields
        .first()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_id)
        .to_string();
    let name = fields
        .get(1)
        .map(|value| value.trim().trim_start_matches('/'))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let running = fields.get(2).and_then(|value| parse_bool(value.trim()));
    let age_days = fields
        .get(3)
        .and_then(|created| parse_age_days(created.trim(), SystemTime::now()));
    let image_id = fields
        .get(4)
        .map(|value| value.trim())
        .unwrap_or_default()
        .to_string();
    let size_bytes = fields
        .get(5)
        .and_then(|value| value.trim().parse::<i64>().ok())
        .and_then(|value| u64::try_from(value).ok());
    let labels = fields
        .get(6)
        .map(|value| parse_semicolon_entries(value.trim()))
        .unwrap_or_default();
    let mount_names = fields
        .get(7)
        .map(|value| parse_semicolon_entries(value.trim()))
        .unwrap_or_default();

    ContainerMetadata {
        id,
        name,
        running,
        image_id,
        size_bytes,
        age_days,
        labels,
        mount_names,
    }
}

fn parse_image_candidate(
    inspect_output: &str,
    now: SystemTime,
    referenced_image_ids: &HashSet<String>,
    labels_known: bool,
    label_protection_required: bool,
) -> CandidateArtifact {
    let line = first_non_empty_line(inspect_output).unwrap_or_default();
    let fields: Vec<&str> = line.split('\t').collect();

    let identifier = fields
        .first()
        .map(|value| value.trim())
        .unwrap_or_default()
        .to_string();
    let display_name = fields
        .get(1)
        .map(|value| parse_semicolon_list(value.trim()))
        .and_then(|entries| entries.into_iter().next());
    let age_days = fields
        .get(2)
        .and_then(|created| parse_age_days(created.trim(), now));
    let size_bytes = fields
        .get(3)
        .and_then(|value| value.trim().parse::<u64>().ok());
    let labels = fields
        .get(4)
        .map(|value| parse_semicolon_entries(value.trim()))
        .unwrap_or_default();

    let referenced = if identifier.is_empty() {
        None
    } else {
        Some(referenced_image_ids.contains(&normalize_image_id(&identifier)))
    };
    let labels_safety_satisfied = labels_known || !label_protection_required;
    let metadata_complete = !identifier.is_empty()
        && age_days.is_some()
        && size_bytes.is_some()
        && labels_safety_satisfied;

    CandidateArtifact {
        backend: BackendKind::Docker,
        resource_kind: ResourceKind::Image,
        identifier,
        display_name,
        labels,
        size_bytes,
        age_days,
        in_use: Some(false),
        referenced,
        protected: false,
        metadata_complete,
        metadata_ambiguous: !metadata_complete,
        discovered_at: Some(now),
    }
}

fn is_missing_image_labels_error(message: &str) -> bool {
    message.contains("template parsing error")
        && message.contains(".Config.Labels")
        && message.contains("map has no entry for key \"Labels\"")
}

fn is_unsupported_image_id_template_error(message: &str) -> bool {
    message.contains("failed to execute template")
        && message.contains("<.ImageID>")
        && message.contains("can't evaluate field ImageID")
}

fn parse_volume_candidate(
    inspect_output: &str,
    now: SystemTime,
    attached_volume_names: &HashSet<String>,
    running_attached_volume_names: &HashSet<String>,
    volume_sizes: &BTreeMap<String, u64>,
) -> CandidateArtifact {
    let line = first_non_empty_line(inspect_output).unwrap_or_default();
    let fields: Vec<&str> = line.split('\t').collect();

    let identifier = fields
        .first()
        .map(|value| value.trim())
        .unwrap_or_default()
        .to_string();
    let age_days = fields
        .get(1)
        .and_then(|created| parse_age_days(created.trim(), now));
    let labels = fields
        .get(2)
        .map(|value| parse_semicolon_entries(value.trim()))
        .unwrap_or_default();
    let referenced = if identifier.is_empty() {
        None
    } else {
        Some(attached_volume_names.contains(&identifier))
    };
    let in_use = if identifier.is_empty() {
        None
    } else {
        Some(running_attached_volume_names.contains(&identifier))
    };
    let size_bytes = volume_sizes.get(&identifier).copied();

    let metadata_complete = !identifier.is_empty()
        && age_days.is_some()
        && in_use.is_some()
        && referenced.is_some()
        && size_bytes.is_some();

    CandidateArtifact {
        backend: BackendKind::Docker,
        resource_kind: ResourceKind::Volume,
        identifier: identifier.clone(),
        display_name: Some(identifier),
        labels,
        size_bytes,
        age_days,
        in_use,
        referenced,
        protected: false,
        metadata_complete,
        metadata_ambiguous: !metadata_complete,
        discovered_at: Some(now),
    }
}

fn parse_df_usage(df_output: &str) -> Option<(u64, u64)> {
    let data_line = df_output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.to_lowercase().contains("used"))?;
    let mut parts = data_line.split_whitespace();
    let used_bytes = parts.next()?.parse::<u64>().ok()?;
    let total_bytes = parts.next()?.parse::<u64>().ok()?;
    Some((used_bytes, total_bytes))
}

fn parse_windows_usage(output: &str) -> Option<(u64, u64)> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() >= 2 {
        let used_bytes = lines.first()?.parse::<u64>().ok()?;
        let total_bytes = lines.get(1)?.parse::<u64>().ok()?;
        return Some((used_bytes, total_bytes));
    }

    let single_line = lines.first()?;
    let mut parts = single_line.split_whitespace();
    let used_bytes = parts.next()?.parse::<u64>().ok()?;
    let total_bytes = parts.next()?.parse::<u64>().ok()?;
    Some((used_bytes, total_bytes))
}

fn extract_windows_drive_letter(path: &str) -> Option<char> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 {
        return None;
    }

    for index in 0..(bytes.len() - 1) {
        let current = bytes[index];
        let next = bytes[index + 1];
        if current.is_ascii_alphabetic() && next == b':' {
            return Some((current as char).to_ascii_uppercase());
        }
    }

    None
}

fn parse_age_days(created: &str, now: SystemTime) -> Option<u64> {
    let date = created.get(0..10)?;
    let mut segments = date.split('-');
    let year = segments.next()?.parse::<i32>().ok()?;
    let month = segments.next()?.parse::<u32>().ok()?;
    let day = segments.next()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let created_days = days_from_civil(year, month, day)?;
    let now_days = now.duration_since(UNIX_EPOCH).ok()?.as_secs() / 86_400;
    if created_days > now_days {
        return None;
    }
    Some(now_days - created_days)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<u64> {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year / 400
    } else {
        (adjusted_year - 399) / 400
    };
    let year_of_era = adjusted_year - era * 400;
    let month_of_year = month as i32;
    let day_of_year =
        (153 * (month_of_year + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days).ok()
}

fn parse_semicolon_entries(raw: &str) -> BTreeSet<String> {
    parse_semicolon_list(raw).into_iter().collect()
}

fn parse_semicolon_list(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_human_size_bytes(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let split_index = trimmed
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(trimmed.len());
    let (value_raw, unit_raw) = trimmed.split_at(split_index);
    let value = value_raw.parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let multiplier = match unit_raw.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1_000.0,
        "mb" => 1_000_000.0,
        "gb" => 1_000_000_000.0,
        "tb" => 1_000_000_000_000.0,
        "kib" => 1024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };

    let bytes = value * multiplier;
    if !bytes.is_finite() || bytes < 0.0 {
        return None;
    }
    Some(bytes.round() as u64)
}

fn normalize_image_id(image_id: &str) -> String {
    image_id.trim().trim_start_matches("sha256:").to_string()
}

fn non_empty_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn first_non_empty_line(output: &str) -> Option<&str> {
    output.lines().map(str::trim).find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[test]
    fn resolve_connection_uses_explicit_host_without_autodetection() {
        let runner = FakeRunner::new(vec![]);

        let (host, context) = resolve_docker_connection(
            &runner,
            Some("unix:///custom/docker.sock".to_string()),
            None,
            &[
                "unix:///var/run/docker.sock".to_string(),
                "unix:///home/vivek/.docker/desktop/docker.sock".to_string(),
            ],
        )
        .expect("configured host should bypass auto-detection");

        assert_eq!(host.as_deref(), Some("unix:///custom/docker.sock"));
        assert_eq!(context, None);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn resolve_connection_auto_detects_single_reachable_host() {
        let runner = FakeRunner::new(vec![
            ok(
                "docker|--host|unix:///home/vivek/.docker/desktop/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
            err(
                "docker|--host|unix:///var/run/docker.sock|version|--format|{{.Server.Version}}",
                "Cannot connect",
            ),
        ]);

        let (host, context) = resolve_docker_connection(
            &runner,
            None,
            None,
            &[
                "unix:///home/vivek/.docker/desktop/docker.sock".to_string(),
                "unix:///var/run/docker.sock".to_string(),
            ],
        )
        .expect("single reachable host should be auto-detected");

        assert_eq!(
            host.as_deref(),
            Some("unix:///home/vivek/.docker/desktop/docker.sock")
        );
        assert_eq!(context, None);
    }

    #[test]
    fn resolve_connection_fails_closed_when_multiple_non_desktop_hosts_are_reachable() {
        let runner = FakeRunner::new(vec![
            ok(
                "docker|--host|unix:///var/run/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
            ok(
                "docker|--host|unix:///run/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
        ]);

        let error = resolve_docker_connection(
            &runner,
            None,
            None,
            &[
                "unix:///var/run/docker.sock".to_string(),
                "unix:///run/docker.sock".to_string(),
            ],
        )
        .expect_err("multiple non-desktop reachable endpoints must fail closed");

        assert!(error.contains("multiple reachable Docker hosts"));
        assert!(error.contains("docker.host"));
        assert!(error.contains("docker.context"));
    }

    #[test]
    fn resolve_connection_prefers_desktop_host_when_multiple_hosts_are_reachable() {
        let runner = FakeRunner::new(vec![
            ok(
                "docker|--host|unix:///home/vivek/.docker/desktop/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
            ok(
                "docker|--host|unix:///run/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
        ]);

        let (host, context) = resolve_docker_connection(
            &runner,
            None,
            None,
            &[
                "unix:///home/vivek/.docker/desktop/docker.sock".to_string(),
                "unix:///run/docker.sock".to_string(),
            ],
        )
        .expect("desktop host should be preferred deterministically");

        assert_eq!(
            host.as_deref(),
            Some("unix:///home/vivek/.docker/desktop/docker.sock")
        );
        assert_eq!(context, None);
    }

    #[test]
    fn resolve_connection_fails_closed_when_multiple_desktop_hosts_are_reachable() {
        let runner = FakeRunner::new(vec![
            ok(
                "docker|--host|unix:///home/vivek/.docker/desktop/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
            ok(
                "docker|--host|unix:///home/alice/.docker/desktop/docker.sock|version|--format|{{.Server.Version}}",
                "27.1.0\n",
            ),
        ]);

        let error = resolve_docker_connection(
            &runner,
            None,
            None,
            &[
                "unix:///home/vivek/.docker/desktop/docker.sock".to_string(),
                "unix:///home/alice/.docker/desktop/docker.sock".to_string(),
            ],
        )
        .expect_err("multiple desktop hosts should remain fail-closed");

        assert!(error.contains("multiple reachable Docker Desktop hosts"));
        assert!(error.contains("docker.host"));
        assert!(error.contains("docker.context"));
    }

    #[derive(Clone, Default)]
    struct FakeRunner {
        expectations: Arc<Mutex<VecDeque<ExpectedCommand>>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[derive(Clone)]
    struct ExpectedCommand {
        key: String,
        output: std::result::Result<String, String>,
    }

    impl FakeRunner {
        fn new(expectations: Vec<ExpectedCommand>) -> Self {
            Self {
                expectations: Arc::new(Mutex::new(VecDeque::from(expectations))),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock poisoned").clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[&str]) -> std::result::Result<String, String> {
            let key = command_key(program, args);
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(key.clone());

            let mut queue = self
                .expectations
                .lock()
                .expect("expectations lock poisoned");
            let expected = queue
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected command invocation: {key}"));
            assert_eq!(expected.key, key, "command mismatch");
            expected.output
        }
    }

    fn ok(key: &str, output: &str) -> ExpectedCommand {
        ExpectedCommand {
            key: key.to_string(),
            output: Ok(output.to_string()),
        }
    }

    fn err(key: &str, message: &str) -> ExpectedCommand {
        ExpectedCommand {
            key: key.to_string(),
            output: Err(message.to_string()),
        }
    }

    fn command_key(program: &str, args: &[&str]) -> String {
        std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join("|")
    }
}
