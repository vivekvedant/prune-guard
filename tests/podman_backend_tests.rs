use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use prune_guard::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use prune_guard::docker_backend::CommandRunner;
use prune_guard::podman_backend::PodmanBackend;
use prune_guard::{
    BackendKind, CandidateDiscoveryRequest, CleanupActionKind, CleanupConfig, CleanupError,
    ExecutionMode, ExecutionRequest, PlannedAction, ResourceKind, UsageSnapshot,
};

#[test]
fn podman_health_check_reports_healthy_when_version_is_available() {
    let runner = FakeRunner::new(vec![ok(
        "podman|version|--format|{{.Server.Version}}",
        "5.2.0\n",
    )]);
    let backend = PodmanBackend::with_runner(runner);

    let report = backend
        .health_check()
        .expect("health check should succeed when podman version resolves");

    assert_eq!(report.backend, BackendKind::Podman);
    assert!(report.healthy);
}

#[test]
fn podman_health_check_degrades_gracefully_when_backend_is_unavailable() {
    let runner = FakeRunner::new(vec![err(
        "podman|version|--format|{{.Server.Version}}",
        "podman command not found",
    )]);
    let backend = PodmanBackend::with_runner(runner);

    let report = backend
        .health_check()
        .expect("unavailable podman should degrade gracefully with unhealthy report");

    assert_eq!(report.backend, BackendKind::Podman);
    assert!(!report.healthy);
    assert!(
        report
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("podman"),
        "unhealthy report should include backend context"
    );
}

#[test]
fn podman_usage_collection_reads_store_graph_root_usage() {
    let runner = FakeRunner::new(vec![
        ok(
            "podman|info|--format|{{.Store.GraphRoot}}",
            "/var/lib/containers/storage\n",
        ),
        ok(
            "df|-B1|--output=used,size|/var/lib/containers/storage",
            "   Used       Size\n500000000 2000000000\n",
        ),
    ]);
    let backend = PodmanBackend::with_runner(runner);

    let usage = backend
        .collect_usage()
        .expect("usage should parse from df output");

    assert_eq!(usage.backend, BackendKind::Podman);
    assert_eq!(usage.used_bytes, 500_000_000);
    assert_eq!(usage.total_bytes, Some(2_000_000_000));
    assert_eq!(usage.used_percent, Some(25));
}

#[test]
fn discovery_marks_running_referenced_and_attached_resources_as_unsafe() {
    let runner = FakeRunner::new(vec![
        ok(
            "podman|ps|-a|-q|--no-trunc",
            "ctr-running\nctr-stopped\n",
        ),
        ok(
            "podman|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-running",
            "ctr-running\t/web\ttrue\t2026-01-01T00:00:00Z\timg-a\t1024\tkeep=true;\tvol-live;\n",
        ),
        ok(
            "podman|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-stopped",
            "ctr-stopped\t/job\tfalse\t2025-12-01T00:00:00Z\timg-b\t512\t\tvol-old;\n",
        ),
        ok("podman|image|ls|-q|--no-trunc", "img-a\nimg-b\n"),
        ok(
            "podman|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}|img-a",
            "img-a\trepo/a:latest;\t2025-01-01T00:00:00Z\t2048\t;\n",
        ),
        ok(
            "podman|image|inspect|--format|{{.Id}}\t{{range .RepoTags}}{{.}};{{end}}\t{{.Created}}\t{{.Size}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}|img-b",
            "img-b\trepo/b:latest;\t2024-01-01T00:00:00Z\t1024\t;\n",
        ),
        ok("podman|volume|ls|-q", "vol-live\nvol-old\nvol-free\n"),
        ok(
            "podman|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-live",
            "vol-live\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "podman|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-old",
            "vol-old\t2024-01-01T00:00:00Z\t;\n",
        ),
        ok(
            "podman|volume|inspect|--format|{{.Name}}\t{{.CreatedAt}}\t{{range $k,$v := .Labels}}{{$k}}={{$v}};{{end}}|vol-free",
            "vol-free\t2024-01-01T00:00:00Z\t;\n",
        ),
    ]);
    let backend = PodmanBackend::with_runner(runner);

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
}

