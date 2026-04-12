use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use prune_guard::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use prune_guard::{
    BackendKind, CandidateArtifact, CandidateDiscoveryRequest, CandidateDiscoveryResponse,
    CleanupConfig, CleanupError, CleanupScheduler, ExecutionMode, ExecutionRequest,
    ExecutionResponse, HealthReport, ResourceKind, SchedulerStopReason, UsageSnapshot,
};

#[test]
fn below_high_watermark_skips_cleanup_loop_and_never_discovers_or_executes() {
    let mut cfg = base_config();
    cfg.high_watermark_percent = 85;
    cfg.target_watermark_percent = 70;

    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(65)],
        PlannerMode::SingleAction,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("below watermark run should no-op safely");

    assert_eq!(report.stop_reason, SchedulerStopReason::BelowHighWatermark);
    assert!(!report.cleanup_started);
    assert_eq!(backend.usage_calls(), 1);
    assert_eq!(backend.discovery_calls(), 0);
    assert_eq!(backend.execute_calls(), 0);
}

#[test]
fn above_high_watermark_runs_cleanup_loop_and_stops_at_target_watermark() {
    let mut cfg = base_config();
    cfg.high_watermark_percent = 85;
    cfg.target_watermark_percent = 70;

    // First sample triggers cleanup (90), second still above target (75), third hits stop condition (70).
    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(90), usage_percent(75), usage_percent(70)],
        PlannerMode::SingleAction,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("loop should converge to target watermark");

    assert_eq!(
        report.stop_reason,
        SchedulerStopReason::TargetWatermarkReached
    );
    assert!(report.cleanup_started);
    assert_eq!(report.iterations, 2);
    assert_eq!(report.actions_planned, 2);
    assert_eq!(report.actions_completed, 2);
    assert_eq!(report.action_failures, 0);
    assert_eq!(backend.discovery_calls(), 2);
    assert_eq!(backend.execute_calls(), 2);
    assert_eq!(backend.usage_calls(), 3);
}

#[test]
fn above_high_with_zero_planned_actions_stops_fail_closed_without_infinite_loop() {
    let mut cfg = base_config();
    cfg.high_watermark_percent = 85;
    cfg.target_watermark_percent = 70;

    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(90), usage_percent(90), usage_percent(90)],
        PlannerMode::ZeroActions,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let (tx, rx) = mpsc::channel();
    let backend_for_thread = Arc::clone(&backend);
    thread::spawn(move || {
        let result = scheduler.run_once(backend_for_thread);
        let _ = tx.send(result);
    });

    let report = rx
        .recv_timeout(Duration::from_millis(250))
        .expect("scheduler must terminate quickly when planner returns zero actions");
    let report = report.expect("scheduler should fail-closed stop without returning an error");

    assert_eq!(
        report.stop_reason,
        SchedulerStopReason::NoActionableCandidates
    );
    assert!(report.cleanup_started);
    assert_eq!(report.iterations, 1);
    assert_eq!(backend.discovery_calls(), 1);
    assert_eq!(backend.execute_calls(), 0);
    assert_eq!(backend.usage_calls(), 1);
}

#[test]
fn unknown_usage_percent_fails_closed_and_skips_cleanup_pipeline() {
    let mut cfg = base_config();
    cfg.high_watermark_percent = 85;
    cfg.target_watermark_percent = 70;

    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_unknown()],
        PlannerMode::SingleAction,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("unknown usage percent should be a fail-closed no-op");

    assert_eq!(report.stop_reason, SchedulerStopReason::UsagePercentUnknown);
    assert!(!report.cleanup_started);
    assert_eq!(backend.usage_calls(), 1);
    assert_eq!(backend.discovery_calls(), 0);
    assert_eq!(backend.execute_calls(), 0);
}

#[test]
fn execution_failure_stops_cycle_fail_closed_before_next_iteration() {
    let cfg = base_config();
    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(90), usage_percent(85)],
        PlannerMode::SingleAction,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::AlwaysFail,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("scheduler should return fail-closed report");

    assert_eq!(
        report.stop_reason,
        SchedulerStopReason::ExecutionFailuresDetected
    );
    assert!(report.cleanup_started);
    assert_eq!(report.action_failures, 1);
    assert_eq!(backend.discovery_calls(), 1);
    assert_eq!(backend.execute_calls(), 1);
}

#[test]
fn scheduler_rechecks_health_each_iteration_and_stops_when_backend_turns_unhealthy() {
    let cfg = base_config();
    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(90), usage_percent(90), usage_percent(90)],
        PlannerMode::SingleAction,
        HealthMode::UnhealthyOnSecondCall,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("scheduler should stop safely when health turns unhealthy");

    assert_eq!(report.stop_reason, SchedulerStopReason::BackendUnhealthy);
    assert_eq!(backend.discovery_calls(), 1);
    assert_eq!(backend.execute_calls(), 1);
    assert_eq!(backend.health_calls(), 2);
}

