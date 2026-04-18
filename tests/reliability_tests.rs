use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use prune_guard::reliability::{
    BackendCycleRunner, BackendRunStatus, InstanceGuard, ReliabilityCoordinator, RetryPolicy,
    RetrySleeper, SingleInstanceLock,
};
use prune_guard::{
    BackendKind, CleanupError, SchedulerRunReport, SchedulerStopReason, UsageSnapshot,
};

#[test]
fn retries_follow_exponential_backoff_and_respect_max_attempts() {
    let runner = Arc::new(MockBackendRunner::new(
        BackendKind::Docker,
        vec![
            Ok(retryable_failure_report(
                BackendKind::Docker,
                "transient failure #1",
            )),
            Ok(retryable_failure_report(
                BackendKind::Docker,
                "transient failure #2",
            )),
            Ok(success_report(BackendKind::Docker)),
        ],
    ));
    let sleeper = FakeSleeper::default();
    let lock = FakeLock::acquired();
    let coordinator = coordinator(policy(5, 10), sleeper.clone(), lock);

    let backends: Vec<Arc<dyn BackendCycleRunner>> = vec![runner.clone()];
    let summary = coordinator
        .run_once(&backends)
        .expect("run should return a summary");

    assert!(summary.lock_acquired);
    assert_eq!(runner.call_count(), 3);
    assert_eq!(
        sleeper.durations(),
        vec![Duration::from_millis(10), Duration::from_millis(20)]
    );
    assert_eq!(summary.backend_reports.len(), 1);
    assert_eq!(summary.backend_reports[0].status, BackendRunStatus::Success);
    assert_eq!(summary.backend_reports[0].attempts, 3);
}

#[test]
fn partial_backend_failure_does_not_block_other_backends() {
    let docker = Arc::new(MockBackendRunner::new(
        BackendKind::Docker,
        vec![
            Err(exec_error(BackendKind::Docker, "hard fail #1")),
            Err(exec_error(BackendKind::Docker, "hard fail #2")),
            Err(exec_error(BackendKind::Docker, "hard fail #3")),
        ],
    ));
    let podman = Arc::new(MockBackendRunner::new(
        BackendKind::Podman,
        vec![Ok(success_report(BackendKind::Podman))],
    ));
    let sleeper = FakeSleeper::default();
    let coordinator = coordinator(policy(3, 5), sleeper, FakeLock::acquired());

    let backends: Vec<Arc<dyn BackendCycleRunner>> = vec![docker.clone(), podman.clone()];
    let summary = coordinator
        .run_once(&backends)
        .expect("partial failure should still produce a full summary");

    assert_eq!(docker.call_count(), 3);
    assert_eq!(podman.call_count(), 1);
    assert_eq!(summary.backend_reports.len(), 2);
    assert_eq!(summary.backend_reports[0].backend, BackendKind::Docker);
    assert_eq!(
        summary.backend_reports[0].status,
        BackendRunStatus::FailedAfterRetries
    );
    assert_eq!(summary.backend_reports[1].backend, BackendKind::Podman);
    assert_eq!(summary.backend_reports[1].status, BackendRunStatus::Success);
    assert!(!summary.all_backends_failed);
    assert!(!summary.no_op);
}

#[test]
fn all_backends_fail_returns_noop_summary_instead_of_crashing() {
    let docker = Arc::new(MockBackendRunner::new(
        BackendKind::Docker,
        vec![
            Err(exec_error(BackendKind::Docker, "docker fail #1")),
            Err(exec_error(BackendKind::Docker, "docker fail #2")),
        ],
    ));
    let podman = Arc::new(MockBackendRunner::new(
        BackendKind::Podman,
        vec![
            Err(exec_error(BackendKind::Podman, "podman fail #1")),
            Err(exec_error(BackendKind::Podman, "podman fail #2")),
        ],
    ));
    let coordinator = coordinator(policy(2, 7), FakeSleeper::default(), FakeLock::acquired());

    let backends: Vec<Arc<dyn BackendCycleRunner>> = vec![docker, podman];
    let summary = coordinator
        .run_once(&backends)
        .expect("all-backend failure must return a summary and not panic");

    assert!(summary.lock_acquired);
    assert_eq!(summary.backend_reports.len(), 2);
    assert!(summary.all_backends_failed);
    assert!(summary.no_op);
}

#[test]
fn repeated_execution_failures_fail_closed_for_backend_after_retries() {
    let runner = Arc::new(MockBackendRunner::new(
        BackendKind::Docker,
        vec![
            Ok(retryable_failure_report(
                BackendKind::Docker,
                "execution failure #1",
            )),
            Ok(retryable_failure_report(
                BackendKind::Docker,
                "execution failure #2",
            )),
            Ok(retryable_failure_report(
                BackendKind::Docker,
                "execution failure #3",
            )),
        ],
    ));
    let coordinator = coordinator(policy(3, 1), FakeSleeper::default(), FakeLock::acquired());

    let backends: Vec<Arc<dyn BackendCycleRunner>> = vec![runner.clone()];
    let summary = coordinator
        .run_once(&backends)
        .expect("fail-closed retry exhaustion should still report safely");

    assert_eq!(runner.call_count(), 3);
    assert_eq!(summary.backend_reports.len(), 1);
    let report = &summary.backend_reports[0];
    assert_eq!(report.backend, BackendKind::Docker);
    assert_eq!(report.attempts, 3);
    assert_eq!(report.status, BackendRunStatus::FailedAfterRetries);
    assert!(report.final_report.is_some());
    assert!(
        report.last_error.is_some(),
        "last retryable failure context should be preserved"
    );
}

