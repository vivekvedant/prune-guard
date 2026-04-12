use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use prune_guard::{
    ActionPlan, BackendKind, CandidateArtifact, CleanupActionKind, CleanupError,
    ExecutionContract, ExecutionMode, ExecutionRequest, ExecutionResponse, PlannedAction,
    ResourceKind, SkippedCandidate,
};
use prune_guard::executor::CleanupExecutor;

#[test]
fn dry_run_plan_never_calls_backend_delete() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(MockExecutor::success(calls.clone()));
    let executor = CleanupExecutor::new(Duration::from_millis(100));
    let plan = action_plan(true, vec![planned_action("img-dry-run", false)], vec![]);

    let report = executor.execute_plan(backend, plan);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(report.completed.len(), 1);
    assert!(report.failures.is_empty());
    assert!(!report.completed[0].executed);
    assert!(report.completed[0].dry_run);
}

#[test]
fn action_level_dry_run_never_calls_backend_delete() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(MockExecutor::success(calls.clone()));
    let executor = CleanupExecutor::new(Duration::from_millis(100));
    let plan = action_plan(false, vec![planned_action("img-action-dry", true)], vec![]);

    let report = executor.execute_plan(backend, plan);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(report.completed.len(), 1);
    assert!(report.failures.is_empty());
    assert!(!report.completed[0].executed);
    assert!(report.completed[0].dry_run);
}

#[test]
fn real_run_executes_actions_when_backend_succeeds() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(MockExecutor::success(calls.clone()));
    let executor = CleanupExecutor::new(Duration::from_millis(100));
    let plan = action_plan(false, vec![planned_action("img-real-run", false)], vec![]);

    let report = executor.execute_plan(backend, plan);

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(report.completed.len(), 1);
    assert!(report.failures.is_empty());
    assert!(report.completed[0].executed);
    assert!(!report.completed[0].dry_run);
}

#[test]
fn executor_captures_per_action_errors_and_continues() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(MockExecutor::always_error(calls.clone()));
    let executor = CleanupExecutor::new(Duration::from_millis(100));
    let plan = action_plan(
        false,
        vec![
            planned_action("img-fail-1", false),
            planned_action("img-fail-2", false),
        ],
        vec![],
    );

    let report = executor.execute_plan(backend, plan);

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(report.completed.is_empty());
    assert_eq!(report.failures.len(), 2);
    assert_eq!(report.failures[0].action.candidate.identifier, "img-fail-1");
    assert_eq!(report.failures[1].action.candidate.identifier, "img-fail-2");
}

#[test]
fn executor_marks_action_as_timeout_when_backend_exceeds_deadline() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(MockExecutor::slow_success(
        calls.clone(),
        Duration::from_millis(60),
    ));
    let executor = CleanupExecutor::new(Duration::from_millis(10));
    let plan = action_plan(false, vec![planned_action("img-slow", false)], vec![]);

    let report = executor.execute_plan(backend, plan);

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(report.completed.is_empty());
    assert_eq!(report.failures.len(), 1);
    match &report.failures[0].error {
        CleanupError::ExecutionFailed { message, .. } => {
            assert!(message.contains("timed out"));
        }
        other => panic!("expected timeout execution failure, got {other:?}"),
    }
}

#[derive(Clone)]
struct MockExecutor {
    calls: Arc<AtomicUsize>,
    mode: MockMode,
}

#[derive(Clone)]
enum MockMode {
    Success,
    AlwaysError,
    SlowSuccess(Duration),
}

impl MockExecutor {
    fn success(calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            mode: MockMode::Success,
        }
    }

    fn always_error(calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            mode: MockMode::AlwaysError,
        }
    }

    fn slow_success(calls: Arc<AtomicUsize>, delay: Duration) -> Self {
        Self {
            calls,
            mode: MockMode::SlowSuccess(delay),
        }
    }
}

impl ExecutionContract for MockExecutor {
    fn execute(&self, request: ExecutionRequest) -> prune_guard::Result<ExecutionResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        match self.mode {
            MockMode::Success => Ok(ExecutionResponse {
                backend: request.backend.clone(),
                candidate: request.action.candidate.clone(),
                executed: matches!(request.mode, ExecutionMode::RealRun),
                dry_run: matches!(request.mode, ExecutionMode::DryRun),
                message: Some("delete executed".to_string()),
            }),
            MockMode::AlwaysError => Err(CleanupError::ExecutionFailed {
                backend: request.backend,
                message: "simulated backend failure".to_string(),
            }),
            MockMode::SlowSuccess(delay) => {
                thread::sleep(delay);
                Ok(ExecutionResponse {
                    backend: request.backend.clone(),
                    candidate: request.action.candidate.clone(),
                    executed: matches!(request.mode, ExecutionMode::RealRun),
                    dry_run: matches!(request.mode, ExecutionMode::DryRun),
                    message: Some("delete executed after delay".to_string()),
                })
            }
        }
    }
}

fn action_plan(
    dry_run: bool,
    actions: Vec<PlannedAction>,
    skipped: Vec<SkippedCandidate>,
) -> ActionPlan {
    ActionPlan {
        backend: BackendKind::Docker,
        dry_run,
        actions,
        skipped,
    }
}

fn planned_action(identifier: &str, dry_run: bool) -> PlannedAction {
    PlannedAction {
        candidate: CandidateArtifact {
            backend: BackendKind::Docker,
            resource_kind: ResourceKind::Image,
            identifier: identifier.to_string(),
            display_name: None,
            labels: BTreeSet::new(),
            size_bytes: Some(1024),
            age_days: Some(30),
            in_use: Some(false),
            referenced: Some(false),
            protected: false,
            metadata_complete: true,
            metadata_ambiguous: false,
            discovered_at: None,
        },
        kind: CleanupActionKind::Delete,
        dry_run,
        reason: Some("planned by test".to_string()),
    }
}
