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
/// Docker backend adapter implementation (Phase 5).
pub mod docker_backend;
/// Shared domain models used across scheduler/policy/planner/executor phases.
pub mod domain;
/// Shared error type for all crate operations.
pub mod error;
/// Batch executor with dry-run and timeout guards.
pub mod executor;
/// Phase 8 observability, security, and portability checks.
pub mod observability;
/// Deterministic action planner with per-run delete-cap enforcement.
pub mod planner;
/// Podman backend adapter implementation (Phase 6).
pub mod podman_backend;
/// Fail-closed policy engine for candidate selection.
pub mod policy;
/// Reliability orchestration (retries, lock, and multi-backend continuation).
pub mod reliability;
/// Scheduler/watermark loop orchestration for periodic daemon ticks.
pub mod scheduler;

pub use backend::{
    ActionPlanner, ActionPlannerContract, CandidateDiscoverer, CandidateDiscovererContract,
    CleanupBackend, ExecutionContract, ExecutionExecutor, HealthCheck, HealthCheckContract,
    UsageCollector, UsageCollectorContract,
};
pub use config::{Config, ConfigError};
pub use docker_backend::{CommandRunner, DockerBackend, OsCommandRunner};
pub use domain::{
    ActionPlan, ActionPlannerRequest, ActionPlannerResponse, ActionPlanningRequest, BackendKind,
    CandidateArtifact, CandidateDiscoveryRequest, CandidateDiscoveryResponse, CleanupActionKind,
    CleanupConfig, DaemonConfig, ExecutionMode, ExecutionRequest, ExecutionResponse, HealthReport,
    PlannedAction, ResourceKind, SkippedCandidate, UsageSnapshot,
};
pub use error::{CleanupError, Result};
pub use executor::{ActionExecutionFailure, CleanupExecutor, ExecutionReport};
pub use observability::{
    emit_scheduler_metrics, evaluate_least_privilege, parse_supported_os, preflight_execution,
    redact_value, validate_supported_os, AuditableRunSummary, InMemoryMetricsRecorder,
    LeastPrivilegeReport, LogLevel, MetricsRecorder, NoopMetricsRecorder, PortabilityReport,
    RuntimePreflightDecision, StructuredLogRecord, SupportedOs, LOG_SCHEMA_VERSION,
};
pub use planner::CleanupPlanner;
pub use podman_backend::PodmanBackend;
pub use policy::{PolicyEngine, PolicyEvaluation};
pub use reliability::{
    BackendCycleRunner, BackendReliabilityReport, BackendRunStatus, FileInstanceLock,
    InstanceGuard, NoopInstanceLock, ReliabilityCoordinator, ReliabilityRunSummary, RetryPolicy,
    RetrySleeper, SchedulerBackendRunner, ThreadSleeper,
};
pub use scheduler::{CleanupScheduler, SchedulerRunReport, SchedulerStopReason};
