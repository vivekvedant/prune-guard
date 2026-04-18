use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", path.display());
    })
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn assert_has_build_system_language(path: &Path) {
    let content = read_text(path);
    assert!(
        contains_case_insensitive(&content, "linux"),
        "{} must mention Linux",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "macos"),
        "{} must mention macOS",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "build matrix"),
        "{} must mention a build matrix",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "checksum"),
        "{} must mention checksums",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "smoke test"),
        "{} must mention smoke tests",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "fail-closed")
            || contains_case_insensitive(&content, "fail closed"),
        "{} must mention fail-closed release behavior",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "dry-run"),
        "{} must mention dry-run behavior",
        path.display()
    );
}

#[test]
fn circleci_cross_platform_workflow_exists_and_has_required_jobs() {
    let root = repo_root();
    let config = read_text(&root.join(".circleci/config.yml"));

    assert!(
        contains_case_insensitive(&config, "cross-platform-build-distribution"),
        "CircleCI config must define cross-platform workflow"
    );
    assert!(
        contains_case_insensitive(&config, "linux-build-package"),
        "CircleCI config must define linux build/package job"
    );
    assert!(
        contains_case_insensitive(&config, "macos-build-package"),
        "CircleCI config must define macOS build/package job"
    );
    assert!(
        !contains_case_insensitive(&config, "windows-build-package"),
        "CircleCI config must not define windows build/package jobs when windows is unsupported"
    );
    assert!(
        contains_case_insensitive(&config, "cargo test --locked"),
        "CircleCI cross-platform jobs must run locked tests"
    );
    assert!(
        contains_case_insensitive(&config, "cargo build --release --locked"),
        "CircleCI cross-platform jobs must run locked release builds"
    );
    assert!(
        contains_case_insensitive(&config, "Smoke test release outputs"),
        "CircleCI cross-platform jobs must include release smoke tests"
    );
    assert!(
        contains_case_insensitive(&config, "store_artifacts"),
        "CircleCI cross-platform jobs must store packaged artifacts"
    );
    assert!(
        contains_case_insensitive(&config, "scripts/release/package-artifacts-deb.sh"),
        "CircleCI linux workflow must package artifacts as a .deb"
    );
    assert!(
        contains_case_insensitive(&config, "-name \"*.deb\""),
        "CircleCI linux workflow must verify .deb artifacts explicitly"
    );
    assert!(
        contains_case_insensitive(&config, "/usr/bin/prune-guard"),
        "CircleCI linux workflow must verify installed daemon binary path inside .deb"
    );
    assert!(
        contains_case_insensitive(&config, "/lib/systemd/system/prune-guard.service"),
        "CircleCI linux workflow must verify systemd service path inside .deb"
    );
    assert!(
        contains_case_insensitive(&config, "/lib/systemd/system/prune-guard.timer"),
        "CircleCI linux workflow must verify systemd timer path inside .deb"
    );
    assert!(
        contains_case_insensitive(&config, "/usr/lib/prune-guard/release"),
        "CircleCI linux workflow must fail when recursive target/release payload is present in .deb"
    );
    assert!(
        !contains_case_insensitive(&config, "dpkg-deb --contents \"$archive_path\" | grep -q"),
        "CircleCI linux workflow must avoid grep -q pipelines that trigger SIGPIPE under pipefail"
    );
    assert!(
        !contains_case_insensitive(&config, "<<<"),
        "CircleCI config must not use bash here-strings because CircleCI v2.1 treats '<<' as template tags"
    );
    assert!(
        contains_case_insensitive(&config, "ignore: main"),
        "cross-platform workflow should avoid direct main-branch push pipelines"
    );
}

#[test]
fn github_actions_cross_platform_workflow_is_not_used() {
    let root = repo_root();
    let github_workflow = root.join(".github/workflows/build-cross-platform.yml");
    assert!(
        !github_workflow.exists(),
        "GitHub workflow should not exist when CircleCI is the source of truth"
    );
}

