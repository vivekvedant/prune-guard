use std::collections::BTreeSet;

use prune_guard::{BackendKind, CandidateArtifact, ResourceKind};

#[test]
fn candidate_is_fail_closed_when_metadata_is_incomplete() {
    let candidate = candidate_template();
    assert!(
        !candidate.is_actionable(),
        "incomplete metadata should be rejected"
    );
}

#[test]
fn candidate_is_fail_closed_when_metadata_is_ambiguous() {
    let mut candidate = candidate_template();
    candidate.metadata_complete = true;
    candidate.metadata_ambiguous = true;
    candidate.in_use = Some(false);
    candidate.referenced = Some(false);
    candidate.age_days = Some(42);

    assert!(
        !candidate.is_actionable(),
        "ambiguous metadata should be rejected"
    );
}

#[test]
fn candidate_is_actionable_only_when_all_safety_signals_are_clear() {
    let mut candidate = candidate_template();
    candidate.metadata_complete = true;
    candidate.metadata_ambiguous = false;
    candidate.in_use = Some(false);
    candidate.referenced = Some(false);
    candidate.age_days = Some(42);

    assert!(candidate.is_actionable());
}

fn candidate_template() -> CandidateArtifact {
    CandidateArtifact {
        backend: BackendKind::Docker,
        resource_kind: ResourceKind::Image,
        identifier: "sha256:abc".to_string(),
        display_name: Some("test-image".to_string()),
        labels: BTreeSet::new(),
        size_bytes: Some(1024),
        age_days: None,
        in_use: None,
        referenced: None,
        protected: false,
        metadata_complete: false,
        metadata_ambiguous: false,
        discovered_at: None,
    }
}
