use std::path::Path;

use super::*;
use crate::config::testing::ConfigBuilder;

fn config_for(clone: &Path) -> Config {
    ConfigBuilder::new(clone).build()
}

fn write_page(clone: &Path, repo_relative_path: &str, frontmatter: &str, body: &str) {
    let full = clone.join(repo_relative_path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, format!("---\n{frontmatter}\n---\n{body}\n")).unwrap();
}

const WELL_FORMED_FRONTMATTER: &str =
    "type: reference\ntitle: A page\ntags:\n  - one\nstatus: current\nupdated: 2024-01-01";

#[test]
fn a_missing_clone_exits_4_stale_not_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    // Deliberately never created: `clone` does not exist on disk at all.
    let clone = tmp.path().join("clone");
    let config = config_for(&clone);

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.outcome, LintOutcome::CloneMissing);
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

#[test]
fn an_empty_corpus_exits_3_the_gap_signal_not_a_violation() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_for(&clone);

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.outcome, LintOutcome::NoFilesFound);
    assert_eq!(report.exit_code(), ExitCode::NoHits);
}

#[test]
fn a_path_argument_escaping_the_clone_is_rejected_as_invalid_not_read() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    std::fs::write(tmp.path().join("secret.md"), "outside the clone").unwrap();
    let config = config_for(&clone);

    let report = LintVerb::new(&config, vec!["../secret.md".to_string()]).gather();

    assert_eq!(
        report.outcome,
        LintOutcome::InvalidPath {
            path: "../secret.md".to_string()
        }
    );
    assert_eq!(report.exit_code(), ExitCode::Usage);
}

#[test]
fn a_path_argument_naming_a_file_that_does_not_exist_is_invalid_not_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_for(&clone);

    let report = LintVerb::new(&config, vec!["nowhere.md".to_string()]).gather();

    assert_eq!(
        report.outcome,
        LintOutcome::InvalidPath {
            path: "nowhere.md".to_string()
        }
    );
    assert_eq!(report.exit_code(), ExitCode::Usage);
}

#[test]
fn a_structural_violation_makes_the_whole_run_exit_2_as_bad_input() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        "type: reference",
        "Body.",
    );
    let config = config_for(&clone);

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match report.outcome {
        LintOutcome::Finished { violations, .. } => {
            assert!(
                violations
                    .iter()
                    .any(|v| v.severity == Severity::Structural)
            );
        }
        other => panic!("expected Finished with violations, got {other:?}"),
    }
}

#[test]
fn a_well_formed_page_with_only_a_prose_style_slip_still_exits_0() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        WELL_FORMED_FRONTMATTER,
        "A sentence \u{2014} with an em dash.",
    );
    let config = config_for(&clone);

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Success);
    match report.outcome {
        LintOutcome::Finished {
            files_checked,
            violations,
        } => {
            assert_eq!(files_checked, 1);
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0].severity, Severity::Heuristic);
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn linting_a_single_path_argument_only_checks_that_file_not_the_whole_corpus() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/good.md",
        WELL_FORMED_FRONTMATTER,
        "Fine.",
    );
    write_page(&clone, "kaibo/reference/bad.md", "type: reference", "Fine.");
    let config = config_for(&clone);

    let report = LintVerb::new(&config, vec!["kaibo/reference/good.md".to_string()]).gather();

    assert_eq!(report.exit_code(), ExitCode::Success);
    match report.outcome {
        LintOutcome::Finished { files_checked, .. } => assert_eq!(files_checked, 1),
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn explain_prints_nothing_to_run_because_lint_shells_out_to_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_for(&clone);

    let plan = LintVerb::new(&config, Vec::new()).explain();

    assert!(plan.is_empty());
}
