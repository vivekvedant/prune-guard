use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use prune_guard::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use prune_guard::docker_backend::{CommandRunner, DockerBackend};
use prune_guard::{
    BackendKind, CandidateDiscoveryRequest, CleanupActionKind, CleanupConfig, CleanupError,
    ExecutionMode, ExecutionRequest, PlannedAction, ResourceKind, UsageSnapshot,
};

#[test]
fn docker_health_check_reports_healthy_when_version_is_available() {
    let runner = FakeRunner::new(vec![ok(
        "docker|version|--format|{{.Server.Version}}",
        "27.1.0\n",
    )]);
    let backend = DockerBackend::with_runner(runner);

    let report = backend
        .health_check()
        .expect("health check should succeed when docker version resolves");

    assert_eq!(report.backend, BackendKind::Docker);
    assert!(report.healthy);
}

#[test]
fn docker_health_check_fails_closed_when_daemon_is_unavailable() {
    let runner = FakeRunner::new(vec![err(
        "docker|version|--format|{{.Server.Version}}",
        "Cannot connect to the Docker daemon",
    )]);
    let backend = DockerBackend::with_runner(runner);

    let error = backend
        .health_check()
        .expect_err("health check must fail closed when docker is unavailable");

    match error {
        CleanupError::HealthCheckFailed { .. } => {}
        other => panic!("expected health check failure, got {other:?}"),
    }
}

