use crate::backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector};
use crate::domain::BackendKind;
use crate::error::{CleanupError, Result};
use crate::scheduler::{CleanupScheduler, SchedulerRunReport, SchedulerStopReason};
use std::fs::{remove_file, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Configures retry and backoff behavior for reliability orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum attempts per backend run, including the initial attempt.
    pub max_attempts: usize,
    /// Initial backoff delay after first failure.
    pub initial_backoff: Duration,
    /// Exponential growth factor for backoff.
    pub backoff_multiplier: u32,
    /// Upper bound for backoff delay.
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            backoff_multiplier: 2,
            max_backoff: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    fn normalized_max_attempts(&self) -> usize {
        self.max_attempts.max(1)
    }

    fn backoff_for_retry(&self, retry_index: usize) -> Duration {
        let multiplier = self
            .backoff_multiplier
            .max(1)
            .saturating_pow(retry_index as u32);
        let candidate = self.initial_backoff.saturating_mul(multiplier);
        candidate.min(self.max_backoff)
    }
}

/// Sleep abstraction for deterministic retry tests.
pub trait RetrySleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

/// Production sleeper using `std::thread::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadSleeper;

impl RetrySleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// Marker trait for lock guards that hold single-instance ownership.
pub trait InstanceGuard: Send {}

/// Lock provider used to enforce single-instance execution.
pub trait SingleInstanceLock: Send + Sync {
    fn try_acquire(&self) -> Result<Option<Box<dyn InstanceGuard>>>;
}

/// No-op lock provider for in-process tests or callers that do not require file locking.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopInstanceLock;

#[derive(Debug, Clone, Copy, Default)]
struct NoopGuard;
impl InstanceGuard for NoopGuard {}

impl SingleInstanceLock for NoopInstanceLock {
    fn try_acquire(&self) -> Result<Option<Box<dyn InstanceGuard>>> {
        Ok(Some(Box::new(NoopGuard)))
    }
}

/// File-based single-instance lock provider.
///
/// Safety behavior:
/// - lock acquisition failure due to existing lock returns `Ok(None)` so caller can no-op safely
/// - I/O failures return an explicit error so caller can fail closed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInstanceLock {
    path: PathBuf,
}

impl FileInstanceLock {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[derive(Debug)]
struct FileInstanceGuard {
    path: PathBuf,
    _file: File,
}

impl InstanceGuard for FileInstanceGuard {}

impl Drop for FileInstanceGuard {
    fn drop(&mut self) {
        let _ = remove_file(&self.path);
    }
}

impl SingleInstanceLock for FileInstanceLock {
    fn try_acquire(&self) -> Result<Option<Box<dyn InstanceGuard>>> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                let _ = file.write_all(b"prune-guard-single-instance-lock");
                Ok(Some(Box::new(FileInstanceGuard {
                    path: self.path.clone(),
                    _file: file,
                })))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(CleanupError::ExecutionFailed {
                backend: BackendKind::Custom("runtime".to_string()),
                message: format!("failed to acquire single-instance lock: {error}"),
            }),
        }
    }
}

/// Trait object boundary for one backend scheduler tick.
pub trait BackendCycleRunner: Send + Sync {
    fn backend_kind(&self) -> BackendKind;
    fn run_cycle(&self) -> Result<SchedulerRunReport>;
}

/// Adapter that binds `CleanupScheduler` to one concrete backend.
pub struct SchedulerBackendRunner<B>
where
    B: HealthCheck
        + UsageCollector
        + CandidateDiscoverer
        + ExecutionContract
        + Send
        + Sync
        + 'static,
{
    scheduler: CleanupScheduler,
    backend: Arc<B>,
}

impl<B> SchedulerBackendRunner<B>
where
    B: HealthCheck
        + UsageCollector
        + CandidateDiscoverer
        + ExecutionContract
        + Send
        + Sync
        + 'static,
{
    pub fn new(scheduler: CleanupScheduler, backend: Arc<B>) -> Self {
        Self { scheduler, backend }
    }
}

impl<B> BackendCycleRunner for SchedulerBackendRunner<B>
where
    B: HealthCheck
        + UsageCollector
        + CandidateDiscoverer
        + ExecutionContract
        + Send
        + Sync
        + 'static,
{
    fn backend_kind(&self) -> BackendKind {
        self.backend.backend_kind()
    }

    fn run_cycle(&self) -> Result<SchedulerRunReport> {
        self.scheduler.run_once(Arc::clone(&self.backend))
    }
}

/// Final status for one backend after retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendRunStatus {
    Success,
    FailedAfterRetries,
}

