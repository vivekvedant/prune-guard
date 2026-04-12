use crate::backend::ExecutionContract;
use crate::domain::{
    ActionPlan, BackendKind, ExecutionMode, ExecutionRequest, ExecutionResponse, PlannedAction,
};
use crate::error::CleanupError;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Action-level execution failure captured by the batch executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionExecutionFailure {
    /// Action that failed.
    pub action: PlannedAction,
    /// Failure details (backend error or timeout wrapper error).
    pub error: CleanupError,
}

/// Batch execution output for one action plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    /// Backend the plan belongs to.
    pub backend: BackendKind,
    /// Plan-level dry-run mode.
    pub dry_run: bool,
    /// Successful outcomes (includes synthetic dry-run responses).
    pub completed: Vec<ExecutionResponse>,
    /// Per-action failures captured without aborting the whole batch.
    pub failures: Vec<ActionExecutionFailure>,
}

impl ExecutionReport {
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

/// Phase 3 execution wrapper for safe plan execution.
///
/// Safety role:
/// - never calls backend delete when dry-run is enabled at plan or action level
/// - wraps real executions in timeout guards
/// - captures per-action errors and continues processing remaining actions
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupExecutor {
    action_timeout: Duration,
}

impl CleanupExecutor {
    /// Build executor with a per-action timeout budget.
    pub fn new(action_timeout: Duration) -> Self {
        Self { action_timeout }
    }

    pub fn action_timeout(&self) -> Duration {
        self.action_timeout
    }

    /// Execute a full action plan, preserving action order in both outcomes and failures.
    pub fn execute_plan<B>(&self, backend: Arc<B>, plan: ActionPlan) -> ExecutionReport
    where
        B: ExecutionContract + Send + Sync + 'static,
    {
        let mut completed = Vec::new();
        let mut failures = Vec::new();

        let plan_backend = plan.backend.clone();
        let plan_dry_run = plan.dry_run;

        for action in plan.actions {
            if plan_dry_run || action.dry_run {
                completed.push(self.synthetic_dry_run_response(plan_backend.clone(), action));
                continue;
            }

            let request = ExecutionRequest {
                backend: plan_backend.clone(),
                action: action.clone(),
                mode: ExecutionMode::RealRun,
            };

            match self.execute_with_timeout(Arc::clone(&backend), request) {
                Ok(response) => completed.push(response),
                Err(error) => failures.push(ActionExecutionFailure { action, error }),
            }
        }

        ExecutionReport {
            backend: plan_backend,
            dry_run: plan_dry_run,
            completed,
            failures,
        }
    }

    fn synthetic_dry_run_response(
        &self,
        backend: BackendKind,
        action: PlannedAction,
    ) -> ExecutionResponse {
        ExecutionResponse {
            backend,
            candidate: action.candidate,
            executed: false,
            dry_run: true,
            message: Some("dry_run_no_delete_executed".to_string()),
        }
    }

    fn execute_with_timeout<B>(
        &self,
        backend: Arc<B>,
        request: ExecutionRequest,
    ) -> Result<ExecutionResponse, CleanupError>
    where
        B: ExecutionContract + Send + Sync + 'static,
    {
        let backend_kind = request.backend.clone();
        let candidate_id = request.action.candidate.identifier.clone();

        let (tx, rx) = mpsc::channel();

        // Timeout wrapper runs execution in a dedicated thread. If timeout is hit,
        // this method returns a failure and continues; the worker thread may finish later.
        thread::spawn(move || {
            let result = backend.execute(request);
            let _ = tx.send(result);
        });

        match rx.recv_timeout(self.action_timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(CleanupError::ExecutionFailed {
                backend: backend_kind,
                message: format!(
                    "action timed out after {}ms for candidate {}",
                    self.action_timeout.as_millis(),
                    candidate_id
                ),
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(CleanupError::ExecutionFailed {
                backend: backend_kind,
                message: format!(
                    "execution worker disconnected before completion for candidate {}",
                    candidate_id
                ),
            }),
        }
    }
}