#[test]
fn docker_usage_collection_reads_docker_root_df_usage() {
    let runner = FakeRunner::new(vec![
        ok("docker|info|--format|{{.DockerRootDir}}", "/var/lib/docker\n"),
        ok(
            "df|-B1|--output=used,size|/var/lib/docker",
            "   Used       Size\n1000000000 4000000000\n",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner);

    let usage = backend
        .collect_usage()
        .expect("usage should parse from df output");

    assert_eq!(usage.backend, BackendKind::Docker);
    assert_eq!(usage.used_bytes, 1_000_000_000);
    assert_eq!(usage.total_bytes, Some(4_000_000_000));
    assert_eq!(usage.used_percent, Some(25));
}

#[test]
fn discovery_marks_running_referenced_and_attached_resources_as_unsafe() {
    let runner = FakeRunner::new(vec![
        ok(
            "docker|ps|-a|-q|--no-trunc",
            "ctr-running\nctr-stopped\n",
        ),
        ok(
            "docker|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-running",
            "ctr-running\t/web\ttrue\t2026-01-01T00:00:00Z\timg-a\t1024\tkeep=true;\tvol-live;\n",
        ),
        ok(
            "docker|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-stopped",
            "ctr-stopped\t/job\tfalse\t2025-12-01T00:00:00Z\timg-b\t512\t\tvol-old;\n",
        ),
        ok("docker|image|ls|-q|--no-trunc", "img-a\nimg-b\n"),
        ok(
            "docker|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}|img-a",
            "img-a\trepo/a:latest;\t2025-01-01T00:00:00Z\t2048\t;\n",
        ),
        ok(
            "docker|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}|img-b",
            "img-b\trepo/b:latest;\t2024-01-01T00:00:00Z\t1024\t;\n",
        ),
        ok("docker|volume|ls|-q", "vol-live\nvol-old\nvol-free\n"),
        ok(
            "docker|system|df|-v|--format|{{range .Volumes}}{{println .Name \"\\t\" .Size}}{{end}}",
            "vol-live\t100MB\nvol-old\t50MB\nvol-free\t30MB\n",
        ),
        ok(
            "docker|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-live",
            "vol-live\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "docker|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-old",
            "vol-old\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "docker|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-free",
            "vol-free\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "docker|system|df|-v|--format|{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}",
            "",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner);

    let response = backend
        .discover_candidates(discovery_request())
        .expect("candidate discovery should parse command outputs");

    let by_id: BTreeMap<_, _> = response
        .candidates
        .iter()
        .map(|candidate| (candidate.identifier.as_str(), candidate))
        .collect();

    let running_container = by_id
        .get("ctr-running")
        .expect("running container candidate should exist");
    assert_eq!(running_container.resource_kind, ResourceKind::Container);
    assert_eq!(running_container.in_use, Some(true));
    assert_eq!(running_container.referenced, Some(false));

    let running_image = by_id
        .get("img-a")
        .expect("image referenced by running container should exist");
    assert_eq!(running_image.resource_kind, ResourceKind::Image);
    assert_eq!(running_image.referenced, Some(true));

    let attached_volume = by_id
        .get("vol-live")
        .expect("attached volume candidate should exist");
    assert_eq!(attached_volume.resource_kind, ResourceKind::Volume);
    assert_eq!(attached_volume.in_use, Some(true));
    assert_eq!(attached_volume.referenced, Some(true));

    let detached_volume = by_id
        .get("vol-free")
        .expect("detached volume candidate should exist");
    assert_eq!(detached_volume.in_use, Some(false));
    assert_eq!(detached_volume.referenced, Some(false));
    assert_eq!(detached_volume.size_bytes, Some(30_000_000));
}

#[test]
fn discovery_marks_ambiguous_metadata_as_not_complete() {
    let runner = FakeRunner::new(vec![
        ok("docker|ps|-a|-q|--no-trunc", "ctr-ambiguous\n"),
        ok(
            "docker|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-ambiguous",
            "ctr-ambiguous\t/unknown\tmaybe\tbad-created\timg-x\tbad-size\t;\t;\n",
        ),
        ok("docker|image|ls|-q|--no-trunc", ""),
        ok("docker|volume|ls|-q", ""),
        ok(
            "docker|system|df|-v|--format|{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}",
            "",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner);

    let response = backend
        .discover_candidates(discovery_request())
        .expect("discovery should surface ambiguous metadata instead of panicking");

    let candidate = response
        .candidates
        .iter()
        .find(|candidate| candidate.identifier == "ctr-ambiguous")
        .expect("ambiguous container candidate should exist");

    assert!(!candidate.metadata_complete);
    assert!(candidate.metadata_ambiguous);
    assert_eq!(candidate.in_use, None);
    assert_eq!(candidate.age_days, None);
    assert_eq!(candidate.size_bytes, None);
}

#[test]
fn discovery_degrades_when_image_labels_are_unavailable() {
    let runner = FakeRunner::new(vec![
        ok("docker|ps|-a|-q|--no-trunc", ""),
        ok(
            "docker|image|ls|-q|--no-trunc",
            "sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11\n",
        ),
        err(
            "docker|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}|sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11",
            "`docker` exited with code Some(1): template parsing error: template: :1:88: executing \"\" at <.Config.Labels>: map has no entry for key \"Labels\"",
        ),
        ok(
            "docker|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t|sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11",
            "sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11\trepo/no-labels:latest;\t2024-01-01T00:00:00Z\t2048\t\n",
        ),
        ok("docker|volume|ls|-q", ""),
        ok(
            "docker|system|df|-v|--format|{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}",
            "",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner);

    let response = backend
        .discover_candidates(discovery_request())
        .expect("discovery should fail closed for one image instead of failing the full backend");

    let image = response
        .candidates
        .iter()
        .find(|candidate| {
            candidate.identifier
                == "sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11"
        })
        .expect("image candidate should still be returned");

    assert!(!image.metadata_complete);
    assert!(image.metadata_ambiguous);
    assert!(image.labels.is_empty());
    assert_eq!(image.referenced, Some(false));
}

#[test]
fn discovery_uses_verbose_df_sizes_for_volumes() {
    let runner = FakeRunner::new(vec![
        ok("docker|ps|-a|-q|--no-trunc", ""),
        ok("docker|image|ls|-q|--no-trunc", ""),
        ok("docker|volume|ls|-q", "vol-a\n"),
        ok(
            "docker|system|df|-v|--format|{{range .Volumes}}{{println .Name \"\\t\" .Size}}{{end}}",
            "vol-a\t67.11MB\n",
        ),
        ok(
            "docker|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-a",
            "vol-a\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "docker|system|df|-v|--format|{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}",
            "",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner.clone());

    let response = backend
        .discover_candidates(discovery_request())
        .expect("discovery should parse volume size map from docker system df");

    let volume = response
        .candidates
        .iter()
        .find(|candidate| candidate.identifier == "vol-a")
        .expect("volume candidate should exist");

    assert!(
        volume.size_bytes.is_some(),
        "volume size should be populated from docker system df -v"
    );
    assert!(
        runner.calls().iter().any(|call| {
            call == "docker|system|df|-v|--format|{{range .Volumes}}{{println .Name \"\\t\" .Size}}{{end}}"
        }),
        "discovery should invoke docker system df verbose volume template"
    );
}

#[test]
fn discovery_emits_build_cache_candidate_with_known_size() {
    let runner = FakeRunner::new(vec![
        ok("docker|ps|-a|-q|--no-trunc", ""),
        ok("docker|image|ls|-q|--no-trunc", ""),
        ok("docker|volume|ls|-q", ""),
        ok(
            "docker|system|df|-v|--format|{{range .BuildCache}}{{println .ID \"\\t\" .Size \"\\t\" .InUse \"\\t\" .LastUsedAt \"\\t\" .CreatedAt}}{{end}}",
            "cache-1\t100MB\tfalse\t2024-01-01T00:00:00Z\t2024-01-01T00:00:00Z\ncache-2\t50MB\ttrue\t2024-01-01T00:00:00Z\t2024-01-01T00:00:00Z\n",
        ),
    ]);
    let backend = DockerBackend::with_runner(runner);

    let response = backend
        .discover_candidates(discovery_request())
        .expect("discovery should synthesize a build cache candidate");

    let cache_candidate = response
        .candidates
        .iter()
        .find(|candidate| candidate.resource_kind == ResourceKind::BuildCache)
        .expect("build cache candidate should be present");

    assert_eq!(cache_candidate.identifier, "docker-build-cache-unused");
    assert_eq!(cache_candidate.size_bytes, Some(100_000_000));
    assert_eq!(cache_candidate.in_use, Some(false));
    assert_eq!(cache_candidate.referenced, Some(false));
    assert!(cache_candidate.metadata_complete);
}

#[test]
fn execution_blocks_running_container_deletion() {
    let runner = FakeRunner::new(vec![ok(
        "docker|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-running",
        "ctr-running\t/web\ttrue\t2026-01-01T00:00:00Z\timg-a\t1024\t;\t;\n",
    )]);
    let backend = DockerBackend::with_runner(runner.clone());

    let error = backend
        .execute(execution_request("ctr-running", ResourceKind::Container, ExecutionMode::RealRun))
        .expect_err("running container must never be deleted");

    match error {
        CleanupError::SafetyViolation { message } => {
            assert!(message.contains("running container"));
        }
        other => panic!("expected safety violation, got {other:?}"),
    }

    assert!(
        !runner
            .calls()
            .iter()
            .any(|call| call == "docker|container|rm|ctr-running"),
        "delete command must not run for running containers"
    );
}

#[test]
fn execution_blocks_referenced_image_deletion() {
    let runner = FakeRunner::new(vec![ok(
        "docker|ps|-a|--format|{{.ImageID}}",
        "img-a\nimg-b\n",
    )]);
    let backend = DockerBackend::with_runner(runner.clone());

    let error = backend
        .execute(execution_request("img-a", ResourceKind::Image, ExecutionMode::RealRun))
        .expect_err("referenced image must never be deleted");

    match error {
        CleanupError::SafetyViolation { message } => {
            assert!(message.contains("referenced image"));
        }
        other => panic!("expected safety violation, got {other:?}"),
    }

    assert!(
        !runner
            .calls()
            .iter()
            .any(|call| call == "docker|image|rm|img-a"),
        "delete command must not run for referenced images"
    );
}

#[test]
fn execution_dry_run_returns_without_delete_command() {
    let runner = FakeRunner::new(vec![]);
    let backend = DockerBackend::with_runner(runner.clone());

    let response = backend
        .execute(execution_request(
            "img-unused",
            ResourceKind::Image,
            ExecutionMode::DryRun,
        ))
        .expect("dry run should succeed without shelling out");

    assert!(response.dry_run);
    assert!(!response.executed);
    assert!(
        runner.calls().is_empty(),
        "dry-run execution must not invoke docker delete commands"
    );
}

#[test]
fn execution_prunes_build_cache_in_real_run() {
    let runner = FakeRunner::new(vec![ok("docker|builder|prune|-f", "Total:\t0B\n")]);
    let backend = DockerBackend::with_runner(runner.clone());

    let response = backend
        .execute(build_cache_execution_request(
            "docker-build-cache-unused",
            Some(0),
            ExecutionMode::RealRun,
        ))
        .expect("build cache candidate should execute prune command");

    assert!(response.executed);
    assert!(!response.dry_run);
    assert!(
        runner
            .calls()
            .iter()
            .any(|call| call == "docker|builder|prune|-f"),
        "build cache execution should call docker builder prune"
    );
}

#[test]
fn execution_prunes_build_cache_with_age_filter_when_present() {
    let runner = FakeRunner::new(vec![ok(
        "docker|builder|prune|-f|--filter|until=48h",
        "Total:\t0B\n",
    )]);
    let backend = DockerBackend::with_runner(runner.clone());

    let response = backend
        .execute(build_cache_execution_request(
            "docker-build-cache-unused",
            Some(2),
            ExecutionMode::RealRun,
        ))
        .expect("build cache prune should include age filter for non-zero age");

    assert!(response.executed);
    assert!(!response.dry_run);
    assert!(
        runner
            .calls()
            .iter()
            .any(|call| call == "docker|builder|prune|-f|--filter|until=48h"),
        "build cache execution should pass until-hour filter"
    );
}

#[derive(Clone, Default)]
struct FakeRunner {
    expectations: Arc<Mutex<VecDeque<ExpectedCommand>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct ExpectedCommand {
    key: String,
    output: Result<String, String>,
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
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        let key = command_key(program, args);
        self.calls.lock().expect("calls lock poisoned").push(key.clone());

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

fn discovery_request() -> CandidateDiscoveryRequest {
    CandidateDiscoveryRequest {
        backend: BackendKind::Docker,
        config: CleanupConfig {
            interval_secs: 1,
            high_watermark_percent: 85,
            target_watermark_percent: 70,
            min_unused_age_days: 7,
            max_delete_per_run_gb: 5,
            dry_run: false,
            protected_images: vec![],
            protected_volumes: vec![],
            protected_labels: vec![],
        },
        usage: UsageSnapshot {
            backend: BackendKind::Docker,
            used_bytes: 90,
            total_bytes: Some(100),
            used_percent: Some(90),
            observed_at: None,
        },
    }
}

fn execution_request(
    id: &str,
    resource_kind: ResourceKind,
    mode: ExecutionMode,
) -> ExecutionRequest {
    ExecutionRequest {
        backend: BackendKind::Docker,
        action: PlannedAction {
            candidate: prune_guard::CandidateArtifact {
                backend: BackendKind::Docker,
                resource_kind,
                identifier: id.to_string(),
                display_name: None,
                labels: BTreeSet::new(),
                size_bytes: Some(1024),
                age_days: Some(60),
                in_use: Some(false),
                referenced: Some(false),
                protected: false,
                metadata_complete: true,
                metadata_ambiguous: false,
                discovered_at: None,
            },
            kind: CleanupActionKind::Delete,
            dry_run: matches!(mode, ExecutionMode::DryRun),
            reason: Some("test action".to_string()),
        },
        mode,
    }
}

fn build_cache_execution_request(
    id: &str,
    age_days: Option<u64>,
    mode: ExecutionMode,
) -> ExecutionRequest {
    ExecutionRequest {
        backend: BackendKind::Docker,
        action: PlannedAction {
            candidate: prune_guard::CandidateArtifact {
                backend: BackendKind::Docker,
                resource_kind: ResourceKind::BuildCache,
                identifier: id.to_string(),
                display_name: Some("docker-build-cache".to_string()),
                labels: BTreeSet::new(),
                size_bytes: Some(1024),
                age_days,
                in_use: Some(false),
                referenced: Some(false),
                protected: false,
                metadata_complete: true,
                metadata_ambiguous: false,
                discovered_at: None,
            },
            kind: CleanupActionKind::Delete,
            dry_run: matches!(mode, ExecutionMode::DryRun),
            reason: Some("build cache test action".to_string()),
        },
        mode,
    }
}
