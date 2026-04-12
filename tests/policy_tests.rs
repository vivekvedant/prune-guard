use std::collections::BTreeSet;

use prune_guard::{
    BackendKind, CandidateArtifact, CleanupConfig, PolicyEngine, PolicyEvaluation, ResourceKind,
};

#[test]
fn rejects_candidate_with_incomplete_metadata() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-incomplete", ResourceKind::Image);
    candidate.metadata_complete = false;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("incomplete metadata must fail closed");

    assert_eq!(rejected.reason, "metadata_incomplete");
}

#[test]
fn rejects_candidate_with_ambiguous_metadata() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-ambiguous", ResourceKind::Image);
    candidate.metadata_ambiguous = true;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("ambiguous metadata must fail closed");

    assert_eq!(rejected.reason, "metadata_ambiguous");
}

#[test]
fn rejects_candidate_already_marked_protected() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-protected", ResourceKind::Image);
    candidate.protected = true;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("pre-protected candidates must be rejected");

    assert_eq!(rejected.reason, "candidate_marked_protected");
}

#[test]
fn rejects_allowlisted_image_by_display_name() {
    let mut cfg = base_config();
    cfg.protected_images = vec!["keep-this-image".to_string()];

    let engine = PolicyEngine::new(cfg);
    let mut candidate = candidate_template("sha256:111", ResourceKind::Image);
    candidate.display_name = Some("keep-this-image".to_string());

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("allowlisted image must be rejected");

    assert_eq!(rejected.reason, "protected_image_allowlist");
}

#[test]
fn rejects_allowlisted_volume_by_identifier() {
    let mut cfg = base_config();
    cfg.protected_volumes = vec!["pgdata".to_string()];

    let engine = PolicyEngine::new(cfg);
    let candidate = candidate_template("pgdata", ResourceKind::Volume);

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("allowlisted volume must be rejected");

    assert_eq!(rejected.reason, "protected_volume_allowlist");
}

#[test]
fn rejects_candidate_with_protected_label_match() {
    let mut cfg = base_config();
    cfg.protected_labels = vec!["owner=ops".to_string()];

    let engine = PolicyEngine::new(cfg);
    let mut candidate = candidate_template("img-labeled", ResourceKind::Image);
    candidate.labels.insert("owner=ops".to_string());

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("protected label must reject candidate");

    assert_eq!(rejected.reason, "protected_label_allowlist");
}

#[test]
fn rejects_candidate_when_in_use_is_true() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-in-use", ResourceKind::Image);
    candidate.in_use = Some(true);

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("in-use candidate must be rejected");

    assert_eq!(rejected.reason, "candidate_in_use");
}

#[test]
fn rejects_candidate_when_in_use_is_unknown() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-in-use-unknown", ResourceKind::Image);
    candidate.in_use = None;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("unknown in-use state must fail closed");

    assert_eq!(rejected.reason, "candidate_in_use_unknown");
}

#[test]
fn rejects_candidate_when_referenced_is_true() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-referenced", ResourceKind::Image);
    candidate.referenced = Some(true);

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("referenced candidate must be rejected");

    assert_eq!(rejected.reason, "candidate_referenced");
}

#[test]
fn rejects_candidate_when_referenced_is_unknown() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-referenced-unknown", ResourceKind::Image);
    candidate.referenced = None;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("unknown referenced state must fail closed");

    assert_eq!(rejected.reason, "candidate_referenced_unknown");
}

#[test]
fn rejects_candidate_when_age_is_unknown() {
    let engine = PolicyEngine::new(base_config());
    let mut candidate = candidate_template("img-age-unknown", ResourceKind::Image);
    candidate.age_days = None;

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("unknown age must fail closed");

    assert_eq!(rejected.reason, "candidate_age_unknown");
}

#[test]
fn rejects_candidate_younger_than_min_unused_age() {
    let mut cfg = base_config();
    cfg.min_unused_age_days = 14;

    let engine = PolicyEngine::new(cfg);
    let mut candidate = candidate_template("img-too-new", ResourceKind::Image);
    candidate.age_days = Some(7);

    let rejected = engine
        .evaluate_candidate(candidate)
        .expect_err("young candidate must be rejected");

    assert_eq!(rejected.reason, "candidate_too_new");
}

#[test]
fn accepts_candidate_when_all_safety_signals_are_clear() {
    let engine = PolicyEngine::new(base_config());
    let candidate = candidate_template("img-ok", ResourceKind::Image);

    let accepted = engine
        .evaluate_candidate(candidate.clone())
        .expect("fully eligible candidate should be accepted");

    assert_eq!(accepted.identifier, candidate.identifier);
}

#[test]
fn evaluate_candidates_is_deterministic_and_preserves_input_order() {
    let mut cfg = base_config();
    cfg.protected_images = vec!["reject-me".to_string()];
    let engine = PolicyEngine::new(cfg);

    let mut rejected_candidate = candidate_template("img-2", ResourceKind::Image);
    rejected_candidate.display_name = Some("reject-me".to_string());

    let candidates = vec![
        candidate_template("img-1", ResourceKind::Image),
        rejected_candidate,
        candidate_template("img-3", ResourceKind::Image),
    ];

    let PolicyEvaluation { accepted, skipped } = engine.evaluate_candidates(candidates);

    let accepted_ids: Vec<&str> = accepted.iter().map(|c| c.identifier.as_str()).collect();
    let skipped_ids: Vec<&str> = skipped.iter().map(|s| s.candidate.identifier.as_str()).collect();

    assert_eq!(accepted_ids, vec!["img-1", "img-3"]);
    assert_eq!(skipped_ids, vec!["img-2"]);
    assert_eq!(skipped[0].reason, "protected_image_allowlist");
}

fn base_config() -> CleanupConfig {
    CleanupConfig {
        min_unused_age_days: 14,
        ..CleanupConfig::default()
    }
}

fn candidate_template(identifier: &str, resource_kind: ResourceKind) -> CandidateArtifact {
    CandidateArtifact {
        backend: BackendKind::Docker,
        resource_kind,
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
    }
}
