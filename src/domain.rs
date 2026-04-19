use std::collections::BTreeSet;
use std::time::SystemTime;

/// Core data model for the cleanup daemon.
///
/// This module is intentionally backend-agnostic: Docker/Podman-specific code
/// should convert their data into these types and then follow the same flow.
/// Config-facing defaults intentionally mirror install-time behavior so callers
/// do not silently diverge from runtime defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupConfig {
    /// Scheduler interval between daemon ticks.
    pub interval_secs: u64,
    /// Trigger cleanup when usage is at or above this percent.
    pub high_watermark_percent: u8,
    /// Stop cleanup once usage falls below this percent.
    pub target_watermark_percent: u8,
    /// Minimum age before an artifact is eligible for deletion.
    pub min_unused_age_days: u64,
    /// Upper bound on deletion size per run.
    pub max_delete_per_run_gb: u64,
    /// Execution mode guard: when true, perform simulation-only execution.
    pub dry_run: bool,
    /// Image IDs or names that must never be deleted.
    pub protected_images: Vec<String>,
    /// Volume IDs or names that must never be deleted.
    pub protected_volumes: Vec<String>,
    /// Labels that mark resources as protected.
    pub protected_labels: Vec<String>,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            high_watermark_percent: 80,
            target_watermark_percent: 70,
            min_unused_age_days: 7,
            max_delete_per_run_gb: 10,
            dry_run: false,
            protected_images: Vec::new(),
            protected_volumes: Vec::new(),
            protected_labels: Vec::new(),
        }
    }
}

/// Alias kept for callers that prefer a daemon-centric name.
pub type DaemonConfig = CleanupConfig;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// Docker runtime backend.
    Docker,
    /// Podman runtime backend.
    Podman,
    /// Extension point for additional backends.
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Container,
    Image,
    Volume,
    BuildCache,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    DryRun,
    RealRun,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::DryRun
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Backend the report belongs to.
    pub backend: BackendKind,
    /// Whether health check passed.
    pub healthy: bool,
    /// Optional detail when unhealthy.
    pub message: Option<String>,
    /// Timestamp captured by backend adapter.
    pub checked_at: Option<SystemTime>,
}

impl HealthReport {
    pub fn healthy(backend: BackendKind) -> Self {
        Self {
            backend,
            healthy: true,
            message: None,
            checked_at: Some(SystemTime::now()),
        }
    }

    pub fn unhealthy(backend: BackendKind, message: impl Into<String>) -> Self {
        Self {
            backend,
            healthy: false,
            message: Some(message.into()),
            checked_at: Some(SystemTime::now()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshot {
    /// Backend that produced this metric.
    pub backend: BackendKind,
    /// Observed used bytes.
    pub used_bytes: u64,
    /// Optional total capacity (missing means backend could not determine it).
    pub total_bytes: Option<u64>,
    /// Optional direct percent reported by backend.
    pub used_percent: Option<u8>,
    /// Snapshot timestamp.
    pub observed_at: Option<SystemTime>,
}

impl UsageSnapshot {
    pub fn percent_used(&self) -> Option<u8> {
        if let Some(used_percent) = self.used_percent {
            Some(used_percent)
        } else {
            self.total_bytes
                .filter(|total_bytes| *total_bytes > 0)
                .map(|total_bytes| ((self.used_bytes.saturating_mul(100)) / total_bytes) as u8)
        }
    }

    /// Any missing usage signal is treated as unknown so later policy layers can fail closed.
    pub fn is_above_watermark(&self, high_watermark_percent: u8) -> Option<bool> {
        self.percent_used()
            .map(|used_percent| used_percent >= high_watermark_percent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateArtifact {
    /// Source backend.
    pub backend: BackendKind,
    /// Resource type (container/image/volume).
    pub resource_kind: ResourceKind,
    /// Stable backend identifier.
    pub identifier: String,
    /// Optional human-readable name.
    pub display_name: Option<String>,
    /// Labels associated with this artifact.
    pub labels: BTreeSet<String>,
    /// Potential reclaimed size in bytes, if known.
    pub size_bytes: Option<u64>,
    /// Artifact age in days, if known.
    pub age_days: Option<u64>,
    /// Whether currently in use. `None` means unknown.
    pub in_use: Option<bool>,
    /// Whether referenced by another resource. `None` means unknown.
    pub referenced: Option<bool>,
    /// Already matched by explicit protection/allowlist checks.
    pub protected: bool,
    /// True only when required metadata fields are present.
    pub metadata_complete: bool,
    /// Set when backend metadata is conflicting or uncertain.
    pub metadata_ambiguous: bool,
    /// Discovery timestamp.
    pub discovered_at: Option<SystemTime>,
}

impl CandidateArtifact {
    /// Fail closed: any unknown or ambiguous signal keeps the candidate out of the deletion path.
    pub fn is_actionable(&self) -> bool {
        self.metadata_complete
            && !self.metadata_ambiguous
            && !self.protected
            && self.in_use == Some(false)
            && self.referenced == Some(false)
            && self.age_days.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedCandidate {
    /// Candidate that was rejected.
    pub candidate: CandidateArtifact,
    /// Human-readable rejection reason for logs/reports.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupActionKind {
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAction {
    /// Candidate targeted by this action.
    pub candidate: CandidateArtifact,
    /// Action kind (currently only delete in phase 1).
    pub kind: CleanupActionKind,
    /// Whether execution should be dry-run for this specific action.
    pub dry_run: bool,
    /// Optional planner note for auditability.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlan {
    /// Backend this plan is for.
    pub backend: BackendKind,
    /// Plan-wide execution mode.
    pub dry_run: bool,
    /// Actions approved by policy/planner.
    pub actions: Vec<PlannedAction>,
    /// Candidates rejected with reasons.
    pub skipped: Vec<SkippedCandidate>,
}

impl ActionPlan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDiscoveryRequest {
    /// Backend being asked to discover candidates.
    pub backend: BackendKind,
    /// Run configuration at discovery time.
    pub config: CleanupConfig,
    /// Latest usage snapshot to support contextual discovery.
    pub usage: UsageSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlanningRequest {
    /// Backend the planner operates on.
    pub backend: BackendKind,
    /// Run configuration used for policy thresholds.
    pub config: CleanupConfig,
    /// Current usage snapshot used for target calculations.
    pub usage: UsageSnapshot,
    /// Candidate set emitted by discovery stage.
    pub candidates: Vec<CandidateArtifact>,
}

/// Backwards-compatible alias for callers that prefer the shorter request name.
pub type ActionPlannerRequest = ActionPlanningRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    /// Backend that should execute the action.
    pub backend: BackendKind,
    /// Concrete action to execute.
    pub action: PlannedAction,
    /// Dry-run vs real-run mode.
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDiscoveryResponse {
    /// Backend that produced this response.
    pub backend: BackendKind,
    /// Raw candidate list before planner filtering.
    pub candidates: Vec<CandidateArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlannerResponse {
    /// Planned action bundle.
    pub plan: ActionPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResponse {
    /// Backend that attempted execution.
    pub backend: BackendKind,
    /// Candidate associated with the action.
    pub candidate: CandidateArtifact,
    /// Whether the delete command actually ran.
    pub executed: bool,
    /// Whether request ran in dry mode.
    pub dry_run: bool,
    /// Optional backend message (success detail or error context).
    pub message: Option<String>,
}
