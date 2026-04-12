use crate::domain::{
    ActionPlan, ActionPlanningRequest, BackendKind, CandidateDiscoveryRequest,
    CandidateDiscoveryResponse, ExecutionRequest, ExecutionResponse, HealthReport, UsageSnapshot,
};
use crate::error::Result;

/// Backend contract layer.
///
/// These traits define a pipeline shape that every backend must follow:
/// 1. health check
/// 2. usage collection
/// 3. candidate discovery
/// 4. action planning
/// 5. execution
///
/// Splitting responsibilities keeps each stage testable and easier to reason
/// about, especially for safety checks.
pub trait HealthCheck {
    /// Identifies which backend this implementation belongs to.
    fn backend_kind(&self) -> BackendKind;
    /// Verifies backend is available/healthy before any destructive path.
    fn health_check(&self) -> Result<HealthReport>;
}

pub trait UsageCollector {
    /// Collects current storage usage for watermark decisions.
    fn collect_usage(&self) -> Result<UsageSnapshot>;
}

pub trait CandidateDiscoverer {
    /// Discovers potentially removable artifacts. This is discovery only, not
    /// final safety approval.
    fn discover_candidates(
        &self,
        request: CandidateDiscoveryRequest,
    ) -> Result<CandidateDiscoveryResponse>;
}

pub trait ActionPlanner {
    /// Converts discovered artifacts into an action plan after policy checks.
    fn plan_actions(&self, request: ActionPlanningRequest) -> Result<ActionPlan>;
}

pub trait ExecutionContract {
    /// Executes one planned action in dry-run or real-run mode.
    fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResponse>;
}

/// Convenience trait alias for a fully-capable backend adapter.
pub trait CleanupBackend:
    HealthCheck + UsageCollector + CandidateDiscoverer + ActionPlanner + ExecutionContract
{
}

impl<T> CleanupBackend for T where
    T: HealthCheck + UsageCollector + CandidateDiscoverer + ActionPlanner + ExecutionContract
{
}

pub trait HealthCheckContract: HealthCheck {}
impl<T: HealthCheck> HealthCheckContract for T {}

pub trait UsageCollectorContract: UsageCollector {}
impl<T: UsageCollector> UsageCollectorContract for T {}

pub trait CandidateDiscovererContract: CandidateDiscoverer {}
impl<T: CandidateDiscoverer> CandidateDiscovererContract for T {}

pub trait ActionPlannerContract: ActionPlanner {}
impl<T: ActionPlanner> ActionPlannerContract for T {}

pub trait ExecutionExecutor: ExecutionContract {}
impl<T: ExecutionContract> ExecutionExecutor for T {}
