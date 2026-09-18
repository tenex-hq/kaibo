use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::config::ConfigSource;
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
fn render_text_names_the_rule_and_severity_of_a_structural_violation() {
    let report = LintReport {
        paths: Vec::new(),
        outcome: LintOutcome::Finished {
            files_checked: 1,
            violations: vec![Violation {
                rule_id: "frontmatter-contract".to_string(),
                severity: Severity::Structural,
                path: "kaibo/reference/page.md".to_string(),
                message: "missing required frontmatter field `title`".to_string(),
            }],
        },
    };

    let text = report.render_text(&RenderOptions::default());

    assert!(text.contains("[structural]"));
    assert!(text.contains("frontmatter-contract"));
    assert!(text.contains("kaibo/reference/page.md"));
    assert!(text.contains("missing required frontmatter field `title`"));
}

#[test]
fn render_text_names_a_heuristic_violation_distinctly_from_a_structural_one() {
    let report = LintReport {
        paths: Vec::new(),
        outcome: LintOutcome::Finished {
            files_checked: 1,
            violations: vec![Violation {
                rule_id: "prose-style".to_string(),
                severity: Severity::Heuristic,
                path: "kaibo/reference/page.md".to_string(),
                message: "body contains an em dash (U+2014); use a plain hyphen instead"
                    .to_string(),
            }],
        },
    };

    let text = report.render_text(&RenderOptions::default());

    assert!(text.contains("[heuristic]"));
    assert!(!text.contains("[structural]"));
}

#[test]
fn render_json_encodes_each_violation_field_as_the_literal_value_it_carries() {
    let report = LintReport {
        paths: vec!["kaibo/reference/page.md".to_string()],
        outcome: LintOutcome::Finished {
            files_checked: 1,
            violations: vec![Violation {
                rule_id: "tags-kebab-case".to_string(),
                severity: Severity::Structural,
                path: "kaibo/reference/page.md".to_string(),
                message: "tag \"bad_tag\" is not kebab-case".to_string(),
            }],
        },
    };

    let json = report.render_json();

    assert_eq!(json["outcome"]["state"], "finished");
    assert_eq!(json["outcome"]["files_checked"], 1);
    let violation = &json["outcome"]["violations"][0];
    assert_eq!(violation["rule_id"], "tags-kebab-case");
    assert_eq!(violation["severity"], "structural");
    assert_eq!(violation["path"], "kaibo/reference/page.md");
    assert_eq!(violation["message"], "tag \"bad_tag\" is not kebab-case");
}