#[test]
fn packaging_scripts_generate_sha256_checksums() {
    let root = repo_root();
    let shell_script = read_text(&root.join("scripts/release/package-artifacts.sh"));
    let deb_script = read_text(&root.join("scripts/release/package-artifacts-deb.sh"));

    assert!(
        contains_case_insensitive(&shell_script, ".sha256"),
        "unix packaging script must emit sha256 files"
    );
    assert!(
        contains_case_insensitive(&shell_script, "sha256sum")
            || contains_case_insensitive(&shell_script, "shasum -a 256"),
        "unix packaging script must compute SHA256 digests"
    );
    assert!(
        contains_case_insensitive(&deb_script, ".sha256"),
        "linux deb packaging script must emit sha256 files"
    );
    assert!(
        contains_case_insensitive(&deb_script, "dpkg-deb --build"),
        "linux deb packaging script must build a .deb package"
    );
    assert!(
        contains_case_insensitive(&deb_script, "/usr/bin/prune-guard"),
        "linux deb packaging script must install daemon binary into /usr/bin"
    );
    assert!(
        contains_case_insensitive(&deb_script, "/lib/systemd/system/prune-guard.service"),
        "linux deb packaging script must install a systemd service unit"
    );
    assert!(
        contains_case_insensitive(&deb_script, "/lib/systemd/system/prune-guard.timer"),
        "linux deb packaging script must install a systemd timer unit"
    );
    assert!(
        contains_case_insensitive(&deb_script, "enable prune-guard.service"),
        "linux deb packaging script must enable daemon service for TOML-driven scheduling"
    );
}

#[test]
fn deb_packaging_script_keeps_payload_minimal_and_deterministic() {
    let root = repo_root();
    let deb_script = read_text(&root.join("scripts/release/package-artifacts-deb.sh"));

    assert!(
        !contains_case_insensitive(&deb_script, "/usr/lib/prune-guard/release"),
        "linux deb packaging script must not include the full target/release tree"
    );
    assert!(
        !contains_case_insensitive(&deb_script, "cp -R \"${release_dir}/.\""),
        "linux deb packaging script must not recursively copy the entire release directory"
    );
}

#[test]
fn systemd_units_exist_and_use_toml_interval_for_daemon_schedule() {
    let root = repo_root();
    let unit_path = root.join("packaging/systemd/prune-guard.service");
    let timer_path = root.join("packaging/systemd/prune-guard.timer");
    assert!(
        unit_path.exists(),
        "expected packaging/systemd/prune-guard.service"
    );
    assert!(
        timer_path.exists(),
        "expected packaging/systemd/prune-guard.timer"
    );

    let unit_content = read_text(&unit_path);
    let timer_content = read_text(&timer_path);
    assert!(
        contains_case_insensitive(&unit_content, "type=simple"),
        "systemd service must run as a long-lived daemon process"
    );
    assert!(
        contains_case_insensitive(&unit_content, "execstart=/usr/bin/prune-guard"),
        "systemd unit must run the installed prune-guard binary"
    );
    assert!(
        !contains_case_insensitive(&unit_content, "--once"),
        "systemd service must not force one-shot mode when TOML controls interval"
    );
    assert!(
        contains_case_insensitive(&unit_content, "/etc/prune-guard/prune-guard.toml"),
        "systemd unit must reference installed config path"
    );
    assert!(
        contains_case_insensitive(&unit_content, "wantedby=multi-user.target"),
        "systemd service must be enable-able at boot for TOML-driven scheduling"
    );
    assert!(
        !contains_case_insensitive(&timer_content, "onunitactivesec"),
        "systemd timer must not hardcode recurring cadence when TOML interval is authoritative"
    );
    assert!(
        contains_case_insensitive(&timer_content, "onbootsec"),
        "systemd timer should only bootstrap daemon startup after boot"
    );
    assert!(
        contains_case_insensitive(&timer_content, "unit=prune-guard.service"),
        "systemd timer must trigger prune-guard.service"
    );
}

#[test]
fn daemon_binary_source_exists_and_uses_install_config_default() {
    let root = repo_root();
    let main_rs_path = root.join("src/main.rs");
    assert!(
        main_rs_path.exists(),
        "expected src/main.rs daemon entrypoint"
    );

    let main_rs = read_text(&main_rs_path);
    assert!(
        contains_case_insensitive(&main_rs, "/etc/prune-guard/prune-guard.toml"),
        "daemon binary must default to installed config path"
    );
    assert!(
        contains_case_insensitive(&main_rs, "--once"),
        "daemon binary should provide one-shot mode for safe local validation"
    );
}