#[test]
fn zero_delete_budget_stops_with_deletion_cap_reason() {
    let mut cfg = base_config();
    cfg.max_delete_per_run_gb = 0;

    let backend = Arc::new(MockSchedulerBackend::new(
        vec![usage_percent(90)],
        PlannerMode::SingleAction,
        HealthMode::AlwaysHealthy,
        ExecutionBehavior::Success,
    ));
    let scheduler = CleanupScheduler::new(cfg);

    let report = scheduler
        .run_once(Arc::clone(&backend))
        .expect("scheduler should stop safely on delete-cap exhaustion");

    assert_eq!(report.stop_reason, SchedulerStopReason::DeletionCapReached);
    assert_eq!(backend.discovery_calls(), 1);
    assert_eq!(backend.execute_calls(), 0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlannerMode {
    SingleAction,
    ZeroActions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HealthMode {
    AlwaysHealthy,
    UnhealthyOnSecondCall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionBehavior {
    Success,
    AlwaysFail,
}

#[derive(Debug)]
struct MockSchedulerBackend {
    usage_samples: Mutex<VecDeque<UsageSnapshot>>,
    planner_mode: PlannerMode,
    health_mode: HealthMode,
    execution_behavior: ExecutionBehavior,
    health_calls: AtomicUsize,
    usage_calls: AtomicUsize,
    discovery_calls: AtomicUsize,
    execute_calls: AtomicUsize,
}

impl MockSchedulerBackend {
    fn new(
        usage_samples: Vec<UsageSnapshot>,
        planner_mode: PlannerMode,
        health_mode: HealthMode,
        execution_behavior: ExecutionBehavior,
    ) -> Self {
        Self {
            usage_samples: Mutex::new(VecDeque::from(usage_samples)),
            planner_mode,
            health_mode,
            execution_behavior,
            health_calls: AtomicUsize::new(0),
            usage_calls: AtomicUsize::new(0),
            discovery_calls: AtomicUsize::new(0),
            execute_calls: AtomicUsize::new(0),
        }
    }

    fn health_calls(&self) -> usize {
        self.health_calls.load(Ordering::SeqCst)
    }

    fn usage_calls(&self) -> usize {
        self.usage_calls.load(Ordering::SeqCst)
    }

    fn discovery_calls(&self) -> usize {
        self.discovery_calls.load(Ordering::SeqCst)
    }

    fn execute_calls(&self) -> usize {
        self.execute_calls.load(Ordering::SeqCst)
    }

    fn sample_candidate(identifier: &str) -> CandidateArtifact {
        CandidateArtifact {
            backend: BackendKind::Docker,
            resource_kind: ResourceKind::Image,
            identifier: identifier.to_string(),
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
        }
    }
}

impl HealthCheck for MockSchedulerBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Docker
    }

    fn health_check(&self) -> prune_guard::Result<HealthReport> {
        let call = self.health_calls.fetch_add(1, Ordering::SeqCst);
        if self.health_mode == HealthMode::UnhealthyOnSecondCall && call >= 1 {
            return Ok(HealthReport::unhealthy(
                BackendKind::Docker,
                "backend degraded after first loop iteration",
            ));
        }

        Ok(HealthReport::healthy(BackendKind::Docker))
    }
}

impl UsageCollector for MockSchedulerBackend {
    fn collect_usage(&self) -> prune_guard::Result<UsageSnapshot> {
        self.usage_calls.fetch_add(1, Ordering::SeqCst);

        let mut samples = self
            .usage_samples
            .lock()
            .expect("usage sample queue lock should not be poisoned");

        if let Some(snapshot) = samples.pop_front() {
            Ok(snapshot)
        } else {
            Err(CleanupError::UsageCollectionFailed {
                backend: BackendKind::Docker,
                message: "no queued usage sample".to_string(),
            })
        }
    }
}

impl CandidateDiscoverer for MockSchedulerBackend {
    fn discover_candidates(
        &self,
        _request: CandidateDiscoveryRequest,
    ) -> prune_guard::Result<CandidateDiscoveryResponse> {
        let call_idx = self.discovery_calls.fetch_add(1, Ordering::SeqCst);

        let mut candidate = Self::sample_candidate(&format!("img-{call_idx}"));
        if self.planner_mode == PlannerMode::ZeroActions {
            // Fail-closed planner behavior: unknown reclaim size must not produce delete action.
            candidate.size_bytes = None;
        }

        Ok(CandidateDiscoveryResponse {
            backend: BackendKind::Docker,
            candidates: vec![candidate],
        })
    }
}

impl ExecutionContract for MockSchedulerBackend {
    fn execute(&self, request: ExecutionRequest) -> prune_guard::Result<ExecutionResponse> {
        self.execute_calls.fetch_add(1, Ordering::SeqCst);
        if self.execution_behavior == ExecutionBehavior::AlwaysFail {
            return Err(CleanupError::ExecutionFailed {
                backend: request.backend,
                message: "simulated execution failure".to_string(),
            });
        }

        Ok(ExecutionResponse {
            backend: request.backend,
            candidate: request.action.candidate,
            executed: matches!(request.mode, ExecutionMode::RealRun),
            dry_run: matches!(request.mode, ExecutionMode::DryRun),
            message: Some("mock execution complete".to_string()),
        })
    }
}

fn base_config() -> CleanupConfig {
    CleanupConfig {
        interval_secs: 1,
        high_watermark_percent: 85,
        target_watermark_percent: 70,
        min_unused_age_days: 7,
        max_delete_per_run_gb: 10,
        dry_run: false,
        protected_images: Vec::new(),
        protected_volumes: Vec::new(),
        protected_labels: Vec::new(),
    }
}

fn usage_percent(used_percent: u8) -> UsageSnapshot {
    UsageSnapshot {
        backend: BackendKind::Docker,
        used_bytes: 90 * 1024 * 1024 * 1024,
        total_bytes: Some(100 * 1024 * 1024 * 1024),
        used_percent: Some(used_percent),
        observed_at: None,
    }
}

fn usage_unknown() -> UsageSnapshot {
    UsageSnapshot {
        backend: BackendKind::Docker,
        used_bytes: 0,
        total_bytes: None,
        used_percent: None,
        observed_at: None,
    }
}