#[test]
fn render_json_reports_the_clone_missing_state_by_name() {
    let report = LintReport {
        paths: Vec::new(),
        outcome: LintOutcome::CloneMissing,
    };

    assert_eq!(report.render_json()["outcome"]["state"], "clone_missing");
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

// --- configurable lint parameters, end to end ---------------------------

#[test]
fn disabling_a_rule_removes_its_violations_from_the_run() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        WELL_FORMED_FRONTMATTER,
        "A sentence \u{2014} with an em dash.",
    );
    let config = ConfigBuilder::new(&clone)
        .lint_disabled_rules(vec!["prose-style".to_string()], ConfigSource::File)
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Success);
    match report.outcome {
        LintOutcome::Finished { violations, .. } => assert_eq!(violations, Vec::new()),
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn a_custom_required_frontmatter_key_is_enforced_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    // Well-formed by the compiled default, but missing `owner`.
    write_page(
        &clone,
        "kaibo/reference/page.md",
        WELL_FORMED_FRONTMATTER,
        "Fine.",
    );
    let config = ConfigBuilder::new(&clone)
        .lint_required_frontmatter_keys(
            vec!["title".to_string(), "owner".to_string()],
            ConfigSource::File,
        )
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match report.outcome {
        LintOutcome::Finished { violations, .. } => {
            assert!(violations.iter().any(|v| v.message.contains("`owner`")));
            // Dropped from required_keys, so its absence is not flagged
            // even though this page also has no `status`... it does, in
            // WELL_FORMED_FRONTMATTER, so assert the positive instead:
            // nothing about `tags` or `updated` is reported either, they
            // were not requested to be dropped, they're just still
            // present and fine.
            assert!(!violations.iter().any(|v| v.message.contains("`tags`")));
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn a_configured_allowed_status_list_flags_an_unlisted_status_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        "type: reference\ntitle: A page\ntags:\n  - one\nstatus: draft\nupdated: 2024-01-01",
        "Fine.",
    );
    let config = ConfigBuilder::new(&clone)
        .lint_allowed_status(
            vec!["current".to_string(), "deprecated".to_string()],
            ConfigSource::File,
        )
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match report.outcome {
        LintOutcome::Finished { violations, .. } => {
            assert!(violations.iter().any(|v| v.message.contains("draft")));
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn a_type_folder_override_remaps_the_expected_type_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/howto/page.md",
        "type: how-to\ntitle: A page\ntags:\n  - one\nstatus: current\nupdated: 2024-01-01",
        "Fine.",
    );
    let mut overrides = BTreeMap::new();
    overrides.insert("howto".to_string(), "how-to".to_string());
    let config = ConfigBuilder::new(&clone)
        .lint_type_folder_overrides(overrides, ConfigSource::File)
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    // Without the override, `type: how-to` under a folder literally named
    // `howto` would mismatch the identity mapping and fail; the override
    // makes this page pass.
    assert_eq!(report.exit_code(), ExitCode::Success);
}

#[test]
fn a_custom_tag_pattern_accepts_what_the_default_kebab_case_pattern_rejects_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        "type: reference\ntitle: A page\ntags:\n  - rule_registry\nstatus: current\nupdated: 2024-01-01",
        "Fine.",
    );
    let config = ConfigBuilder::new(&clone)
        .lint_tag_pattern("[a-z0-9_]+", ConfigSource::File)
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Success);
}

#[test]
fn an_invalid_tag_pattern_config_fails_closed_before_any_file_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        WELL_FORMED_FRONTMATTER,
        "Fine.",
    );
    let config = ConfigBuilder::new(&clone)
        .lint_tag_pattern("[unclosed", ConfigSource::File)
        .build();

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match report.outcome {
        LintOutcome::InvalidConfig { detail } => assert!(detail.contains("[unclosed")),
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

/// The guardrail: a page in the corpus clone cannot change which lint
/// rules run or how they're parameterised, no matter how it tries. This
/// page's frontmatter carries a `lint:` table shaped exactly like the real
/// `[lint]` config-file section, attempting to relax the tag pattern to
/// accept anything and to disable `tags-kebab-case` outright. Neither
/// reaches the registry: `LintedFile` is built from an already-resolved
/// `Config`, and `rules::registry` is called with that `Config`'s
/// `LintConfig`, never with anything parsed from a page. The tag this page
/// carries violates the *real* default pattern, so if the hostile
/// frontmatter had any effect, this test would go green for the wrong
/// reason - it doesn't, because the violation still fires.
#[test]
fn hostile_frontmatter_cannot_change_which_rules_run_or_how() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    write_page(
        &clone,
        "kaibo/reference/page.md",
        "type: reference\n\
         title: A page\n\
         tags:\n  - BAD_TAG\n\
         status: current\n\
         updated: 2024-01-01\n\
         lint:\n  \
           disabled_rules: [\"tags-kebab-case\"]\n  \
           tags_kebab_case:\n    \
             pattern: \".*\"",
        "Fine.",
    );
    // The default config: nothing here honours the page's `lint:` table.
    let config = config_for(&clone);

    // Sanity check the attack is real: the hostile table really is present
    // in the parsed page, extra and unconsumed.
    let contents = std::fs::read_to_string(clone.join("kaibo/reference/page.md")).unwrap();
    let doc = frontmatter::parse(&contents).unwrap();
    assert!(doc.frontmatter.extra.contains_key("lint"));

    let report = LintVerb::new(&config, Vec::new()).gather();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match report.outcome {
        LintOutcome::Finished { violations, .. } => {
            assert!(
                violations
                    .iter()
                    .any(|v| v.rule_id == "tags-kebab-case" && v.message.contains("BAD_TAG")),
                "expected the real default tag pattern to still reject BAD_TAG; got {violations:?}"
            );
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}
