use std::fs;

#[test]
fn circleci_rust_image_uses_a_versioned_tag() {
    let config = fs::read_to_string(".circleci/config.yml")
        .expect("CI config must be readable for safety validation");

    let rust_image_line = config
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("- image: cimg/rust:"))
        .expect("CI config must declare a Rust image");

    assert!(
        !rust_image_line.contains(":stable"),
        "Version alias tags are unsafe because they can disappear; pin a semver tag instead: {rust_image_line}"
    );
    assert!(
        rust_image_line.contains("cimg/rust:1."),
        "Rust image must be pinned to a concrete major.minor[.patch] tag: {rust_image_line}"
    );
}

#[test]
fn circleci_workflow_triggers_only_for_pull_requests_to_main() {
    let config = fs::read_to_string(".circleci/config.yml")
        .expect("CI config must be readable for trigger validation");

    assert!(
        config.contains("pipeline.event.name == \"pull_request\""),
        "workflow must explicitly target pull_request events"
    );
    assert!(
        config.contains("pipeline.event.github.pull_request.base.ref == \"main\""),
        "workflow must explicitly target pull requests whose base branch is main"
    );
    assert!(
        !config.contains("branches:\n              only: main"),
        "branch-only filters are push-based and should not be used for PR-to-main-only workflows"
    );
}
