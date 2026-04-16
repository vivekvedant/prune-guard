use prune_guard::{
    backend::{CandidateDiscoverer, ExecutionContract, HealthCheck, UsageCollector},
    CleanupConfig, CleanupScheduler, Config, DockerBackend, PodmanBackend, SchedulerRunReport,
};
use std::env;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_CONFIG_PATH: &str = "/etc/prune-guard/prune-guard.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliOptions {
    config_path: String,
    backend_override: Option<String>,
    once: bool,
    ticks: Option<usize>,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args())?;
    let config = Config::load_from_path(&options.config_path)
        .map_err(|error| format!("failed to load config `{}`: {error}", options.config_path))?;

    let backend = select_backend_name(&config, options.backend_override.as_deref())?;
    let cleanup_config = to_cleanup_config(&config);
    let scheduler = CleanupScheduler::new(cleanup_config.clone());

    let requested_ticks = if options.once {
        Some(1)
    } else {
        options.ticks
    };

    match backend.as_str() {
        "docker" => run_scheduler_loop(&scheduler, DockerBackend::new(), &cleanup_config, requested_ticks),
        "podman" => run_scheduler_loop(&scheduler, PodmanBackend::new(), &cleanup_config, requested_ticks),
        _ => Err(format!("unsupported backend selected: {backend}")),
    }
}

fn run_scheduler_loop<B>(
    scheduler: &CleanupScheduler,
    backend: B,
    config: &CleanupConfig,
    ticks: Option<usize>,
) -> Result<(), String>
where
    B: HealthCheck + UsageCollector + CandidateDiscoverer + ExecutionContract + Send + Sync + 'static,
{
    let backend = Arc::new(backend);
    let interval = Duration::from_secs(config.interval_secs);

    match ticks {
        Some(total_ticks) => {
            for index in 0..total_ticks {
                let report = scheduler
                    .run_once(Arc::clone(&backend))
                    .map_err(|error| format!("scheduler tick failed: {error}"))?;
                log_report(index + 1, &report);
                if index + 1 < total_ticks {
                    thread::sleep(interval);
                }
            }
            Ok(())
        }
        None => {
            let mut tick_index: usize = 1;
            loop {
                let report = scheduler
                    .run_once(Arc::clone(&backend))
                    .map_err(|error| format!("scheduler tick failed: {error}"))?;
                log_report(tick_index, &report);
                tick_index = tick_index.saturating_add(1);
                thread::sleep(interval);
            }
        }
    }
}

fn log_report(tick_index: usize, report: &SchedulerRunReport) {
    println!("{}", format_report_line(tick_index, report));
    if let Some(error) = &report.last_error {
        println!("tick={} last_error={}", tick_index, error);
    }
}

fn format_report_line(tick_index: usize, report: &SchedulerRunReport) -> String {
    let initial_used_bytes = report.initial_usage.as_ref().map(|usage| usage.used_bytes);
    let final_used_bytes = report.final_usage.as_ref().map(|usage| usage.used_bytes);
    let reclaimed_bytes = match (initial_used_bytes, final_used_bytes) {
        (Some(initial_used_bytes), Some(final_used_bytes)) => {
            Some(initial_used_bytes.saturating_sub(final_used_bytes))
        }
        _ => None,
    };
    let usage_percent_before = report
        .initial_usage
        .as_ref()
        .and_then(|usage| usage.percent_used());
    let usage_percent_after = report
        .final_usage
        .as_ref()
        .and_then(|usage| usage.percent_used());

    format!(
        "tick={} backend={:?} dry_run={} cleanup_started={} stop_reason={:?} actions_planned={} actions_completed={} action_failures={} skipped_candidates={} initial_used_bytes={} final_used_bytes={} reclaimed_bytes={} usage_percent_before={} usage_percent_after={}",
        tick_index,
        report.backend,
        report.dry_run,
        report.cleanup_started,
        report.stop_reason,
        report.actions_planned,
        report.actions_completed,
        report.action_failures,
        report.skipped_candidates,
        format_optional_u64(initial_used_bytes),
        format_optional_u64(final_used_bytes),
        format_optional_u64(reclaimed_bytes),
        format_optional_u8(usage_percent_before),
        format_optional_u8(usage_percent_after)
    )
}

fn format_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_optional_u8(value: Option<u8>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn to_cleanup_config(config: &Config) -> CleanupConfig {
    CleanupConfig {
        interval_secs: config.interval_secs,
        high_watermark_percent: config.high_watermark_percent,
        target_watermark_percent: config.target_watermark_percent,
        min_unused_age_days: config.min_unused_age_days,
        max_delete_per_run_gb: config.max_delete_per_run_gb,
        dry_run: config.dry_run,
        allow_missing_image_labels: config.allow_missing_image_labels,
        protected_images: config.protected_images.clone(),
        protected_volumes: config.protected_volumes.clone(),
        protected_labels: config.protected_labels.clone(),
    }
}

fn select_backend_name(config: &Config, backend_override: Option<&str>) -> Result<String, String> {
    if config.enabled_backends.is_empty() {
        return Err("enabled_backends cannot be empty".to_string());
    }

    if let Some(backend_name) = backend_override {
        let normalized = normalize_backend_name(backend_name)?;
        let enabled = config
            .enabled_backends
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(normalized.as_str()));
        if !enabled {
            return Err(format!(
                "backend override `{normalized}` is not present in enabled_backends"
            ));
        }
        return Ok(normalized);
    }

    for backend_name in &config.enabled_backends {
        if normalize_backend_name(backend_name).is_err() {
            return Err(format!(
                "unsupported backend configured in enabled_backends: {}",
                backend_name
            ));
        }
    }

    normalize_backend_name(config.enabled_backends[0].as_str())
}

