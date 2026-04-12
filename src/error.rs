use crate::domain::BackendKind;
use std::error::Error;
use std::fmt;

/// Shared result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, CleanupError>;

/// Unified error model for all daemon phases.
///
/// Keeping a single error enum simplifies logging/reporting and keeps
/// backend-specific failures visible with backend context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupError {
    /// User/configuration provided invalid values.
    InvalidConfig { message: String },
    /// Requested backend is not implemented or not enabled.
    UnsupportedBackend {
        backend: BackendKind,
        message: String,
    },
    /// Backend could not be reached.
    BackendUnavailable {
        backend: BackendKind,
        message: String,
    },
    /// Health check stage failed.
    HealthCheckFailed {
        backend: BackendKind,
        message: String,
    },
    /// Usage collection stage failed.
    UsageCollectionFailed {
        backend: BackendKind,
        message: String,
    },
    /// Candidate discovery stage failed.
    CandidateDiscoveryFailed {
        backend: BackendKind,
        message: String,
    },
    /// Action planning stage failed.
    ActionPlanningFailed {
        backend: BackendKind,
        message: String,
    },
    /// Execution stage failed.
    ExecutionFailed {
        backend: BackendKind,
        message: String,
    },
    /// Explicit guard for unsafe conditions.
    SafetyViolation { message: String },
    /// Temporary placeholder for not-yet-implemented pieces.
    NotImplemented { component: &'static str },
}

impl CleanupError {
    pub fn not_implemented(component: &'static str) -> Self {
        Self::NotImplemented { component }
    }
}

impl fmt::Display for CleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig { message } => write!(f, "invalid config: {message}"),
            Self::UnsupportedBackend { backend, message } => {
                write!(f, "unsupported backend {:?}: {message}", backend)
            }
            Self::BackendUnavailable { backend, message } => {
                write!(f, "backend {:?} unavailable: {message}", backend)
            }
            Self::HealthCheckFailed { backend, message } => {
                write!(f, "health check failed for {:?}: {message}", backend)
            }
            Self::UsageCollectionFailed { backend, message } => {
                write!(f, "usage collection failed for {:?}: {message}", backend)
            }
            Self::CandidateDiscoveryFailed { backend, message } => {
                write!(f, "candidate discovery failed for {:?}: {message}", backend)
            }
            Self::ActionPlanningFailed { backend, message } => {
                write!(f, "action planning failed for {:?}: {message}", backend)
            }
            Self::ExecutionFailed { backend, message } => {
                write!(f, "execution failed for {:?}: {message}", backend)
            }
            Self::SafetyViolation { message } => write!(f, "safety violation: {message}"),
            Self::NotImplemented { component } => write!(f, "{component} not implemented"),
        }
    }
}

impl Error for CleanupError {}
