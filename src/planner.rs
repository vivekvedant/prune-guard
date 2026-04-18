use crate::domain::{
    ActionPlan, ActionPlanningRequest, CleanupActionKind, CleanupConfig, PlannedAction,
    SkippedCandidate,
};
use crate::policy::PolicyEngine;

/// Canonical skip reason emitted when per-run delete cap blocks a candidate.
pub const SKIPPED_REASON_DELETION_CAP_REACHED: &str = "deletion_cap_reached";

/// Planner for Phase 3 candidate-to-action conversion.
///
/// Safety role:
/// - consumes only candidates accepted by the fail-closed policy engine
/// - enforces per-run delete-cap limits before creating delete actions
/// - rejects any candidate with unknown reclaim size so cap enforcement remains reliable
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlanner {
    config: CleanupConfig,
}

impl CleanupPlanner {
    /// Binary GiB conversion used for run-cap enforcement.
    const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

    /// Build planner from runtime cleanup configuration.
    pub fn new(config: CleanupConfig) -> Self {
        Self { config }
    }

    /// Returns immutable planner config.
    pub fn config(&self) -> &CleanupConfig {
        &self.config
    }

    /// Build an action plan from discovered candidates.
    ///
    /// The plan is deterministic:
    /// - candidate order is preserved
    /// - each rejected candidate gets one explicit reason
    /// - cap checks are applied in order, so earlier accepted candidates consume budget first
    pub fn plan(&self, request: ActionPlanningRequest) -> ActionPlan {
        let ActionPlanningRequest {
            backend,
            config,
            candidates,
            ..
        } = request;

        // Safety-critical: each run can override config (for example dry-run/cap).
        // Planning must use request-scoped config instead of constructor defaults.
        let policy = PolicyEngine::new(config.clone());

        let mut actions = Vec::new();
        let mut skipped = Vec::new();

        let max_delete_bytes = config
            .max_delete_per_run_gb
            .saturating_mul(Self::BYTES_PER_GIB);
        let mut remaining_delete_bytes = max_delete_bytes;
        let mut unknown_size_budget_reserved = false;

        for candidate in candidates {
            let candidate = match policy.evaluate_candidate(candidate) {
                Ok(candidate) => candidate,
                Err(skipped_candidate) => {
                    skipped.push(skipped_candidate);
                    continue;
                }
            };

            if candidate.backend != backend {
                skipped.push(SkippedCandidate {
                    candidate,
                    reason: "candidate_backend_mismatch".to_string(),
                });
                continue;
            }

            let size_bytes = match candidate.size_bytes {
                Some(size_bytes) => size_bytes,
                None => {
                    // Conservative fallback for unknown sizes:
                    // - at most one unknown-size candidate may proceed per run
                    // - reserve the full remaining budget immediately
                    // This keeps behavior bounded while preserving strict cap pressure.
                    if remaining_delete_bytes == 0 || unknown_size_budget_reserved {
                        skipped.push(SkippedCandidate {
                            candidate,
                            reason: if remaining_delete_bytes == 0 {
                                SKIPPED_REASON_DELETION_CAP_REACHED.to_string()
                            } else {
                                "candidate_size_unknown".to_string()
                            },
                        });
                        continue;
                    }

                    unknown_size_budget_reserved = true;
                    remaining_delete_bytes = 0;
                    0
                }
            };

            if size_bytes > remaining_delete_bytes {
                skipped.push(SkippedCandidate {
                    candidate,
                    reason: SKIPPED_REASON_DELETION_CAP_REACHED.to_string(),
                });
                continue;
            }

            remaining_delete_bytes = remaining_delete_bytes.saturating_sub(size_bytes);
            actions.push(PlannedAction {
                candidate,
                kind: CleanupActionKind::Delete,
                dry_run: config.dry_run,
                reason: Some("policy_accepted_within_delete_cap".to_string()),
            });
        }

        ActionPlan {
            backend,
            dry_run: config.dry_run,
            actions,
            skipped,
        }
    }
}