fn normalize_backend_name(name: &str) -> Result<String, String> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "docker" | "podman" => Ok(normalized),
        _ => Err(format!("unsupported backend: {name}")),
    }
}

fn parse_args<I>(args: I) -> Result<CliOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut config_path = DEFAULT_CONFIG_PATH.to_string();
    let mut backend_override: Option<String> = None;
    let mut once = false;
    let mut ticks: Option<usize> = None;

    let mut iter = args.into_iter();
    let _program = iter.next();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --config".to_string())?;
                if value.trim().is_empty() {
                    return Err("config path cannot be empty".to_string());
                }
                config_path = value;
            }
            "--backend" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --backend".to_string())?;
                backend_override = Some(value);
            }
            "--once" => {
                once = true;
            }
            "--ticks" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --ticks".to_string())?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --ticks value: {value}"))?;
                if parsed == 0 {
                    return Err("--ticks must be greater than 0".to_string());
                }
                ticks = Some(parsed);
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {arg}"));
            }
        }
    }

    if once && ticks.is_some() {
        return Err("use either --once or --ticks, not both".to_string());
    }

    Ok(CliOptions {
        config_path,
        backend_override,
        once,
        ticks,
    })
}

fn print_usage() {
    println!("Usage: prune-guard [--config PATH] [--backend docker|podman] [--once|--ticks N]");
}

#[cfg(test)]
mod tests {
    use super::{format_report_line, parse_args, select_backend_name, CliOptions, DEFAULT_CONFIG_PATH};
    use prune_guard::{BackendKind, Config, SchedulerRunReport, SchedulerStopReason, UsageSnapshot};

    #[test]
    fn parse_args_uses_safe_defaults() {
        let parsed = parse_args(vec!["prune-guard".to_string()]).expect("defaults should parse");
        assert_eq!(
            parsed,
            CliOptions {
                config_path: DEFAULT_CONFIG_PATH.to_string(),
                backend_override: None,
                once: false,
                ticks: None,
            }
        );
    }

    #[test]
    fn parse_args_rejects_once_with_ticks() {
        let err = parse_args(vec![
            "prune-guard".to_string(),
            "--once".to_string(),
            "--ticks".to_string(),
            "2".to_string(),
        ])
        .expect_err("once and ticks together should fail");
        assert!(err.contains("either --once or --ticks"));
    }

    #[test]
    fn select_backend_uses_first_supported_when_override_missing() {
        let config = Config {
            enabled_backends: vec!["podman".to_string(), "docker".to_string()],
            ..Config::default()
        };
        let backend = select_backend_name(&config, None).expect("backend should resolve");
        assert_eq!(backend, "podman");
    }

    #[test]
    fn select_backend_rejects_override_not_in_enabled_list() {
        let config = Config {
            enabled_backends: vec!["docker".to_string()],
            ..Config::default()
        };
        let err = select_backend_name(&config, Some("podman"))
            .expect_err("override outside enabled list should fail");
        assert!(err.contains("not present in enabled_backends"));
    }

    #[test]
    fn select_backend_fails_closed_on_unknown_enabled_backend() {
        let config = Config {
            enabled_backends: vec!["dockre".to_string()],
            ..Config::default()
        };
        let err = select_backend_name(&config, None)
            .expect_err("unknown backend configuration must fail closed");
        assert!(err.contains("unsupported backend configured"));
    }

    #[test]
    fn report_line_includes_reclaimed_stats_when_usage_is_known() {
        let report = SchedulerRunReport {
            backend: BackendKind::Docker,
            dry_run: false,
            cleanup_started: true,
            iterations: 1,
            actions_planned: 2,
            actions_completed: 2,
            action_failures: 0,
            skipped_candidates: 3,
            initial_usage: Some(usage(90, 100)),
            final_usage: Some(usage(70, 100)),
            stop_reason: SchedulerStopReason::NoActionableCandidates,
            last_error: None,
        };

        let line = format_report_line(1, &report);
        assert!(
            line.contains("initial_used_bytes=90"),
            "line should include initial_used_bytes: {line}"
        );
        assert!(
            line.contains("final_used_bytes=70"),
            "line should include final_used_bytes: {line}"
        );
        assert!(
            line.contains("reclaimed_bytes=20"),
            "line should include reclaimed_bytes: {line}"
        );
        assert!(
            line.contains("usage_percent_before=90"),
            "line should include usage_percent_before: {line}"
        );
        assert!(
            line.contains("usage_percent_after=70"),
            "line should include usage_percent_after: {line}"
        );
    }

    #[test]
    fn report_line_marks_reclaimed_stats_unknown_when_usage_missing() {
        let report = SchedulerRunReport {
            backend: BackendKind::Docker,
            dry_run: false,
            cleanup_started: false,
            iterations: 0,
            actions_planned: 0,
            actions_completed: 0,
            action_failures: 0,
            skipped_candidates: 0,
            initial_usage: None,
            final_usage: None,
            stop_reason: SchedulerStopReason::HealthCheckFailed,
            last_error: None,
        };

        let line = format_report_line(1, &report);
        assert!(
            line.contains("reclaimed_bytes=unknown"),
            "line should include unknown reclaimed stats: {line}"
        );
    }

    fn usage(used: u64, total: u64) -> UsageSnapshot {
        UsageSnapshot {
            backend: BackendKind::Docker,
            used_bytes: used,
            total_bytes: Some(total),
            used_percent: Some(((used.saturating_mul(100)) / total) as u8),
            observed_at: None,
        }
    }
}
