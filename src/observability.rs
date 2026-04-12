use crate::domain::{BackendKind, CleanupActionKind};
use crate::scheduler::SchedulerRunReport;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Stable schema version for structured log records.
pub const LOG_SCHEMA_VERSION: &str = "1.0";
const CORE_LOG_FIELDS: [&str; 6] = ["schema_version", "level", "event_type", "reason", "backend", "action"];

/// Log severity used by structured log records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Structured log envelope for auditable cleanup decisions.
///
/// Safety intent:
/// - always carries a machine-readable schema version
/// - always includes an explicit decision reason
/// - redacts sensitive fields by default
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredLogRecord {
    level: LogLevel,
    event_type: String,
    reason: String,
    backend: Option<BackendKind>,
    action: Option<CleanupActionKind>,
    details: BTreeMap<String, String>,
}

impl StructuredLogRecord {
    pub fn new(level: LogLevel, event_type: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            level,
            event_type: event_type.into(),
            reason: reason.into(),
            backend: None,
            action: None,
            details: BTreeMap::new(),
        }
    }

    pub fn with_backend(mut self, backend: BackendKind) -> Self {
        self.backend = Some(backend);
        self
    }

    pub fn with_action(mut self, action: CleanupActionKind) -> Self {
        self.action = Some(action);
        self
    }

    /// Adds one detail field after applying redaction rules.
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = value.into();
        self.details.insert(key.clone(), redact_value(&key, &value));
        self
    }

    /// Flattens the record into a stable key-value map for schema validation/tests.
    pub fn to_kv_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("schema_version".to_string(), LOG_SCHEMA_VERSION.to_string());
        map.insert("level".to_string(), self.level.as_str().to_string());
        map.insert("event_type".to_string(), self.event_type.clone());
        map.insert("reason".to_string(), self.reason.clone());
        if let Some(backend) = &self.backend {
            map.insert("backend".to_string(), backend_to_str(backend).to_string());
        }
        if let Some(action) = self.action {
            map.insert("action".to_string(), action_to_str(action).to_string());
        }
        for (key, value) in &self.details {
            // Never allow detail payloads to overwrite core envelope fields.
            if CORE_LOG_FIELDS.contains(&key.as_str()) {
                continue;
            }
            map.insert(key.clone(), value.clone());
        }
        map
    }

    /// Renders one JSON line without adding external dependencies.
    pub fn to_json_line(&self) -> String {
        let map = self.to_kv_map();
        let mut parts = Vec::with_capacity(map.len());
        for (key, value) in map {
            parts.push(format!("\"{}\":\"{}\"", escape_json(&key), escape_json(&value)));
        }
        format!("{{{}}}", parts.join(","))
    }
}

/// Redacts potentially sensitive values for logs.
///
/// Fail-closed rule:
/// - unknown sensitive keys are treated as secrets and hidden
/// - bearer tokens are redacted even when key names are generic
pub fn redact_value(key: &str, value: &str) -> String {
    let lowered = key.to_ascii_lowercase();
    if is_sensitive_key(&lowered) {
        return "[REDACTED]".to_string();
    }

    let trimmed = value.trim_start();
    if let Some((scheme, credential)) = trimmed.split_once(char::is_whitespace) {
        let credential = credential.trim_start();
        if scheme.eq_ignore_ascii_case("bearer") && !credential.is_empty() {
            return format!("{scheme} [REDACTED]");
        }
    }

    value.to_string()
}

/// Auditable summary that normalizes scheduler output for operational logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditableRunSummary {
    pub backend: BackendKind,
    pub dry_run: bool,
    pub cleanup_started: bool,
    pub iterations: usize,
    pub actions_planned: usize,
    pub actions_completed: usize,
    pub action_failures: usize,
    pub skipped_candidates: usize,
    pub auditable_reason: String,
}

impl AuditableRunSummary {
    pub fn from_report(report: &SchedulerRunReport) -> Self {
        let error_detail = report
            .last_error
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "none".to_string());

        Self {
            backend: report.backend.clone(),
            dry_run: report.dry_run,
            cleanup_started: report.cleanup_started,
            iterations: report.iterations,
            actions_planned: report.actions_planned,
            actions_completed: report.actions_completed,
            action_failures: report.action_failures,
            skipped_candidates: report.skipped_candidates,
            auditable_reason: format!(
                "stop_reason={:?};last_error={}",
                report.stop_reason, error_detail
            ),
        }
    }
}

/// Optional metrics interface used by Phase 8 observability hooks.
pub trait MetricsRecorder: Send + Sync {
    fn increment_counter(&self, metric: &str, value: u64);
}

/// No-op metrics recorder for deployments that disable metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMetricsRecorder;

impl MetricsRecorder for NoopMetricsRecorder {
    fn increment_counter(&self, _metric: &str, _value: u64) {}
}

/// In-memory metrics recorder used by unit/integration tests.
#[derive(Debug, Default)]
pub struct InMemoryMetricsRecorder {
    counters: Mutex<BTreeMap<String, u64>>,
}

