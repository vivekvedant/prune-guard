use std::collections::BTreeSet;

use prune_guard::{
    ActionPlanningRequest, BackendKind, CandidateArtifact, CleanupConfig, CleanupPlanner,
    ResourceKind, UsageSnapshot,
};

const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn planner_respects_per_run_delete_cap_and_preserves_order() {
    let mut cfg = base_config();
    cfg.max_delete_per_run_gb = 3;
    cfg.dry_run = true;

    let planner = CleanupPlanner::new(cfg.clone());
    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![
            candidate_template("img-1", BackendKind::Docker, Some(BYTES_PER_GIB)),
            candidate_template("img-2", BackendKind::Docker, Some(2 * BYTES_PER_GIB)),
            candidate_template("img-3", BackendKind::Docker, Some(BYTES_PER_GIB)),
        ],
    };

    let plan = planner.plan(request);

    let planned_ids: Vec<&str> = plan
        .actions
        .iter()
        .map(|action| action.candidate.identifier.as_str())
        .collect();
    let skipped_ids: Vec<&str> = plan
        .skipped
        .iter()
        .map(|entry| entry.candidate.identifier.as_str())
        .collect();

    assert_eq!(planned_ids, vec!["img-1", "img-2"]);
    assert_eq!(skipped_ids, vec!["img-3"]);
    assert_eq!(plan.skipped[0].reason, "deletion_cap_reached");
    assert!(plan.actions.iter().all(|action| action.dry_run));
}

#[test]
fn planner_skips_candidate_when_size_is_unknown() {
    let cfg = base_config();
    let planner = CleanupPlanner::new(cfg.clone());
    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![candidate_template(
            "img-size-unknown",
            BackendKind::Docker,
            None,
        )],
    };

    let plan = planner.plan(request);

    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, "candidate_size_unknown");
}

#[test]
fn planner_skips_candidate_when_backend_does_not_match_request() {
    let cfg = base_config();
    let planner = CleanupPlanner::new(cfg.clone());
    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![candidate_template(
            "podman-img",
            BackendKind::Podman,
            Some(BYTES_PER_GIB),
        )],
    };

    let plan = planner.plan(request);

    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, "candidate_backend_mismatch");
}

#[test]
fn planner_carries_policy_rejection_reasons() {
    let cfg = base_config();
    let planner = CleanupPlanner::new(cfg.clone());
    let mut candidate = candidate_template(
        "img-policy-reject",
        BackendKind::Docker,
        Some(BYTES_PER_GIB),
    );
    candidate.metadata_complete = false;

    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![candidate],
    };

    let plan = planner.plan(request);

    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, "metadata_incomplete");
}

fn base_config() -> CleanupConfig {
    CleanupConfig {
        min_unused_age_days: 14,
        max_delete_per_run_gb: 5,
        dry_run: true,
        ..CleanupConfig::default()
    }
}

fn usage_template() -> UsageSnapshot {
    UsageSnapshot {
        backend: BackendKind::Docker,
        used_bytes: 90 * BYTES_PER_GIB,
        total_bytes: Some(100 * BYTES_PER_GIB),
        used_percent: Some(90),
        observed_at: None,
    }
}

fn candidate_template(
    identifier: &str,
    backend: BackendKind,
    size_bytes: Option<u64>,
) -> CandidateArtifact {
    CandidateArtifact {
        backend,
        resource_kind: ResourceKind::Image,
        identifier: identifier.to_string(),
        display_name: None,
        labels: BTreeSet::new(),
        size_bytes,
        age_days: Some(30),
        in_use: Some(false),
        referenced: Some(false),
        protected: false,
        metadata_complete: true,
        metadata_ambiguous: false,
        discovered_at: None,
    }
}
