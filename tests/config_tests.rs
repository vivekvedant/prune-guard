use prune_guard::Config;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn default_config_is_safety_first() {
    let cfg = Config::default();

    assert!(!cfg.dry_run, "dry_run must default to false");
    assert_eq!(cfg.interval_secs, 900);
    assert_eq!(cfg.high_watermark_percent, 85);
    assert_eq!(cfg.target_watermark_percent, 70);
    assert_eq!(cfg.min_unused_age_days, 30);
    assert_eq!(cfg.max_delete_per_run_gb, 10);
    assert!(
        !cfg.allow_missing_image_labels,
        "allow_missing_image_labels must default to false"
    );
    assert_eq!(cfg.enabled_backends, vec!["docker".to_string()]);
    assert!(cfg.protected_images.is_empty());
    assert!(cfg.protected_volumes.is_empty());
    assert!(cfg.protected_labels.is_empty());
    assert_eq!(cfg.docker_host, None);
    assert_eq!(cfg.docker_context, None);
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
            allow_missing_image_labels = true
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
    assert!(cfg.allow_missing_image_labels);
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
            allow_missing_image_labels = true
            protected_images = ["postgres:16"]

            [docker]
            context = "desktop-linux"
        "#,
    ))
    .expect("sectioned TOML should parse");

    assert_eq!(cfg.interval_secs, 1800);
    assert_eq!(cfg.high_watermark_percent, 90);
    assert_eq!(cfg.target_watermark_percent, 75);
    assert!(!cfg.dry_run);
    assert!(cfg.allow_missing_image_labels);
    assert_eq!(cfg.protected_images, vec!["postgres:16"]);
    assert_eq!(cfg.docker_context.as_deref(), Some("desktop-linux"));
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

#[test]
fn parse_supports_docker_host_override() {
    let cfg = Config::parse_str(
        r#"
            [docker]
            host = "unix:///home/vivek/.docker/desktop/docker.sock"
        "#,
    )
    .expect("docker.host should parse");

    assert_eq!(
        cfg.docker_host.as_deref(),
        Some("unix:///home/vivek/.docker/desktop/docker.sock")
    );
    assert_eq!(cfg.docker_context, None);
}

#[test]
fn parse_rejects_ambiguous_docker_host_and_context() {
    let err = Config::parse_str(
        r#"
            [docker]
            host = "unix:///var/run/docker.sock"
            context = "desktop-linux"
        "#,
    )
    .expect_err("host and context together must fail closed");

    assert!(
        err.to_string()
            .contains("docker.host and docker.context cannot both be set"),
        "unexpected error: {err}"
    );
}

#[test]
fn install_config_template_exists_and_is_safety_first() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let install_cfg_path = repo_root.join("config/prune-guard.toml");

    let cfg = Config::load_from_path(&install_cfg_path).unwrap_or_else(|err| {
        panic!(
            "install config template should exist and parse ({}): {err}",
            install_cfg_path.display()
        )
    });

    assert!(
        !cfg.dry_run,
        "installed config template must default to real-run mode"
    );
    assert!(
        cfg.target_watermark_percent < cfg.high_watermark_percent,
        "installed config template must keep target watermark below high watermark"
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