#[test]
fn build_system_docs_and_flowcharts_are_indexed() {
    let root = repo_root();
    let docs_readme = read_text(&root.join("docs/README.md"));
    let flowcharts_readme = read_text(&root.join("flowcharts/README.md"));

    assert!(
        contains_case_insensitive(&docs_readme, "cross-platform-build-distribution.md"),
        "docs/README.md must list the cross-platform build doc"
    );
    assert!(
        contains_case_insensitive(&flowcharts_readme, "cross-platform-build-distribution.md"),
        "flowcharts/README.md must list the cross-platform build flowchart"
    );
}

#[test]
fn build_docs_exist_and_cover_required_release_steps() {
    let root = repo_root();
    let docs_build = root.join("docs/cross-platform-build-distribution.md");
    let flowchart_build = root.join("flowcharts/cross-platform-build-distribution.md");

    assert!(
        docs_build.exists(),
        "expected docs/cross-platform-build-distribution.md"
    );
    assert!(
        flowchart_build.exists(),
        "expected flowcharts/cross-platform-build-distribution.md"
    );

    let docs_content = read_text(&docs_build);
    assert_has_build_system_language(&docs_build);
    assert!(
        contains_case_insensitive(&docs_content, "cross-platform build matrix"),
        "{} must include a build matrix section",
        docs_build.display()
    );
    assert!(
        contains_case_insensitive(&docs_content, "artifact packaging"),
        "{} must include an artifact packaging section",
        docs_build.display()
    );
    assert!(
        contains_case_insensitive(&docs_content, ".deb"),
        "{} must document linux .deb packaging",
        docs_build.display()
    );
    assert!(
        contains_case_insensitive(&docs_content, "checksums and integrity"),
        "{} must include a checksum section",
        docs_build.display()
    );
    assert!(
        contains_case_insensitive(&docs_content, "smoke test gate"),
        "{} must include a smoke-test gate section",
        docs_build.display()
    );
    assert!(
        contains_case_insensitive(&docs_content, "fail-closed release policy"),
        "{} must include a fail-closed release policy section",
        docs_build.display()
    );

    let flow_content = read_text(&flowchart_build);
    assert_has_build_system_language(&flowchart_build);
    assert!(
        contains_case_insensitive(&flow_content, "build matrix flow"),
        "{} must include a build matrix flow",
        flowchart_build.display()
    );
    assert!(
        contains_case_insensitive(&flow_content, "integrity and smoke test flow"),
        "{} must include an integrity and smoke test flow",
        flowchart_build.display()
    );
    assert!(
        contains_case_insensitive(&flow_content, "release gate flow"),
        "{} must include a release gate flow",
        flowchart_build.display()
    );
    assert!(
        contains_case_insensitive(&flow_content, ".deb"),
        "{} must document linux .deb packaging flow",
        flowchart_build.display()
    );
    assert!(
        contains_case_insensitive(&flow_content, "flowchart td"),
        "{} must be a mermaid flowchart document",
        flowchart_build.display()
    );
}

#[test]
fn build_and_test_guide_exists_is_indexed_and_has_binary_smoke_steps() {
    let root = repo_root();
    let guide_path = root.join("docs/build-and-test.md");
    let docs_readme = read_text(&root.join("docs/README.md"));

    assert!(guide_path.exists(), "expected docs/build-and-test.md");
    assert!(
        contains_case_insensitive(&docs_readme, "build-and-test.md"),
        "docs/README.md must list docs/build-and-test.md"
    );

    let guide = read_text(&guide_path);
    assert!(
        contains_case_insensitive(&guide, "cargo build --release --locked"),
        "build-and-test guide must include a locked release build command"
    );
    assert!(
        contains_case_insensitive(&guide, "cargo test --locked"),
        "build-and-test guide must include locked test execution"
    );
    assert!(
        contains_case_insensitive(&guide, "./target/release/prune-guard --help"),
        "build-and-test guide must include binary smoke-test help command"
    );
}
