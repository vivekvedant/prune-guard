use prune_guard::observability::{
    emit_scheduler_metrics, evaluate_least_privilege, parse_supported_os, preflight_execution,
    redact_value, validate_supported_os, AuditableRunSummary, InMemoryMetricsRecorder, LogLevel,
    StructuredLogRecord, LOG_SCHEMA_VERSION,
};
use prune_guard::{BackendKind, CleanupError, SchedulerRunReport, SchedulerStopReason, UsageSnapshot};

#[test]
fn structured_log_record_exposes_required_schema_fields() {
    let record = StructuredLogRecord::new(LogLevel::Info, "candidate_skipped", "candidate is protected")
        .with_backend(BackendKind::Docker)
        .with_detail("candidate_id", "sha256:abc")
        .with_detail("auth_token", "secret-token");

    let map = record.to_kv_map();
    assert_eq!(map.get("schema_version"), Some(&LOG_SCHEMA_VERSION.to_string()));
    assert_eq!(map.get("event_type"), Some(&"candidate_skipped".to_string()));
    assert_eq!(map.get("reason"), Some(&"candidate is protected".to_string()));
    assert_eq!(map.get("backend"), Some(&"docker".to_string()));
    assert_eq!(map.get("candidate_id"), Some(&"sha256:abc".to_string()));
    assert_eq!(map.get("auth_token"), Some(&"[REDACTED]".to_string()));
}

#[test]
fn redaction_masks_sensitive_fields_and_tokens() {
    assert_eq!(redact_value("token", "abc"), "[REDACTED]");
    assert_eq!(redact_value("password", "abc"), "[REDACTED]");
    assert_eq!(
        redact_value("http_header", "Bearer my-secret"),
        "Bearer [REDACTED]"
    );
    assert_eq!(redact_value("authorization", "Bearer my-secret"), "[REDACTED]");
    assert_eq!(redact_value("plain_field", "visible"), "visible");
}

#[test]
fn run_summary_contains_auditable_reason_and_counts() {
    let report = SchedulerRunReport {
        backend: BackendKind::Docker,
        dry_run: true,
        cleanup_started: false,
        iterations: 1,
        actions_planned: 0,
        actions_completed: 0,
        action_failures: 0,
        skipped_candidates: 3,
        initial_usage: Some(sample_usage()),
        final_usage: Some(sample_usage()),
        stop_reason: SchedulerStopReason::NoActionableCandidates,
        last_error: Some(CleanupError::SafetyViolation {
            message: "protected resource".to_string(),
        }),
    };

    let summary = AuditableRunSummary::from_report(&report);
    assert_eq!(summary.backend, BackendKind::Docker);
    assert_eq!(summary.actions_planned, 0);
    assert_eq!(summary.skipped_candidates, 3);
    assert!(
        summary.auditable_reason.contains("NoActionableCandidates"),
        "summary must keep stop reason for auditability"
    );
}

#[test]
fn optional_metrics_recorder_captures_run_metrics() {
    let recorder = InMemoryMetricsRecorder::default();
    let report = SchedulerRunReport {
        backend: BackendKind::Podman,
        dry_run: false,
        cleanup_started: true,
        iterations: 2,
        actions_planned: 4,
        actions_completed: 3,
        action_failures: 1,
        skipped_candidates: 0,
        initial_usage: Some(sample_usage()),
        final_usage: Some(sample_usage()),
        stop_reason: SchedulerStopReason::ExecutionFailuresDetected,
        last_error: None,
    };

    emit_scheduler_metrics(&recorder, &report);
    let counters = recorder.counters();

    assert_eq!(counters.get("scheduler_runs_total"), Some(&1));
    assert_eq!(counters.get("scheduler_actions_planned_total"), Some(&4));
    assert_eq!(counters.get("scheduler_actions_completed_total"), Some(&3));
    assert_eq!(counters.get("scheduler_action_failures_total"), Some(&1));
}

#[test]
fn least_privilege_check_fails_closed_for_root_or_unknown_uid() {
    let root = evaluate_least_privilege(Some(0));
    assert!(!root.least_privilege_ok);
    assert!(root.reason.contains("root"));

    let unknown = evaluate_least_privilege(None);
    assert!(!unknown.least_privilege_ok);
    assert!(unknown.reason.contains("unknown"));
}

#[test]
fn portability_matrix_accepts_linux_macos_windows() {
    assert_eq!(parse_supported_os("linux").expect("linux supported").as_str(), "linux");
    assert_eq!(parse_supported_os("macos").expect("macos supported").as_str(), "macos");
    assert_eq!(
        parse_supported_os("windows")
            .expect("windows supported")
            .as_str(),
        "windows"
    );
}

#[test]
fn preflight_enforces_fail_closed_when_os_or_privilege_is_unsafe() {
    let unsupported_os = preflight_execution(false, "solaris", Some(1000));
    assert!(unsupported_os.enforce_dry_run);
    assert!(unsupported_os.reasons.iter().any(|reason| reason.contains("unsupported os")));

    let elevated = preflight_execution(false, "linux", Some(0));
    assert!(elevated.enforce_dry_run);
    assert!(
        elevated
            .reasons
            .iter()
            .any(|reason| reason.contains("least-privilege"))
    );

    let safe = preflight_execution(false, "linux", Some(1000));
    assert!(!safe.enforce_dry_run);
}

#[test]
fn supported_os_validation_returns_reasonable_report() {
    let linux = validate_supported_os("linux");
    assert!(linux.supported);
    assert_eq!(linux.normalized_os, Some("linux".to_string()));

    let unknown = validate_supported_os("aix");
    assert!(!unknown.supported);
    assert!(unknown.reason.contains("unsupported"));
}

fn sample_usage() -> UsageSnapshot {
    UsageSnapshot {
        backend: BackendKind::Docker,
        used_bytes: 900,
        total_bytes: Some(1000),
        used_percent: Some(90),
        observed_at: None,
    }
}
