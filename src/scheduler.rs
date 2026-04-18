use crate::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use crate::domain::{
    ActionPlanningRequest, BackendKind, CandidateDiscoveryRequest, CleanupConfig, UsageSnapshot,
};
use crate::error::CleanupError;
use crate::planner::SKIPPED_REASON_DELETION_CAP_REACHED;
use crate::{CleanupExecutor, CleanupPlanner, Result, SkippedCandidate};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Conservative upper bound for per-cycle cleanup iterations.
///
/// Why this exists:
/// - prevents runaway loops when usage never drops as expected
/// - guarantees bounded work per scheduler tick
const DEFAULT_MAX_CYCLE_ITERATIONS: usize = 128;

/// Stop reason for one scheduler cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerStopReason {
    /// Backend reported unhealthy status.
    BackendUnhealthy,
    /// Health check call itself failed.
    HealthCheckFailed,
    /// Usage collection failed.
    UsageCollectionFailed,
    /// Usage was collected but percent could not be determined.
    UsagePercentUnknown,
    /// Current usage is below high watermark so cleanup is not needed.
    BelowHighWatermark,
    /// Cleanup loop lowered usage to target watermark.
    TargetWatermarkReached,
    /// Candidate discovery failed and cycle terminated fail-closed.
    CandidateDiscoveryFailed,
    /// One or more action executions failed; cycle stops immediately fail-closed.
    ExecutionFailuresDetected,
    /// Planner returned no executable actions.
    NoActionableCandidates,
    /// Planner signaled deletion cap pressure and produced no actions.
    DeletionCapReached,
    /// Safety guard: maximum loop iterations reached.
    IterationLimitReached,
}

/// Summary for one scheduler tick/cycle.
///
/// Safety intent:
/// - makes every stop path explicit and auditable
/// - captures whether cleanup started or the cycle no-oped
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerRunReport {
    pub backend: BackendKind,
    pub dry_run: bool,
    pub cleanup_started: bool,
    pub iterations: usize,
    pub actions_planned: usize,
    pub actions_completed: usize,
    pub reclaimed_estimated_bytes: u64,
    pub action_failures: usize,
    pub skipped_candidates: usize,
    pub initial_usage: Option<UsageSnapshot>,
    pub final_usage: Option<UsageSnapshot>,
    pub stop_reason: SchedulerStopReason,
    pub last_error: Option<CleanupError>,
}

impl SchedulerRunReport {
    fn new(backend: BackendKind, dry_run: bool) -> Self {
        Self {
            backend,
            dry_run,
            cleanup_started: false,
            iterations: 0,
            actions_planned: 0,
            actions_completed: 0,
            reclaimed_estimated_bytes: 0,
            action_failures: 0,
            skipped_candidates: 0,
            initial_usage: None,
            final_usage: None,
            stop_reason: SchedulerStopReason::BelowHighWatermark,
            last_error: None,
        }
    }
}

/// Phase 4 scheduler and watermark loop.
///
/// Safety role:
/// - only starts cleanup at/above high watermark
/// - re-checks usage after every execution batch
/// - stops when target watermark is reached, planner has no actions, or loop safety cap hits
/// - fail-closed on uncertainty/errors by stopping the cycle without forcing execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupScheduler {
    config: CleanupConfig,
    planner: CleanupPlanner,
    executor: CleanupExecutor,
    max_cycle_iterations: usize,
}

impl CleanupScheduler {
    /// Build scheduler with conservative defaults.
    pub fn new(config: CleanupConfig) -> Self {
        Self::with_limits(
            config,
            Duration::from_secs(30),
            DEFAULT_MAX_CYCLE_ITERATIONS,
        )
    }

    /// Build scheduler with explicit action timeout and loop-iteration limit.
    pub fn with_limits(
        config: CleanupConfig,
        action_timeout: Duration,
        max_cycle_iterations: usize,
    ) -> Self {
        Self {
            planner: CleanupPlanner::new(config.clone()),
            executor: CleanupExecutor::new(action_timeout),
            config,
            max_cycle_iterations: max_cycle_iterations.max(1),
        }
    }

    pub fn config(&self) -> &CleanupConfig {
        &self.config
    }

