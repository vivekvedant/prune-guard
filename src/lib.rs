#![forbid(unsafe_code)]
//! `prune_guard` crate entry-point.
//!
//! This file intentionally stays small:
//! - declares the top-level modules
//! - re-exports the public API so callers can import from `prune_guard::*`
//!
//! If you are new to Rust, a `pub mod x;` line means "compile `src/x.rs` as a
//! module", and a `pub use ...;` line means "re-export this item at crate root".

/// Backend contracts: traits that Docker/Podman adapters must implement.
pub mod backend;
/// Configuration model and TOML parsing helpers.
pub mod config;
/// Shared domain models used across scheduler/policy/planner/executor phases.
pub mod domain;
/// Docker backend adapter implementation (Phase 5).
pub mod docker_backend;
/// Podman backend adapter implementation (Phase 6).
pub mod podman_backend;
/// Shared error type for all crate operations.
pub mod error;
/// Batch executor with dry-run and timeout guards.
pub mod executor;
/// Deterministic action planner with per-run delete-cap enforcement.
pub mod planner;
/// Fail-closed policy engine for candidate selection.
pub mod policy;
/// Scheduler/watermark loop orchestration for periodic daemon ticks.
pub mod scheduler;

pub use backend::{
    ActionPlanner, ActionPlannerContract, CandidateDiscoverer, CandidateDiscovererContract,
    CleanupBackend, ExecutionContract, ExecutionExecutor, HealthCheck, HealthCheckContract,
    UsageCollector, UsageCollectorContract,
};
pub use config::{Config, ConfigError};
pub use domain::{
    ActionPlan, ActionPlannerRequest, ActionPlannerResponse, ActionPlanningRequest, BackendKind,
    CandidateArtifact, CandidateDiscoveryRequest, CandidateDiscoveryResponse, CleanupActionKind,
    CleanupConfig, DaemonConfig, ExecutionMode, ExecutionRequest, ExecutionResponse, HealthReport,
    PlannedAction, ResourceKind, SkippedCandidate, UsageSnapshot,
};
pub use docker_backend::{CommandRunner, DockerBackend, OsCommandRunner};
pub use error::{CleanupError, Result};
pub use executor::{ActionExecutionFailure, CleanupExecutor, ExecutionReport};
pub use planner::CleanupPlanner;
pub use policy::{PolicyEngine, PolicyEvaluation};
pub use podman_backend::PodmanBackend;
pub use scheduler::{CleanupScheduler, SchedulerRunReport, SchedulerStopReason};