#[test]
fn single_instance_lock_prevents_concurrent_run() {
    let runner = Arc::new(MockBackendRunner::new(
        BackendKind::Docker,
        vec![Ok(success_report(BackendKind::Docker))],
    ));
    let coordinator = coordinator(policy(3, 10), FakeSleeper::default(), FakeLock::contended());

    let backends: Vec<Arc<dyn BackendCycleRunner>> = vec![runner.clone()];
    let summary = coordinator
        .run_once(&backends)
        .expect("lock contention should no-op safely");

    assert!(!summary.lock_acquired);
    assert!(summary.no_op);
    assert!(summary.backend_reports.is_empty());
    assert_eq!(
        runner.call_count(),
        0,
        "backend must not run without the lock"
    );
}

#[derive(Debug, Clone, Default)]
struct FakeSleeper {
    durations: Arc<Mutex<Vec<Duration>>>,
}

impl FakeSleeper {
    fn durations(&self) -> Vec<Duration> {
        self.durations
            .lock()
            .expect("durations lock poisoned")
            .clone()
    }
}

impl RetrySleeper for FakeSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations
            .lock()
            .expect("durations lock poisoned")
            .push(duration);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockMode {
    Acquired,
    Contended,
}

#[derive(Debug, Clone, Copy)]
struct FakeLock {
    mode: LockMode,
}

impl FakeLock {
    fn acquired() -> Self {
        Self {
            mode: LockMode::Acquired,
        }
    }

    fn contended() -> Self {
        Self {
            mode: LockMode::Contended,
        }
    }
}

#[derive(Debug)]
struct FakeGuard;
impl InstanceGuard for FakeGuard {}

impl SingleInstanceLock for FakeLock {
    fn try_acquire(&self) -> prune_guard::Result<Option<Box<dyn InstanceGuard>>> {
        match self.mode {
            LockMode::Acquired => Ok(Some(Box::new(FakeGuard))),
            LockMode::Contended => Ok(None),
        }
    }
}

#[derive(Debug)]
struct MockBackendRunner {
    backend: BackendKind,
    outcomes: Mutex<VecDeque<prune_guard::Result<SchedulerRunReport>>>,
    calls: AtomicUsize,
}

impl MockBackendRunner {
    fn new(backend: BackendKind, outcomes: Vec<prune_guard::Result<SchedulerRunReport>>) -> Self {
        Self {
            backend,
            outcomes: Mutex::new(VecDeque::from(outcomes)),
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl BackendCycleRunner for MockBackendRunner {
    fn backend_kind(&self) -> BackendKind {
        self.backend.clone()
    }

    fn run_cycle(&self) -> prune_guard::Result<SchedulerRunReport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .expect("outcomes lock poisoned")
            .pop_front()
            .unwrap_or_else(|| Err(exec_error(self.backend.clone(), "missing scripted outcome")))
    }
}

fn coordinator(
    retry_policy: RetryPolicy,
    sleeper: FakeSleeper,
    lock: FakeLock,
) -> ReliabilityCoordinator<FakeSleeper, FakeLock> {
    ReliabilityCoordinator::new(retry_policy, sleeper, lock)
}

fn policy(max_attempts: usize, initial_backoff_ms: u64) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        initial_backoff: Duration::from_millis(initial_backoff_ms),
        backoff_multiplier: 2,
        max_backoff: Duration::from_secs(5),
    }
}

fn success_report(backend: BackendKind) -> SchedulerRunReport {
    SchedulerRunReport {
        backend,
        dry_run: false,
        cleanup_started: true,
        iterations: 1,
        actions_planned: 1,
        actions_completed: 1,
        reclaimed_estimated_bytes: 0,
        action_failures: 0,
        skipped_candidates: 0,
        initial_usage: Some(sample_usage()),
        final_usage: Some(sample_usage()),
        stop_reason: SchedulerStopReason::TargetWatermarkReached,
        last_error: None,
    }
}

fn retryable_failure_report(backend: BackendKind, message: &str) -> SchedulerRunReport {
    SchedulerRunReport {
        backend: backend.clone(),
        dry_run: false,
        cleanup_started: true,
        iterations: 1,
        actions_planned: 1,
        actions_completed: 0,
        reclaimed_estimated_bytes: 0,
        action_failures: 1,
        skipped_candidates: 0,
        initial_usage: Some(sample_usage()),
        final_usage: Some(sample_usage()),
        stop_reason: SchedulerStopReason::ExecutionFailuresDetected,
        last_error: Some(exec_error(backend, message)),
    }
}

fn exec_error(backend: BackendKind, message: &str) -> CleanupError {
    CleanupError::ExecutionFailed {
        backend,
        message: message.to_string(),
    }
}

fn sample_usage() -> UsageSnapshot {
    UsageSnapshot {
        backend: BackendKind::Docker,
        used_bytes: 900,
        total_bytes: Some(1000),
        used_percent: Some(90),
        observed_at: None,
    }
}