    /// Run a single scheduler tick.
    pub fn run_once<B>(&self, backend: Arc<B>) -> Result<SchedulerRunReport>
    where
        B: HealthCheck
            + UsageCollector
            + CandidateDiscoverer
            + ExecutionContract
            + Send
            + Sync
            + 'static,
    {
        let backend_kind = backend.backend_kind();
        let mut report = SchedulerRunReport::new(backend_kind.clone(), self.config.dry_run);

        match backend.health_check() {
            Ok(health_report) if health_report.healthy => {}
            Ok(_) => {
                report.stop_reason = SchedulerStopReason::BackendUnhealthy;
                return Ok(report);
            }
            Err(error) => {
                report.stop_reason = SchedulerStopReason::HealthCheckFailed;
                report.last_error = Some(error);
                return Ok(report);
            }
        }

        let mut usage = match backend.collect_usage() {
            Ok(usage) => usage,
            Err(error) => {
                report.stop_reason = SchedulerStopReason::UsageCollectionFailed;
                report.last_error = Some(error);
                return Ok(report);
            }
        };
        report.initial_usage = Some(usage.clone());

        let mut used_percent = match usage.percent_used() {
            Some(percent) => percent,
            None => {
                report.stop_reason = SchedulerStopReason::UsagePercentUnknown;
                report.final_usage = Some(usage);
                return Ok(report);
            }
        };

        if used_percent < self.config.high_watermark_percent {
            report.stop_reason = SchedulerStopReason::BelowHighWatermark;
            report.final_usage = Some(usage);
            return Ok(report);
        }

        report.cleanup_started = true;

        for iteration_index in 0..self.max_cycle_iterations {
            if iteration_index > 0 {
                // Re-validate backend health before any later-loop destructive path.
                match backend.health_check() {
                    Ok(health_report) if health_report.healthy => {}
                    Ok(_) => {
                        report.stop_reason = SchedulerStopReason::BackendUnhealthy;
                        report.final_usage = Some(usage);
                        return Ok(report);
                    }
                    Err(error) => {
                        report.stop_reason = SchedulerStopReason::HealthCheckFailed;
                        report.last_error = Some(error);
                        report.final_usage = Some(usage);
                        return Ok(report);
                    }
                }
            }

            if used_percent <= self.config.target_watermark_percent {
                report.stop_reason = SchedulerStopReason::TargetWatermarkReached;
                report.final_usage = Some(usage);
                return Ok(report);
            }

            report.iterations += 1;

            let discovery_request = CandidateDiscoveryRequest {
                backend: backend_kind.clone(),
                config: self.config.clone(),
                usage: usage.clone(),
            };

            let discovered = match backend.discover_candidates(discovery_request) {
                Ok(discovered) => discovered,
                Err(error) => {
                    report.stop_reason = SchedulerStopReason::CandidateDiscoveryFailed;
                    report.last_error = Some(error);
                    report.final_usage = Some(usage);
                    return Ok(report);
                }
            };

            let plan = self.planner.plan(ActionPlanningRequest {
                backend: backend_kind.clone(),
                config: self.config.clone(),
                usage: usage.clone(),
                candidates: discovered.candidates,
            });

            report.actions_planned += plan.actions.len();
            report.skipped_candidates += plan.skipped.len();

            if plan.actions.is_empty() {
                report.stop_reason = no_action_stop_reason(&plan.skipped);
                report.final_usage = Some(usage);
                return Ok(report);
            }

            let execution_report = self.executor.execute_plan(Arc::clone(&backend), plan);
            report.reclaimed_estimated_bytes += execution_report
                .completed
                .iter()
                .filter(|response| response.executed)
                .filter_map(|response| response.candidate.size_bytes)
                .sum::<u64>();
            report.actions_completed += execution_report.completed.len();
            report.action_failures += execution_report.failures.len();
            if let Some(failure) = execution_report.failures.first() {
                report.stop_reason = SchedulerStopReason::ExecutionFailuresDetected;
                report.last_error = Some(failure.error.clone());
                report.final_usage = Some(usage);
                return Ok(report);
            }

            usage = match backend.collect_usage() {
                Ok(usage) => usage,
                Err(error) => {
                    report.stop_reason = SchedulerStopReason::UsageCollectionFailed;
                    report.last_error = Some(error);
                    report.final_usage = Some(usage);
                    return Ok(report);
                }
            };

            used_percent = match usage.percent_used() {
                Some(percent) => percent,
                None => {
                    report.stop_reason = SchedulerStopReason::UsagePercentUnknown;
                    report.final_usage = Some(usage);
                    return Ok(report);
                }
            };
        }

        report.stop_reason = SchedulerStopReason::IterationLimitReached;
        report.final_usage = Some(usage);
        Ok(report)
    }

    /// Run repeated scheduler ticks with interval sleep between ticks.
    ///
    /// This provides the periodic daemon behavior while keeping each tick bounded
    /// by `run_once` safety gates.
    pub fn run_for_ticks<B>(&self, backend: Arc<B>, ticks: usize) -> Result<Vec<SchedulerRunReport>>
    where
        B: HealthCheck
            + UsageCollector
            + CandidateDiscoverer
            + ExecutionContract
            + Send
            + Sync
            + 'static,
    {
        let mut reports = Vec::with_capacity(ticks);

        for tick_index in 0..ticks {
            reports.push(self.run_once(Arc::clone(&backend))?);
            if tick_index + 1 < ticks {
                thread::sleep(Duration::from_secs(self.config.interval_secs));
            }
        }

        Ok(reports)
    }
}

fn no_action_stop_reason(skipped: &[SkippedCandidate]) -> SchedulerStopReason {
    if skipped
        .iter()
        .any(|entry| entry.reason == SKIPPED_REASON_DELETION_CAP_REACHED)
    {
        SchedulerStopReason::DeletionCapReached
    } else {
        SchedulerStopReason::NoActionableCandidates
    }
}