/// Reliability result for one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendReliabilityReport {
    pub backend: BackendKind,
    pub attempts: usize,
    pub status: BackendRunStatus,
    pub final_report: Option<SchedulerRunReport>,
    pub last_error: Option<CleanupError>,
}

impl BackendReliabilityReport {
    fn success(backend: BackendKind, attempts: usize, report: SchedulerRunReport) -> Self {
        Self {
            backend,
            attempts,
            status: BackendRunStatus::Success,
            final_report: Some(report),
            last_error: None,
        }
    }

    fn failed(
        backend: BackendKind,
        attempts: usize,
        report: Option<SchedulerRunReport>,
        last_error: Option<CleanupError>,
    ) -> Self {
        Self {
            backend,
            attempts,
            status: BackendRunStatus::FailedAfterRetries,
            final_report: report,
            last_error,
        }
    }
}

/// Multi-backend reliability run summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReliabilityRunSummary {
    pub lock_acquired: bool,
    pub backend_reports: Vec<BackendReliabilityReport>,
    pub all_backends_failed: bool,
    pub no_op: bool,
}

impl ReliabilityRunSummary {
    fn lock_not_acquired() -> Self {
        Self {
            lock_acquired: false,
            backend_reports: Vec::new(),
            all_backends_failed: true,
            no_op: true,
        }
    }
}

/// Reliability coordinator for Phase 7.
///
/// Safety behavior:
/// - retries transient and execution failure paths with bounded exponential backoff
/// - continues to next backend when one backend exhausts retries
/// - reports all-backends-fail as no-op summary rather than panicking/crashing
/// - enforces single-instance lock before running any backend
pub struct ReliabilityCoordinator<S: RetrySleeper, L: SingleInstanceLock> {
    retry_policy: RetryPolicy,
    sleeper: S,
    lock: L,
}

impl<S: RetrySleeper, L: SingleInstanceLock> ReliabilityCoordinator<S, L> {
    pub fn new(retry_policy: RetryPolicy, sleeper: S, lock: L) -> Self {
        Self {
            retry_policy,
            sleeper,
            lock,
        }
    }

    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    pub fn run_once(
        &self,
        backends: &[Arc<dyn BackendCycleRunner>],
    ) -> Result<ReliabilityRunSummary> {
        let Some(_guard) = self.lock.try_acquire()? else {
            return Ok(ReliabilityRunSummary::lock_not_acquired());
        };

        let mut backend_reports = Vec::with_capacity(backends.len());
        for backend in backends {
            backend_reports.push(self.run_backend_with_retry(backend.as_ref()));
        }

        let all_backends_failed = backend_reports
            .iter()
            .all(|entry| entry.status == BackendRunStatus::FailedAfterRetries);

        let no_op = if backend_reports.is_empty() || all_backends_failed {
            true
        } else {
            backend_reports.iter().all(|entry| {
                entry
                    .final_report
                    .as_ref()
                    .map(|report| !report.cleanup_started)
                    .unwrap_or(true)
            })
        };

        Ok(ReliabilityRunSummary {
            lock_acquired: true,
            backend_reports,
            all_backends_failed,
            no_op,
        })
    }

    fn run_backend_with_retry(&self, backend: &dyn BackendCycleRunner) -> BackendReliabilityReport {
        let max_attempts = self.retry_policy.normalized_max_attempts();
        let backend_kind = backend.backend_kind();

        let mut attempts = 0usize;
        let mut last_error: Option<CleanupError> = None;
        let mut last_report: Option<SchedulerRunReport> = None;

        while attempts < max_attempts {
            attempts += 1;

            match backend.run_cycle() {
                Ok(report) if should_retry_from_report(&report) => {
                    last_error = report.last_error.clone();
                    last_report = Some(report);
                }
                Ok(report) => {
                    return BackendReliabilityReport::success(backend_kind, attempts, report)
                }
                Err(error) => {
                    last_error = Some(error);
                    last_report = None;
                }
            }

            if attempts < max_attempts {
                let retry_index = attempts - 1;
                self.sleeper
                    .sleep(self.retry_policy.backoff_for_retry(retry_index));
            }
        }

        BackendReliabilityReport::failed(backend_kind, attempts, last_report, last_error)
    }
}

fn should_retry_from_report(report: &SchedulerRunReport) -> bool {
    if report.action_failures > 0 {
        return true;
    }

    matches!(
        report.stop_reason,
        SchedulerStopReason::ExecutionFailuresDetected
            | SchedulerStopReason::HealthCheckFailed
            | SchedulerStopReason::UsageCollectionFailed
            | SchedulerStopReason::CandidateDiscoveryFailed
    )
}
