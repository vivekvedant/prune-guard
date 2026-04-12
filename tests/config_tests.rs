use prune_guard::Config;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn default_config_is_safety_first() {
    let cfg = Config::default();

    assert!(cfg.dry_run, "dry_run must default to true");
    assert_eq!(cfg.interval_secs, 900);
    assert_eq!(cfg.high_watermark_percent, 85);
    assert_eq!(cfg.target_watermark_percent, 70);
    assert_eq!(cfg.min_unused_age_days, 30);
    assert_eq!(cfg.max_delete_per_run_gb, 10);
    assert_eq!(cfg.enabled_backends, vec!["docker".to_string()]);
    assert!(cfg.protected_images.is_empty());
    assert!(cfg.protected_volumes.is_empty());
    assert!(cfg.protected_labels.is_empty());
}

#[test]
fn parse_str_applies_explicit_overrides() {
    let cfg = Config::parse_str(
        r#"
            interval_secs = 1200
            high_watermark_percent = 80
            target_watermark_percent = 65
            min_unused_age_days = 14
            max_delete_per_run_gb = 4
            dry_run = false
            enabled_backends = ["docker", "podman"]
            protected_images = ["alpine:latest", "busybox:latest"]
            protected_volumes = ["pgdata"]
            protected_labels = ["keep=true", "owner=ops"]
        "#,
    )
    .expect("configuration should parse");

    assert_eq!(cfg.interval_secs, 1200);
    assert_eq!(cfg.high_watermark_percent, 80);
    assert_eq!(cfg.target_watermark_percent, 65);
    assert_eq!(cfg.min_unused_age_days, 14);
    assert_eq!(cfg.max_delete_per_run_gb, 4);
    assert!(!cfg.dry_run);
    assert_eq!(cfg.enabled_backends, vec!["docker", "podman"]);
    assert_eq!(
        cfg.protected_images,
        vec!["alpine:latest", "busybox:latest"]
    );
    assert_eq!(cfg.protected_volumes, vec!["pgdata"]);
    assert_eq!(cfg.protected_labels, vec!["keep=true", "owner=ops"]);
}

#[test]
fn from_reader_supports_sectioned_toml() {
    let cfg = Config::from_reader(Cursor::new(
        r#"
            [runtime]
            interval_secs = 1800

            [thresholds]
            high_watermark_percent = 90
            target_watermark_percent = 75

            [safety]
            dry_run = false
            protected_images = ["postgres:16"]
        "#,
    ))
    .expect("sectioned TOML should parse");

    assert_eq!(cfg.interval_secs, 1800);
    assert_eq!(cfg.high_watermark_percent, 90);
    assert_eq!(cfg.target_watermark_percent, 75);
    assert!(!cfg.dry_run);
    assert_eq!(cfg.protected_images, vec!["postgres:16"]);
}

#[test]
fn load_from_path_reads_toml_file() {
    let path = unique_temp_path("prune_guard_config_test.toml");
    fs::write(
        &path,
        r#"
            interval_secs = 600
            high_watermark_percent = 88
            target_watermark_percent = 72
            dry_run = true
        "#,
    )
    .expect("test config should be writable");

    let cfg = Config::load_from_path(&path).expect("file should load");

    assert_eq!(cfg.interval_secs, 600);
    assert_eq!(cfg.high_watermark_percent, 88);
    assert_eq!(cfg.target_watermark_percent, 72);
    assert!(cfg.dry_run);

    let _ = fs::remove_file(&path);
}

#[test]
fn invalid_threshold_relationship_is_rejected() {
    let err = Config::parse_str(
        r#"
            interval_secs = 600
            high_watermark_percent = 70
            target_watermark_percent = 80
        "#,
    )
    .expect_err("target above high watermark should fail closed");

    let message = err.to_string();
    assert!(
        message.contains("target_watermark_percent must be lower than high_watermark_percent"),
        "unexpected error message: {message}"
    );
}

fn unique_temp_path(file_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    path.push(format!("{}_{}_{}", std::process::id(), nanos, file_name));
    path
}