#[test]
fn discovery_marks_ambiguous_metadata_as_not_complete() {
    let runner = FakeRunner::new(vec![
        ok("podman|ps|-a|-q|--no-trunc", "ctr-ambiguous\n"),
        ok(
            "podman|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-ambiguous",
            "ctr-ambiguous\t/unknown\tmaybe\tbad-created\timg-x\tbad-size\t;\t;\n",
        ),
        ok("podman|image|ls|-q|--no-trunc", ""),
        ok("podman|volume|ls|-q", ""),
    ]);
    let backend = PodmanBackend::with_runner(runner);

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
fn execution_blocks_running_container_deletion() {
    let runner = FakeRunner::new(vec![ok(
        "podman|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-running",
        "ctr-running\t/web\ttrue\t2026-01-01T00:00:00Z\timg-a\t1024\t;\t;\n",
    )]);
    let backend = PodmanBackend::with_runner(runner.clone());

    let error = backend
        .execute(execution_request(
            "ctr-running",
            ResourceKind::Container,
            ExecutionMode::RealRun,
        ))
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
            .any(|call| call == "podman|container|rm|ctr-running"),
        "delete command must not run for running containers"
    );
}

#[test]
fn execution_blocks_referenced_image_deletion() {
    let runner = FakeRunner::new(vec![ok(
        "podman|ps|-a|--format|{{.ImageID}}",
        "img-a\nimg-b\n",
    )]);
    let backend = PodmanBackend::with_runner(runner.clone());

    let error = backend
        .execute(execution_request(
            "img-a",
            ResourceKind::Image,
            ExecutionMode::RealRun,
        ))
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
            .any(|call| call == "podman|image|rm|img-a"),
        "delete command must not run for referenced images"
    );
}

#[test]
fn execution_blocks_attached_volume_deletion() {
    let runner = FakeRunner::new(vec![
        ok("podman|ps|-a|-q|--no-trunc", "ctr-a\n"),
        ok(
            "podman|container|inspect|--size|--format|{{.Id}}\t{{.Name}}\t{{.State.Running}}\t{{.Created}}\t{{.Image}}\t{{.SizeRw}}\t{{range $k,$v := .Config.Labels}}{{$k}}={{$v}};{{end}}\t{{range .Mounts}}{{.Name}};{{end}}|ctr-a",
            "ctr-a\t/job\tfalse\t2025-01-01T00:00:00Z\timg-a\t200\t;\tvol-used;\n",
        ),
    ]);
    let backend = PodmanBackend::with_runner(runner.clone());

    let error = backend
        .execute(execution_request(
            "vol-used",
            ResourceKind::Volume,
            ExecutionMode::RealRun,
        ))
        .expect_err("attached volume must never be deleted");

    match error {
        CleanupError::SafetyViolation { message } => {
            assert!(message.contains("attached volume"));
        }
        other => panic!("expected safety violation, got {other:?}"),
    }

    assert!(
        !runner
            .calls()
            .iter()
            .any(|call| call == "podman|volume|rm|vol-used"),
        "delete command must not run for attached volumes"
    );
}

#[test]
fn execution_dry_run_returns_without_delete_command() {
    let runner = FakeRunner::new(vec![]);
    let backend = PodmanBackend::with_runner(runner.clone());

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
        "dry-run execution must not invoke podman delete commands"
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

fn discovery_request() -> CandidateDiscoveryRequest {
    CandidateDiscoveryRequest {
        backend: BackendKind::Podman,
        config: CleanupConfig {
            interval_secs: 1,
            high_watermark_percent: 85,
            target_watermark_percent: 70,
            min_unused_age_days: 7,
            max_delete_per_run_gb: 5,
            dry_run: false,
            allow_missing_image_labels: false,
            protected_images: vec![],
            protected_volumes: vec![],
            protected_labels: vec![],
        },
        usage: UsageSnapshot {
            backend: BackendKind::Podman,
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
        backend: BackendKind::Podman,
        action: PlannedAction {
            candidate: prune_guard::CandidateArtifact {
                backend: BackendKind::Podman,
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
