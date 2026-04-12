use crate::domain::{
    ActionPlan, ActionPlanningRequest, CleanupActionKind, CleanupConfig, PlannedAction,
    SkippedCandidate,
};
use crate::policy::PolicyEngine;

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
            candidates,
            ..
        } = request;

        let policy = PolicyEngine::new(self.config.clone());
        let evaluation = policy.evaluate_candidates(candidates);

        let mut actions = Vec::new();
        let mut skipped = evaluation.skipped;

        let max_delete_bytes = self
            .config
            .max_delete_per_run_gb
            .saturating_mul(Self::BYTES_PER_GIB);
        let mut remaining_delete_bytes = max_delete_bytes;

        for candidate in evaluation.accepted {
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
                    // Fail closed: if reclaimed size is unknown we cannot safely enforce the cap.
                    skipped.push(SkippedCandidate {
                        candidate,
                        reason: "candidate_size_unknown".to_string(),
                    });
                    continue;
                }
            };

            if size_bytes > remaining_delete_bytes {
                skipped.push(SkippedCandidate {
                    candidate,
                    reason: "deletion_cap_reached".to_string(),
                });
                continue;
            }

            remaining_delete_bytes = remaining_delete_bytes.saturating_sub(size_bytes);
            actions.push(PlannedAction {
                candidate,
                kind: CleanupActionKind::Delete,
                dry_run: self.config.dry_run,
                reason: Some("policy_accepted_within_delete_cap".to_string()),
            });
        }

        ActionPlan {
            backend,
            dry_run: self.config.dry_run,
            actions,
            skipped,
        }
    }
}
