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

fn list_markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("failed to read directory {}: {err}", dir.display()));

        for entry in entries {
            let entry = entry.unwrap_or_else(|err| {
                panic!("failed to inspect directory entry in {}: {err}", dir.display())
            });
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

fn markdown_files_matching(root: &Path, folder: &str, needle: &str) -> Vec<PathBuf> {
    let base = root.join(folder);
    list_markdown_files(&base)
        .into_iter()
        .filter(|path| {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            file_name.contains(&needle.to_ascii_lowercase())
        })
        .collect()
}

fn assert_has_fail_closed_and_dry_run_default(path: &Path) {
    let content = read_text(path);
    assert!(
        contains_case_insensitive(&content, "fail-closed")
            || contains_case_insensitive(&content, "fail closed"),
        "{} must mention fail-closed (or fail closed) safety behavior",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "dry-run"),
        "{} must mention dry-run behavior",
        path.display()
    );
    assert!(
        contains_case_insensitive(&content, "by default")
            || contains_case_insensitive(&content, "default dry-run")
            || contains_case_insensitive(&content, "dry-run default")
            || contains_case_insensitive(&content, "default behavior")
            || contains_case_insensitive(&content, "default runtime mode"),
        "{} must state dry-run default behavior explicitly",
        path.display()
    );
}

#[test]
fn docs_and_flowcharts_readmes_list_phase_nine() {
    let root = repo_root();
    let docs_readme = read_text(&root.join("docs/README.md"));
    let flowcharts_readme = read_text(&root.join("flowcharts/README.md"));

    assert!(
        contains_case_insensitive(&docs_readme, "phase-9"),
        "docs/README.md must include a Phase 9 entry"
    );
    assert!(
        contains_case_insensitive(&flowcharts_readme, "phase-9"),
        "flowcharts/README.md must include a Phase 9 entry"
    );
}

#[test]
fn phase_nine_release_readiness_docs_exist_and_include_safety_language() {
    let root = repo_root();
    let docs_phase_nine = markdown_files_matching(&root, "docs", "phase-9");
    let flowcharts_phase_nine = markdown_files_matching(&root, "flowcharts", "phase-9");

    assert!(
        !docs_phase_nine.is_empty(),
        "expected at least one Phase 9 markdown doc in docs/"
    );
    assert!(
        !flowcharts_phase_nine.is_empty(),
        "expected at least one Phase 9 markdown doc in flowcharts/"
    );

    for path in docs_phase_nine.iter().chain(flowcharts_phase_nine.iter()) {
        let content = read_text(path);
        assert!(
            contains_case_insensitive(&content, "release"),
            "{} should describe release-readiness behavior",
            path.display()
        );
        assert_has_fail_closed_and_dry_run_default(path);
    }
}

#[test]
fn runbook_and_checklist_files_exist_and_state_safe_defaults() {
    let root = repo_root();
    let all_markdown = list_markdown_files(&root);
    let mut runbooks = Vec::new();
    let mut checklists = Vec::new();

    for path in all_markdown {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if file_name.contains("runbook") {
            runbooks.push(path.clone());
        }
        if file_name.contains("checklist") {
            checklists.push(path);
        }
    }

    assert!(
        !runbooks.is_empty(),
        "expected at least one runbook markdown file in the repository"
    );
    assert!(
        !checklists.is_empty(),
        "expected at least one checklist markdown file in the repository"
    );

    for path in runbooks.iter().chain(checklists.iter()) {
        assert_has_fail_closed_and_dry_run_default(path);
    }
}
