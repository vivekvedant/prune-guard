use crate::domain::{CandidateArtifact, CleanupConfig, ResourceKind, SkippedCandidate};

/// Policy engine for Phase 2 candidate filtering.
///
/// Safety role:
/// - enforce a deterministic, fail-closed acceptance contract
/// - keep delete eligibility decisions centralized and auditable
/// - produce explicit skip reasons for every rejected candidate
///
/// Every unknown signal is treated as unsafe. If the engine cannot confidently
/// prove a candidate is safe to delete, it rejects that candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEngine {
    config: CleanupConfig,
}

/// Batch policy evaluation output.
///
/// `accepted` contains candidates that passed all gates.
/// `skipped` contains candidates rejected with explicit reasons for logs/audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEvaluation {
    pub accepted: Vec<CandidateArtifact>,
    pub skipped: Vec<SkippedCandidate>,
}

impl PolicyEngine {
    /// Build a policy engine from runtime cleanup configuration.
    pub fn new(config: CleanupConfig) -> Self {
        Self { config }
    }

    /// Returns the immutable configuration this engine evaluates against.
    pub fn config(&self) -> &CleanupConfig {
        &self.config
    }

    /// Evaluate one candidate and return either the approved candidate or a
    /// skip record containing the deterministic reject reason.
    pub fn evaluate_candidate(
        &self,
        candidate: CandidateArtifact,
    ) -> std::result::Result<CandidateArtifact, SkippedCandidate> {
        if let Some(reason) = self.rejection_reason(&candidate) {
            Err(SkippedCandidate {
                candidate,
                reason: reason.to_string(),
            })
        } else {
            Ok(candidate)
        }
    }

    /// Evaluate all candidates while preserving input order for both accepted
    /// and skipped lists. Stable ordering is important for deterministic tests
    /// and predictable run reports.
    pub fn evaluate_candidates(&self, candidates: Vec<CandidateArtifact>) -> PolicyEvaluation {
        let mut accepted = Vec::new();
        let mut skipped = Vec::new();

        for candidate in candidates {
            match self.evaluate_candidate(candidate) {
                Ok(candidate) => accepted.push(candidate),
                Err(skipped_candidate) => skipped.push(skipped_candidate),
            }
        }

        PolicyEvaluation { accepted, skipped }
    }

    /// Ordered fail-closed checks.
    ///
    /// The check order is intentional and stable so each candidate emits exactly
    /// one primary reject reason. This keeps policy outcomes deterministic.
    fn rejection_reason(&self, candidate: &CandidateArtifact) -> Option<&'static str> {
        if !candidate.metadata_complete {
            return Some("metadata_incomplete");
        }
        if candidate.metadata_ambiguous {
            return Some("metadata_ambiguous");
        }
        if candidate.protected {
            return Some("candidate_marked_protected");
        }
        if self.is_protected_image(candidate) {
            return Some("protected_image_allowlist");
        }
        if self.is_protected_volume(candidate) {
            return Some("protected_volume_allowlist");
        }
        if self.matches_protected_label(candidate) {
            return Some("protected_label_allowlist");
        }
        match candidate.in_use {
            Some(true) => return Some("candidate_in_use"),
            None => return Some("candidate_in_use_unknown"),
            Some(false) => {}
        }
        match candidate.referenced {
            Some(true) => return Some("candidate_referenced"),
            None => return Some("candidate_referenced_unknown"),
            Some(false) => {}
        }

        let age_days = match candidate.age_days {
            Some(age_days) => age_days,
            None => return Some("candidate_age_unknown"),
        };

        if age_days < self.config.min_unused_age_days {
            return Some("candidate_too_new");
        }

        None
    }

    fn is_protected_image(&self, candidate: &CandidateArtifact) -> bool {
        if !matches!(candidate.resource_kind, ResourceKind::Image) {
            return false;
        }

        value_matches_allowlist(&candidate.identifier, &self.config.protected_images)
            || candidate
                .display_name
                .as_deref()
                .is_some_and(|name| value_matches_allowlist(name, &self.config.protected_images))
    }

    fn is_protected_volume(&self, candidate: &CandidateArtifact) -> bool {
        if !matches!(candidate.resource_kind, ResourceKind::Volume) {
            return false;
        }

        value_matches_allowlist(&candidate.identifier, &self.config.protected_volumes)
            || candidate
                .display_name
                .as_deref()
                .is_some_and(|name| value_matches_allowlist(name, &self.config.protected_volumes))
    }

    fn matches_protected_label(&self, candidate: &CandidateArtifact) -> bool {
        candidate
            .labels
            .iter()
            .any(|label| value_matches_allowlist(label, &self.config.protected_labels))
    }
}

fn value_matches_allowlist(value: &str, allowlist: &[String]) -> bool {
    let value = value.trim();
    !value.is_empty()
        && allowlist.iter().any(|configured| {
            let configured = configured.trim();
            !configured.is_empty() && configured == value
        })
}

#[cfg(test)]
mod tests {
    use super::value_matches_allowlist;

    #[test]
    fn allowlist_matching_trims_entries() {
        assert!(value_matches_allowlist("keep", &[" keep ".to_string()]));
    }

    #[test]
    fn allowlist_matching_ignores_empty_entries() {
        assert!(!value_matches_allowlist("keep", &[" ".to_string()]));
    }
}
