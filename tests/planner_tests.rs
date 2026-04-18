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
fn planner_uses_conservative_fallback_for_first_unknown_size_candidate() {
    let cfg = base_config();
    let planner = CleanupPlanner::new(cfg.clone());
    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![
            candidate_template("img-size-unknown-1", BackendKind::Docker, None),
            candidate_template("img-size-unknown-2", BackendKind::Docker, None),
        ],
    };

    let plan = planner.plan(request);

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].candidate.identifier, "img-size-unknown-1");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].candidate.identifier, "img-size-unknown-2");
    assert_eq!(plan.skipped[0].reason, "deletion_cap_reached");
}

#[test]
fn planner_chunks_oversized_build_cache_candidate_to_remaining_cap() {
    let mut cfg = base_config();
    cfg.max_delete_per_run_gb = 10;
    cfg.dry_run = false;
    let planner = CleanupPlanner::new(cfg.clone());

    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![CandidateArtifact {
            backend: BackendKind::Docker,
            resource_kind: ResourceKind::BuildCache,
            identifier: "docker-build-cache-unused".to_string(),
            display_name: Some("docker-build-cache".to_string()),
            labels: BTreeSet::new(),
            size_bytes: Some(12 * BYTES_PER_GIB),
            age_days: Some(30),
            in_use: Some(false),
            referenced: Some(false),
            protected: false,
            metadata_complete: true,
            metadata_ambiguous: false,
            discovered_at: None,
        }],
    };

    let plan = planner.plan(request);

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(
        plan.actions[0].candidate.resource_kind,
        ResourceKind::BuildCache
    );
    assert_eq!(
        plan.actions[0].candidate.size_bytes,
        Some(10 * BYTES_PER_GIB)
    );
    assert!(plan.skipped.is_empty());
}

#[test]
fn planner_skips_unknown_size_candidate_when_delete_budget_is_zero() {
    let mut cfg = base_config();
    cfg.max_delete_per_run_gb = 0;
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
    assert_eq!(plan.skipped[0].reason, "deletion_cap_reached");
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

#[test]
fn planner_uses_request_config_for_policy_cap_and_dry_run() {
    let mut planner_cfg = base_config();
    planner_cfg.min_unused_age_days = 7;
    planner_cfg.max_delete_per_run_gb = 10;
    planner_cfg.dry_run = false;
    let planner = CleanupPlanner::new(planner_cfg);

    let mut request_cfg = base_config();
    request_cfg.min_unused_age_days = 30;
    request_cfg.max_delete_per_run_gb = 1;
    request_cfg.dry_run = true;

    let mut policy_rejected = candidate_template(
        "img-policy-rejected-by-request-config",
        BackendKind::Docker,
        Some(BYTES_PER_GIB),
    );
    policy_rejected.age_days = Some(20);

    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: request_cfg,
        usage: usage_template(),
        candidates: vec![
            policy_rejected,
            candidate_template("img-action", BackendKind::Docker, Some(BYTES_PER_GIB)),
            candidate_template("img-cap-rejected", BackendKind::Docker, Some(BYTES_PER_GIB)),
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
    let skipped_reasons: Vec<&str> = plan
        .skipped
        .iter()
        .map(|entry| entry.reason.as_str())
        .collect();

    assert_eq!(planned_ids, vec!["img-action"]);
    assert_eq!(
        skipped_ids,
        vec!["img-policy-rejected-by-request-config", "img-cap-rejected"]
    );
    assert_eq!(
        skipped_reasons,
        vec!["candidate_too_new", "deletion_cap_reached"]
    );
    assert!(plan.dry_run);
    assert!(plan.actions.iter().all(|action| action.dry_run));
}

#[test]
fn planner_preserves_skipped_order_across_policy_and_planner_rejections() {
    let mut cfg = base_config();
    cfg.max_delete_per_run_gb = 1;
    let planner = CleanupPlanner::new(cfg.clone());

    let mut policy_rejected = candidate_template(
        "img-policy-second",
        BackendKind::Docker,
        Some(BYTES_PER_GIB),
    );
    policy_rejected.metadata_complete = false;

    let request = ActionPlanningRequest {
        backend: BackendKind::Docker,
        config: cfg,
        usage: usage_template(),
        candidates: vec![
            candidate_template(
                "img-cap-first",
                BackendKind::Docker,
                Some(2 * BYTES_PER_GIB),
            ),
            policy_rejected,
            candidate_template(
                "img-backend-third",
                BackendKind::Podman,
                Some(BYTES_PER_GIB),
            ),
            candidate_template("img-size-fourth", BackendKind::Docker, None),
        ],
    };

    let plan = planner.plan(request);

    let skipped_ids: Vec<&str> = plan
        .skipped
        .iter()
        .map(|entry| entry.candidate.identifier.as_str())
        .collect();
    let skipped_reasons: Vec<&str> = plan
        .skipped
        .iter()
        .map(|entry| entry.reason.as_str())
        .collect();
    let planned_ids: Vec<&str> = plan
        .actions
        .iter()
        .map(|action| action.candidate.identifier.as_str())
        .collect();

    assert_eq!(planned_ids, vec!["img-size-fourth"]);
    assert_eq!(
        skipped_ids,
        vec!["img-cap-first", "img-policy-second", "img-backend-third"]
    );
    assert_eq!(
        skipped_reasons,
        vec![
            "deletion_cap_reached",
            "metadata_incomplete",
            "candidate_backend_mismatch"
        ]
    );
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
