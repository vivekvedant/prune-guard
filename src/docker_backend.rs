use crate::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use crate::domain::{
    BackendKind, CandidateArtifact, CandidateDiscoveryRequest, CandidateDiscoveryResponse,
    ExecutionMode, ExecutionRequest, ExecutionResponse, HealthReport, ResourceKind, UsageSnapshot,
};
use crate::error::{CleanupError, Result};
use std::collections::{BTreeSet, HashSet};
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
}

impl DockerBackend<OsCommandRunner> {
    pub fn new() -> Self {
        Self::with_runner(OsCommandRunner)
    }
}

impl Default for DockerBackend<OsCommandRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> DockerBackend<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }

    fn run_docker(&self, args: &[&str]) -> std::result::Result<String, String> {
        self.runner.run("docker", args)
    }

    fn run_df(&self, root_dir: &str) -> std::result::Result<String, String> {
        self.runner
            .run("df", &["-B1", "--output=used,size", root_dir])
    }

    fn collect_container_metadata_raw(&self) -> std::result::Result<Vec<ContainerMetadata>, String> {
        let container_ids = self
            .run_docker(&["ps", "-a", "-q", "--no-trunc"])
            .map_err(|message| format!("failed listing containers: {message}"))?;

        let mut containers = Vec::new();
        for id in non_empty_lines(&container_ids) {
            let inspect = self
                .run_docker(&[
                    "container",
                    "inspect",
                    "--size",
                    "--format",
                    CONTAINER_INSPECT_TEMPLATE,
                    id,
                ])
                .map_err(|message| format!("failed to inspect container `{id}`: {message}"))?;
            containers.push(parse_container_metadata(id, &inspect));
        }

        Ok(containers)
    }

    fn ensure_container_not_running(&self, container_id: &str) -> Result<()> {
        let inspect = self
            .run_docker(&[
                "container",
                "inspect",
                "--size",
                "--format",
                CONTAINER_INSPECT_TEMPLATE,
                container_id,
            ])
            .map_err(|message| CleanupError::ExecutionFailed {
                backend: BackendKind::Docker,
                message: format!("failed to inspect container `{container_id}`: {message}"),
            })?;

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
                    let inspect =
                        self.run_docker(&["container", "inspect", "--format", "{{.Image}}", container_id])
                            .map_err(|fallback_message| CleanupError::ExecutionFailed {
                                backend: BackendKind::Docker,
                                message: format!(
                                    "failed to inspect image reference for container `{container_id}`: {fallback_message}"
                                ),
                            })?;
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

    fn delete_resource(&self, kind: &ResourceKind, identifier: &str) -> Result<()> {
        let command_error = match kind {
            ResourceKind::Container => self.run_docker(&["container", "rm", identifier]),
            ResourceKind::Image => self.run_docker(&["image", "rm", identifier]),
            ResourceKind::Volume => self.run_docker(&["volume", "rm", identifier]),
            ResourceKind::Unknown(kind) => {
                return Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("unsupported resource kind `{kind}`"),
                });
            }
        };

        command_error.map(|_| ()).map_err(|message| CleanupError::ExecutionFailed {
            backend: BackendKind::Docker,
            message: format!("delete command failed for `{identifier}`: {message}"),
        })
    }
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

        let df_output = self
            .run_df(root_dir)
            .map_err(|message| CleanupError::UsageCollectionFailed {
                backend: BackendKind::Docker,
                message: format!("failed reading disk usage for `{root_dir}`: {message}"),
            })?;

        let (used_bytes, total_bytes) = parse_df_usage(&df_output).ok_or_else(|| {
            CleanupError::UsageCollectionFailed {
                backend: BackendKind::Docker,
                message: "could not parse df usage output".to_string(),
            }
        })?;

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
                        // incomplete so policy skips deletion.
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
            ));
        }

        let volume_names = self
            .run_docker(&["volume", "ls", "-q"])
            .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                backend: BackendKind::Docker,
                message,
            })?;

        for volume_name in non_empty_lines(&volume_names) {
            let inspect =
                self.run_docker(&["volume", "inspect", "--format", VOLUME_INSPECT_TEMPLATE, volume_name])
                    .map_err(|message| CleanupError::CandidateDiscoveryFailed {
                        backend: BackendKind::Docker,
                        message: format!("failed to inspect volume `{volume_name}`: {message}"),
                    })?;

            candidates.push(parse_volume_candidate(
                &inspect,
                now,
                &attached_volume_names,
                &running_attached_volume_names,
            ));
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
            ResourceKind::Container => self.ensure_container_not_running(&action.candidate.identifier)?,
            ResourceKind::Image => self.ensure_image_not_referenced(&action.candidate.identifier)?,
            ResourceKind::Volume => self.ensure_volume_not_attached(&action.candidate.identifier)?,
            ResourceKind::Unknown(ref kind) => {
                return Err(CleanupError::ExecutionFailed {
                    backend: BackendKind::Docker,
                    message: format!("cannot execute unknown resource kind `{kind}`"),
                });
            }
        }

        self.delete_resource(&action.candidate.resource_kind, &action.candidate.identifier)?;

        Ok(ExecutionResponse {
            backend: BackendKind::Docker,
            candidate: action.candidate,
            executed: true,
            dry_run: false,
            message: Some("delete_executed".to_string()),
        })
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
        let metadata_complete =
            !self.id.is_empty() && self.running.is_some() && self.size_bytes.is_some() && age_days.is_some();
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
    let metadata_complete =
        !identifier.is_empty() && age_days.is_some() && size_bytes.is_some() && labels_known;

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

    let metadata_complete =
        !identifier.is_empty() && age_days.is_some() && in_use.is_some() && referenced.is_some();

    CandidateArtifact {
        backend: BackendKind::Docker,
        resource_kind: ResourceKind::Volume,
        identifier: identifier.clone(),
        display_name: Some(identifier),
        labels,
        size_bytes: None,
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
    let day_of_year = (153 * (month_of_year + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
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
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
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