impl InMemoryMetricsRecorder {
    pub fn counters(&self) -> BTreeMap<String, u64> {
        let guard = match self.counters.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.clone()
    }
}

impl MetricsRecorder for InMemoryMetricsRecorder {
    fn increment_counter(&self, metric: &str, value: u64) {
        let mut counters = match self.counters.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = counters.entry(metric.to_string()).or_insert(0);
        *entry = entry.saturating_add(value);
    }
}

/// Emits scheduler-level counters for per-run summaries.
pub fn emit_scheduler_metrics<R: MetricsRecorder>(recorder: &R, report: &SchedulerRunReport) {
    recorder.increment_counter("scheduler_runs_total", 1);
    recorder.increment_counter("scheduler_actions_planned_total", report.actions_planned as u64);
    recorder.increment_counter(
        "scheduler_actions_completed_total",
        report.actions_completed as u64,
    );
    recorder.increment_counter(
        "scheduler_action_failures_total",
        report.action_failures as u64,
    );
}

/// Least-privilege check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeastPrivilegeReport {
    pub least_privilege_ok: bool,
    pub reason: String,
}

/// Evaluates whether execution context satisfies least-privilege expectations.
///
/// Fail-closed rule:
/// - unknown uid is treated as unsafe for real-run execution
/// - root/elevated uid is treated as unsafe for real-run execution
pub fn evaluate_least_privilege(effective_uid: Option<u32>) -> LeastPrivilegeReport {
    match effective_uid {
        Some(0) => LeastPrivilegeReport {
            least_privilege_ok: false,
            reason: "running as root violates least-privilege guard".to_string(),
        },
        Some(_) => LeastPrivilegeReport {
            least_privilege_ok: true,
            reason: "least-privilege check passed".to_string(),
        },
        None => LeastPrivilegeReport {
            least_privilege_ok: false,
            reason: "unknown effective uid violates least-privilege guard".to_string(),
        },
    }
}

/// Supported operating systems for runtime portability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedOs {
    Linux,
    MacOs,
    Windows,
}

impl SupportedOs {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Windows => "windows",
        }
    }
}

pub fn parse_supported_os(name: &str) -> Option<SupportedOs> {
    match name.to_ascii_lowercase().as_str() {
        "linux" => Some(SupportedOs::Linux),
        "macos" | "darwin" => Some(SupportedOs::MacOs),
        "windows" => Some(SupportedOs::Windows),
        _ => None,
    }
}

/// Portability validation output for one platform string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortabilityReport {
    pub supported: bool,
    pub normalized_os: Option<String>,
    pub reason: String,
}

pub fn validate_supported_os(name: &str) -> PortabilityReport {
    match parse_supported_os(name) {
        Some(os) => PortabilityReport {
            supported: true,
            normalized_os: Some(os.as_str().to_string()),
            reason: "os supported".to_string(),
        },
        None => PortabilityReport {
            supported: false,
            normalized_os: None,
            reason: format!("unsupported os: {name}"),
        },
    }
}

/// Runtime preflight decision before real deletion execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePreflightDecision {
    pub enforce_dry_run: bool,
    pub reasons: Vec<String>,
    pub portability_supported: bool,
    pub least_privilege_ok: bool,
}

/// Computes fail-closed dry-run enforcement from security and portability checks.
pub fn preflight_execution(
    requested_dry_run: bool,
    os_name: &str,
    effective_uid: Option<u32>,
) -> RuntimePreflightDecision {
    let portability = validate_supported_os(os_name);
    let least_privilege = evaluate_least_privilege(effective_uid);

    let mut reasons = Vec::new();
    if requested_dry_run {
        reasons.push("dry-run requested by configuration".to_string());
    }
    if !portability.supported {
        reasons.push(portability.reason.clone());
    }
    if !least_privilege.least_privilege_ok {
        reasons.push(format!(
            "least-privilege check failed: {}",
            least_privilege.reason
        ));
    }

    RuntimePreflightDecision {
        enforce_dry_run: requested_dry_run || !portability.supported || !least_privilege.least_privilege_ok,
        reasons,
        portability_supported: portability.supported,
        least_privilege_ok: least_privilege.least_privilege_ok,
    }
}

fn backend_to_str(backend: &BackendKind) -> &str {
    match backend {
        BackendKind::Docker => "docker",
        BackendKind::Podman => "podman",
        BackendKind::Custom(_) => "custom",
    }
}

fn action_to_str(action: CleanupActionKind) -> &'static str {
    match action {
        CleanupActionKind::Delete => "delete",
    }
}

fn is_sensitive_key(key: &str) -> bool {
    const SENSITIVE_KEYWORDS: [&str; 6] = [
        "token",
        "password",
        "secret",
        "credential",
        "authorization",
        "auth",
    ];
    SENSITIVE_KEYWORDS.iter().any(|keyword| key.contains(keyword))
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000C}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0000}'..='\u{001F}' => {
                escaped.push_str(&format!("\\u{:04x}", ch as u32));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}
