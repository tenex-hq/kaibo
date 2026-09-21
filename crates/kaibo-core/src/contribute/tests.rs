use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::clock::testing::FixedClock;
use crate::config::ConfigSource;
use crate::config::testing::ConfigBuilder;
use crate::process::testing::{FakeCommandRunner, failed, ok};

const NOW_EPOCH: u64 = 1_700_000_000; // 2023-11-14

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
}

fn config_with_repo(clone: &std::path::Path) -> crate::config::Config {
    ConfigBuilder::new(clone)
        .repo("org/knowledge", ConfigSource::File)
        .build()
}

fn valid_input() -> ApplyInput {
    ApplyInput {
        content_type: "how-to".to_string(),
        domain: "kaibo".to_string(),
        title: "Write a good query".to_string(),
        body: "Some body text.".to_string(),
        tags: vec!["good-tag".to_string()],
        placement: Placement::Create,
        binding: None,
    }
}

/// The same contribution, filed as a binding standard.
fn binding_input() -> ApplyInput {
    ApplyInput {
        binding: Some(BindingInput {
            severity: normative::Severity::Must,
            actions: vec![normative::ActionKind::FileEdit],
            tags: vec!["workload-repo".to_string()],
        }),
        ..valid_input()
    }
}

// --- slugify -------------------------------------------------------------

#[test]
fn slugify_lowercases_and_hyphenates_a_plain_title() {
    assert_eq!(slugify("Write A Good Query"), "write-a-good-query");
}

#[test]
fn slugify_strips_shell_metacharacters_entirely_rather_than_hyphenating_them_all() {
    // `; rm -rf /` would, if any metacharacter survived into a branch name
    // passed as a single argv element, still not be shell-interpreted (no
    // shell sits between kaibo and git), but this asserts the defense in
    // depth this module documents: none of `;`, `$`, backticks, or `/`
    // reach the slug at all.
    let slug = slugify("title; rm -rf / `whoami` $(evil)");
    assert!(!slug.contains(';'));
    assert!(!slug.contains('$'));
    assert!(!slug.contains('`'));
    assert!(!slug.contains('/'));
    assert_eq!(slug, "title-rm-rf-whoami-evil");
}

#[test]
fn slugify_drops_a_newline_rather_than_letting_it_split_into_two_lines() {
    let slug = slugify("first line\nContribute/evil");
    assert!(!slug.contains('\n'));
    assert_eq!(slug, "first-line-contribute-evil");
}

#[test]
fn slugify_of_an_all_punctuation_title_is_empty() {
    assert_eq!(slugify("!!!"), "");
}

#[test]
fn slugify_has_no_leading_or_trailing_hyphen() {
    assert_eq!(slugify("-hello-"), "hello");
}

// --- `plan` ----------------------------------------------------------------

fn write_moc(clone: &std::path::Path, contents: &str) {
    std::fs::write(clone.join("_index.md"), contents).unwrap();
}

#[test]
fn plan_never_calls_git_or_gh_and_writes_nothing_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n- **owner:** team\n");
    let config = config_with_repo(&clone);

    let runner =
        FakeCommandRunner::new().on(QmdCommand::query(&config, "how do queries work"), ok("[]"));
    let verb = ContributePlanVerb::new(&config, "how do queries work", None, None);
    let before: Vec<_> = std::fs::read_dir(&clone).unwrap().collect();

    let report = verb.gather(&runner);

    let after: Vec<_> = std::fs::read_dir(&clone).unwrap().collect();
    assert_eq!(
        before.len(),
        after.len(),
        "plan must not write to the clone"
    );
    for call in runner.calls() {
        assert_ne!(call.program, "git", "plan must never shell out to git");
        assert_ne!(call.program, "gh", "plan must never shell out to gh");
    }
    assert!(matches!(report.outcome, PlanOutcome::Ready { .. }));
}

#[test]
fn plan_reports_an_ambiguity_for_each_unresolved_field_and_never_prompts() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok("[]"));

    let report = ContributePlanVerb::new(&config, "gist", None, None).gather(&runner);

    assert_eq!(report.ambiguities.len(), 2);
    assert!(report.target_path.is_none());
}

#[test]
fn plan_computes_a_target_path_only_once_both_type_and_domain_are_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok("[]"));

    let report = ContributePlanVerb::new(
        &config,
        "gist",
        Some("how-to".to_string()),
        Some("kaibo".to_string()),
    )
    .gather(&runner);

    assert_eq!(report.target_path.as_deref(), Some("kaibo/how-to/gist.md"));
    assert!(report.ambiguities.is_empty());
}

#[test]
fn plan_degrades_to_empty_candidates_when_qmd_is_unreachable_rather_than_failing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let runner =
        FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), failed("qmd: not found"));

    let report = ContributePlanVerb::new(&config, "gist", None, None).gather(&runner);

    match &report.outcome {
        PlanOutcome::Ready {
            candidates,
            qmd_unavailable,
            ..
        } => {
            assert!(candidates.is_empty());
            // The exact failing stderr, not just "some detail was set" -
            // otherwise a mutant that always treats the qmd output as a
            // success (and then fails to parse its empty stdout as JSON,
            // producing some *other* detail message) would survive.
            assert_eq!(qmd_unavailable.as_deref(), Some("qmd: not found"));
        }
        other => panic!("expected Ready with degraded candidates, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Success);
}

#[test]
fn plan_candidates_carry_the_exact_path_and_score_qmd_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let response = r#"[{"score":0.87,"file":"qmd://knowledge/kaibo/reference/x.md?index=kaibo"}]"#;
    let runner = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok(response));

    let report = ContributePlanVerb::new(&config, "gist", None, None).gather(&runner);

    match &report.outcome {
        PlanOutcome::Ready {
            candidates,
            qmd_unavailable,
            ..
        } => {
            assert!(qmd_unavailable.is_none());
            assert_eq!(
                candidates,
                &vec![Candidate {
                    path: "kaibo/reference/x.md".to_string(),
                    score: 0.87,
                }]
            );
        }
        other => panic!("expected Ready with a real candidate, got {other:?}"),
    }
}

#[test]
fn plan_target_path_is_none_when_only_one_of_type_and_domain_is_a_valid_segment() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok("[]"));

    // A valid type but an invalid (path-escaping) domain.
    let report = ContributePlanVerb::new(
        &config,
        "gist",
        Some("how-to".to_string()),
        Some("../escape".to_string()),
    )
    .gather(&runner);
    assert!(report.target_path.is_none());

    // The reverse: a valid domain but an invalid type.
    let runner2 = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok("[]"));
    let report2 = ContributePlanVerb::new(
        &config,
        "gist",
        Some("../escape".to_string()),
        Some("kaibo".to_string()),
    )
    .gather(&runner2);
    assert!(report2.target_path.is_none());
}

#[test]
fn plan_explain_lists_the_qmd_query_and_runs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);

    let commands = ContributePlanVerb::new(&config, "gist", None, None).explain();

    assert_eq!(commands, vec![QmdCommand::query(&config, "gist")]);
}

#[test]
fn plan_explain_is_empty_when_the_clone_does_not_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("missing");
    let config = config_with_repo(&clone);

    assert!(
        ContributePlanVerb::new(&config, "gist", None, None)
            .explain()
            .is_empty()
    );
}

// --- `apply`: pre-branch stop conditions -----------------------------------

#[test]
fn a_dirty_clone_stops_apply_before_anything_is_written_or_branched() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(git_status_porcelain(&clone), ok(" M some-file.md\n"));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, valid_input()).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::UncommittedChanges { .. })
    ));
    assert_eq!(report.returned_to_main, None);
    assert!(!clone.join("kaibo/how-to/write-a-good-query.md").exists());
    assert_eq!(report.exit_code(), ExitCode::Stale);
    // Only the status check ran - no branch, no add, no commit.
    assert_eq!(runner.calls(), vec![git_status_porcelain(&clone)]);
}

#[test]
fn an_invalid_domain_segment_stops_apply_before_touching_the_clone() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());
    let mut input = valid_input();
    input.domain = "../escape".to_string();

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::InvalidField {
            field: "domain",
            ..
        })
    ));
    assert!(runner.calls().is_empty());
}

#[test]
fn a_page_that_fails_structural_lint_stops_apply_and_leaves_the_write_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(git_status_porcelain(&clone), ok(""));
    let clock = FixedClock(now());
    let mut input = valid_input();
    // Not kebab-case: the `tags-kebab-case` structural rule must fire.
    input.tags = vec!["NotKebabCase".to_string()];

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Stopped(ApplyStop::LintFailed { violations }) => {
            assert!(violations.iter().any(|v| v.rule_id == "tags-kebab-case"));
        }
        other => panic!("expected LintFailed, got {other:?}"),
    }
    // Stop-and-report, never discard: the write stays on disk uncommitted.
    assert!(clone.join("kaibo/how-to/write-a-good-query.md").exists());
    assert_eq!(report.returned_to_main, None);
    // No branch was ever created.
    assert!(runner.calls().iter().all(|c| !(c.program == "git"
        && c.args.contains(&"checkout".to_string())
        && c.args.contains(&"-b".to_string()))));
}

#[test]
fn a_branch_name_that_already_exists_stops_apply_without_creating_it() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let branch = "contribute/write-a-good-query";
    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(
            git_branch_list(&clone, branch),
            ok("  contribute/write-a-good-query\n"),
        );
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, valid_input()).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::BranchCollision { .. })
    ));
    assert_eq!(report.returned_to_main, None);
}

// --- `apply`: sanitisation of caller-supplied text -------------------------

#[test]
fn a_title_with_a_newline_and_shell_metacharacters_never_reaches_the_commit_message_raw() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let mut input = valid_input();
    input.title = "evil\ntitle; $(rm -rf /)".to_string();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = format!(
        "{}/{}/{}.md",
        input.domain,
        input.content_type,
        slugify(&input.title)
    );

    let message = commit_message(&input);
    assert!(!message.contains('\n'));

    // `pr_body` uses `\n\n` as its own section separator, so the body as a
    // whole legitimately contains newlines - what must never happen is the
    // *raw*, un-sanitised title (still carrying its embedded newline and
    // metacharacters) ending up in the body.
    let body = pr_body(&input);
    assert!(!body.contains(&input.title));
    assert!(body.contains(&trust::strip_control_chars(&input.title)));

    // The branch/path derived from this title carry none of the offending
    // characters either.
    assert!(!branch.contains(';') && !branch.contains('\n') && !branch.contains('$'));
    assert!(!path.contains(';') && !path.contains('\n') && !path.contains('$'));

    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(&input);
    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &message), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("true\n"))
        .on(git_push(&clone, "origin", &branch), ok(""))
        .on(
            gh_pr_create("org/knowledge", "main", &branch, &title, &body),
            ok("https://github.com/org/knowledge/pull/1\n"),
        )
        .on(
            gh_pr_checks("org/knowledge", "https://github.com/org/knowledge/pull/1"),
            ok(""),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());
    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);
    // Sanity: this input is otherwise well-formed, so it completes,
    // proving the sanitised values above are exactly what a real run
    // uses rather than the input being rejected first for some other
    // reason.
    assert!(matches!(report.outcome, ApplyOutcome::Completed { .. }));
}

// --- `apply`: fork-parent verification --------------------------------------

#[test]
fn a_fork_whose_parent_is_not_the_configured_repo_stops_apply_and_never_pushes_to_it() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = target_path(&input);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("false\n"))
        .on(gh_whoami(), ok("attacker\n"))
        .on(gh_fork("org/knowledge"), ok(""))
        // The fork exists, but its parent is a *different* repo than the
        // one kaibo is configured against - a squatted fork.
        .on(
            gh_fork_parent("attacker", "knowledge"),
            ok("someone-else/knowledge\n"),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Stopped(ApplyStop::ForkParentMismatch { expected, actual }) => {
            assert_eq!(expected, "org/knowledge");
            assert_eq!(actual, "someone-else/knowledge");
        }
        other => panic!("expected ForkParentMismatch, got {other:?}"),
    }
    // No push, to either remote, was ever attempted.
    assert!(
        runner
            .calls()
            .iter()
            .all(|c| !(c.program == "git" && c.args.contains(&"push".to_string())))
    );
    // The clone was still returned to `main`, per the unconditional rule.
    assert_eq!(report.returned_to_main, Some(true));
}

// --- `apply`: unconditional return to `main` --------------------------------

#[test]
fn apply_returns_the_clone_to_main_even_when_the_push_itself_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = target_path(&input);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("true\n"))
        .on(
            git_push(&clone, "origin", &branch),
            failed("network unreachable"),
        )
        .on(git_checkout_main(&clone), ok("Switched to branch 'main'\n"));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::PushFailed { .. })
    ));
    assert_eq!(report.returned_to_main, Some(true));
    assert!(runner.calls().contains(&git_checkout_main(&clone)));
}

// --- `apply`: happy paths ----------------------------------------------------

#[test]
fn a_direct_push_contribution_writes_a_draft_page_opens_a_pr_and_reports_ci() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = target_path(&input);
    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(&input);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("true\n"))
        .on(git_push(&clone, "origin", &branch), ok(""))
        .on(
            gh_pr_create("org/knowledge", "main", &branch, &title, &body),
            ok("creating pull request\nhttps://github.com/org/knowledge/pull/42\n"),
        )
        .on(
            gh_pr_checks("org/knowledge", "https://github.com/org/knowledge/pull/42"),
            ok("All checks passed\n"),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Completed {
            push_route,
            pr_url,
            ci,
        } => {
            assert_eq!(push_route.kind, PushRouteKind::Direct);
            assert_eq!(pr_url, "https://github.com/org/knowledge/pull/42");
            assert_eq!(*ci, CiVerdict::Passed);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Success);
    assert_eq!(report.returned_to_main, Some(true));

    let written = std::fs::read_to_string(clone.join(&path)).unwrap();
    assert!(written.starts_with("---\n"));
    assert!(written.contains("status: draft"));
    assert!(written.contains("updated: 2023-11-14"));
    assert!(written.contains("Some body text."));
}

#[test]
fn a_caller_without_push_access_forks_verifies_the_parent_and_pushes_there() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = target_path(&input);
    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(&input);
    let head = "contributor:contribute/write-a-good-query".to_string();

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("false\n"))
        .on(gh_whoami(), ok("contributor\n"))
        .on(gh_fork("org/knowledge"), ok(""))
        .on(
            gh_fork_parent("contributor", "knowledge"),
            ok("org/knowledge\n"),
        )
        .on(
            git_remote_add(
                &clone,
                "contribute-fork",
                "https://github.com/contributor/knowledge.git",
            ),
            ok(""),
        )
        .on(git_push(&clone, "contribute-fork", &branch), ok(""))
        .on(
            gh_pr_create("org/knowledge", "main", &head, &title, &body),
            ok("https://github.com/org/knowledge/pull/7\n"),
        )
        .on(
            gh_pr_checks("org/knowledge", "https://github.com/org/knowledge/pull/7"),
            ok(""),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Completed {
            push_route, pr_url, ..
        } => {
            assert_eq!(push_route.kind, PushRouteKind::Fork);
            assert_eq!(push_route.owner.as_deref(), Some("contributor"));
            assert_eq!(pr_url, "https://github.com/org/knowledge/pull/7");
        }
        other => panic!("expected Completed via fork, got {other:?}"),
    }
}

#[test]
fn a_red_pr_is_reported_as_a_failed_ci_verdict_and_a_non_success_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let path = target_path(&input);
    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(&input);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, &path), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("true\n"))
        .on(git_push(&clone, "origin", &branch), ok(""))
        .on(
            gh_pr_create("org/knowledge", "main", &branch, &title, &body),
            ok("https://github.com/org/knowledge/pull/9\n"),
        )
        .on(
            gh_pr_checks("org/knowledge", "https://github.com/org/knowledge/pull/9"),
            failed("2 checks failing"),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Completed {
            ci: CiVerdict::Failed { .. },
            ..
        } => {}
        other => panic!("expected a failed CI verdict, got {other:?}"),
    }
    assert_eq!(
        report.exit_code(),
        ExitCode::Usage,
        "a red PR is not a finished contribution"
    );
}

// --- `apply`: append mode ----------------------------------------------------

#[test]
fn appending_bumps_updated_and_preserves_the_existing_title_and_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(clone.join("kaibo/how-to")).unwrap();
    let existing = "---\ntitle: Existing page\ntype: how-to\ntags:\n  - existing-tag\nstatus: current\nupdated: 2020-01-01\n---\nOriginal body.\n";
    std::fs::write(clone.join("kaibo/how-to/existing.md"), existing).unwrap();
    let config = config_with_repo(&clone);

    let mut input = valid_input();
    input.placement = Placement::Append {
        path: "kaibo/how-to/existing.md".to_string(),
    };
    let branch = format!("contribute/{}", slugify(&input.title));

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(git_branch_list(&clone, &branch), ok(""))
        .on(git_checkout_new_branch(&clone, &branch), ok(""))
        .on(git_add(&clone, "kaibo/how-to/existing.md"), ok(""))
        .on(git_commit(&clone, &commit_message(&input)), ok(""))
        .on(gh_permission_check("org/knowledge"), ok("true\n"))
        .on(git_push(&clone, "origin", &branch), ok(""))
        .on(
            gh_pr_create(
                "org/knowledge",
                "main",
                &branch,
                &trust::strip_control_chars(&input.title),
                &pr_body(&input),
            ),
            ok("https://github.com/org/knowledge/pull/1\n"),
        )
        .on(
            gh_pr_checks("org/knowledge", "https://github.com/org/knowledge/pull/1"),
            ok(""),
        )
        .on(git_checkout_main(&clone), ok(""));
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    assert!(matches!(report.outcome, ApplyOutcome::Completed { .. }));
    let written = std::fs::read_to_string(clone.join("kaibo/how-to/existing.md")).unwrap();
    assert!(written.contains("title: Existing page"));
    assert!(written.contains("existing-tag"));
    assert!(written.contains("updated: 2023-11-14"));
    assert!(written.contains("Original body."));
    assert!(written.contains("Some body text."));
}

#[test]
fn appending_to_a_path_outside_the_clone_is_rejected_before_anything_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    // A real file that exists, but outside the clone.
    let outside = tmp.path().join("secret.md");
    std::fs::write(&outside, "---\ntitle: x\n---\nbody").unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(git_status_porcelain(&clone), ok(""));
    let clock = FixedClock(now());

    let mut input = valid_input();
    input.placement = Placement::Append {
        path: "../secret.md".to_string(),
    };

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::AppendTargetUnreadable { .. })
    ));
}

// --- `--explain` -------------------------------------------------------------

#[test]
fn apply_explain_is_non_empty_and_names_no_command_runner_to_call() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    let config = config_with_repo(&clone);

    // `Explainable::explain` takes no `&dyn CommandRunner` at all - the
    // type signature itself is the proof this runs nothing; this test
    // additionally checks the output is non-trivial, i.e. actually useful
    // as the safety affordance the verb promises.
    let commands = ContributeApplyVerb::new(&config, valid_input()).explain();

    assert!(commands.len() >= 8);
    assert!(commands.iter().any(|c| c.program == "git"));
    assert!(commands.iter().any(|c| c.program == "gh"));
}

// --- exit codes --------------------------------------------------------------

#[test]
fn repo_not_configured_is_a_usage_error_and_touches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = ConfigBuilder::new(&clone).build();
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, valid_input()).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::RepoNotConfigured)
    ));
    assert_eq!(report.exit_code(), ExitCode::Usage);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_missing_clone_is_a_stale_error_not_a_usage_error() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("does-not-exist");
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, valid_input()).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::CloneMissing)
    ));
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

// --- rendering ---------------------------------------------------------------

#[test]
fn a_lint_failure_finding_names_the_real_clone_path_not_the_branch_name() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(git_status_porcelain(&clone), ok(""));
    let clock = FixedClock(now());
    let mut input = valid_input();
    input.tags = vec!["NotKebabCase".to_string()];

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    let text = report.render_text(&RenderOptions::default());
    let clone_display = clone.display().to_string();
    assert!(
        text.contains(&clone_display),
        "the fix instruction must name the real clone path, got: {text}"
    );
    assert!(
        !text.contains("contribute/write-a-good-query checkout"),
        "the fix instruction must not name the branch as if it were a path, got: {text}"
    );
}

// --- targeted mutation-gap coverage -----------------------------------------

#[test]
fn commit_message_names_the_title_and_the_placement_verb_exactly() {
    let mut input = valid_input();
    input.title = "Write a good query".to_string();
    input.placement = Placement::Create;
    assert_eq!(commit_message(&input), "contribute: add Write a good query");

    input.placement = Placement::Append {
        path: "kaibo/how-to/existing.md".to_string(),
    };
    assert_eq!(
        commit_message(&input),
        "contribute: update Write a good query"
    );
}

#[test]
fn a_report_that_did_return_to_main_carries_no_return_to_main_finding() {
    let report = ApplyReport {
        path: "kaibo/how-to/x.md".to_string(),
        branch: "contribute/x".to_string(),
        clone_display: "/tmp/corpus".to_string(),
        returned_to_main: Some(true),
        outcome: ApplyOutcome::Completed {
            push_route: PushRoute {
                kind: PushRouteKind::Direct,
                owner: None,
            },
            pr_url: "https://github.com/org/knowledge/pull/1".to_string(),
            ci: CiVerdict::Passed,
        },
    };

    assert!(report.findings().is_empty());
}

#[test]
fn a_failed_return_to_main_is_reported_as_its_own_finding() {
    let report = ApplyReport {
        path: "kaibo/how-to/x.md".to_string(),
        branch: "contribute/x".to_string(),
        clone_display: "/tmp/corpus".to_string(),
        returned_to_main: Some(false),
        outcome: ApplyOutcome::Completed {
            push_route: PushRoute {
                kind: PushRouteKind::Direct,
                owner: None,
            },
            pr_url: "https://github.com/org/knowledge/pull/1".to_string(),
            ci: CiVerdict::Passed,
        },
    };

    let findings = report.findings();
    assert_eq!(findings.len(), 1);
    assert!(findings[0].0.contains("/tmp/corpus"));
}

#[test]
fn an_unreadable_git_status_stops_apply_as_clone_unreadable_not_uncommitted_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(
        git_status_porcelain(&clone),
        failed("fatal: not a git repository"),
    );
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, valid_input()).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Stopped(ApplyStop::CloneUnreadable { detail }) => {
            assert!(detail.contains("not a git repository"));
        }
        other => panic!("expected CloneUnreadable, got {other:?}"),
    }
}

#[test]
fn appending_to_a_directory_is_rejected_as_an_unreadable_target() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    // A real, contained "path" that resolves fine but is a directory, not
    // a file - must be rejected the same as a missing path, not read as
    // if it were a page.
    std::fs::create_dir_all(clone.join("kaibo/how-to/existing.md")).unwrap();
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new().on(git_status_porcelain(&clone), ok(""));
    let clock = FixedClock(now());
    let mut input = valid_input();
    input.placement = Placement::Append {
        path: "kaibo/how-to/existing.md".to_string(),
    };

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    assert!(matches!(
        report.outcome,
        ApplyOutcome::Stopped(ApplyStop::AppendTargetUnreadable { .. })
    ));
}

#[test]
fn a_failing_branch_list_command_stops_apply_as_checkout_failed_not_a_collision() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    let config = config_with_repo(&clone);
    let input = valid_input();
    let branch = format!("contribute/{}", slugify(&input.title));
    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain(&clone), ok(""))
        .on(
            git_branch_list(&clone, &branch),
            failed("git: command not found"),
        );
    let clock = FixedClock(now());

    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &clock);

    match &report.outcome {
        ApplyOutcome::Stopped(ApplyStop::CheckoutFailed { detail }) => {
            assert!(detail.contains("git: command not found"));
        }
        other => panic!("expected CheckoutFailed, got {other:?}"),
    }
}

// --- `today` (civil-from-days) -----------------------------------------------

#[test]
fn today_converts_several_known_epoch_seconds_to_their_correct_calendar_dates() {
    // Independently known (epoch seconds, Y, M, D) pairs, not derived from
    // the function under test, chosen to stress the arithmetic
    // differently: the Unix epoch itself, an ordinary mid-month date, a
    // leap day, the day right after a leap day, a non-leap-year February
    // end, and a year boundary.
    let cases: &[(u64, u16, u8, u8)] = &[
        (0, 1970, 1, 1),
        (1_700_000_000, 2023, 11, 14),
        (951_782_400, 2000, 2, 29), // leap day
        (951_868_800, 2000, 3, 1),  // day after a leap day
        (1_421_362_265, 2015, 1, 15),
        (946_684_799, 1999, 12, 31), // one second before a year boundary
        (946_684_800, 2000, 1, 1),   // the year boundary itself
    ];

    for &(epoch, year, month, day) in cases {
        let date = today(&FixedClock(UNIX_EPOCH + Duration::from_secs(epoch)));
        assert_eq!(
            (date.year, date.month, date.day),
            (year, month, day),
            "epoch {epoch} converted incorrectly"
        );
    }
}

// --- rendering: `PlanReport` and `ApplyReport` JSON/text --------------------

#[test]
fn plan_render_text_and_json_carry_the_resolved_fields_and_candidate_count() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(&clone).unwrap();
    write_moc(&clone, "## kaibo\n");
    let config = config_with_repo(&clone);
    let response = r#"[{"score":0.5,"file":"qmd://knowledge/kaibo/reference/x.md?index=kaibo"}]"#;
    let runner = FakeCommandRunner::new().on(QmdCommand::query(&config, "gist"), ok(response));
    let report = ContributePlanVerb::new(
        &config,
        "gist",
        Some("how-to".to_string()),
        Some("kaibo".to_string()),
    )
    .gather(&runner);

    let text = report.render_text(&RenderOptions::default());
    assert!(!text.is_empty());
    assert!(text.contains("kaibo/how-to/gist.md"));
    assert!(text.contains("kaibo/reference/x.md"));

    let json = report.render_json();
    assert_eq!(json["target_path"], "kaibo/how-to/gist.md");
    assert_eq!(
        json["outcome"]["candidates"][0]["path"],
        "kaibo/reference/x.md"
    );
}

#[test]
fn apply_render_json_carries_the_pr_url_and_push_route_on_completion() {
    let report = ApplyReport {
        path: "kaibo/how-to/x.md".to_string(),
        branch: "contribute/x".to_string(),
        clone_display: "/tmp/corpus".to_string(),
        returned_to_main: Some(true),
        outcome: ApplyOutcome::Completed {
            push_route: PushRoute {
                kind: PushRouteKind::Fork,
                owner: Some("contributor".to_string()),
            },
            pr_url: "https://github.com/org/knowledge/pull/1".to_string(),
            ci: CiVerdict::Passed,
        },
    };

    let json = report.render_json();
    assert_eq!(json["outcome"]["state"], "completed");
    assert_eq!(json["outcome"]["push_route"], "fork");
    assert_eq!(json["outcome"]["fork_owner"], "contributor");
    assert_eq!(
        json["outcome"]["pr_url"],
        "https://github.com/org/knowledge/pull/1"
    );
    assert_eq!(json["outcome"]["ci"]["passed"], true);
}

// --- binding standards ---------------------------------------------------

/// Serialize the frontmatter `apply` would write for `input`, the same way
/// the Create path does, so these tests read the real bytes rather than the
/// struct that produced them.
fn written_frontmatter(input: &ApplyInput) -> String {
    let doc = Document {
        frontmatter: Frontmatter {
            doc_type: Some(input.content_type.clone()),
            title: Some(input.title.clone()),
            tags: Some(input.tags.clone()),
            status: Some(Status::Draft),
            updated: Some(Date::parse("2023-11-14").unwrap()),
            extra: binding_frontmatter(input.binding.as_ref()),
        },
        body: input.body.clone(),
    };
    frontmatter::serialize(&doc).unwrap()
}

#[test]
fn the_page_a_binding_contribution_writes_is_one_the_schema_accepts() {
    // The point of typing the input: what `contribute` writes and what
    // `normative` reads cannot drift apart, because this fails if they do.
    let contents = written_frontmatter(&binding_input());
    let doc = frontmatter::parse(&contents).unwrap();

    let standard = normative::parse(&doc.frontmatter, &doc.body)
        .unwrap()
        .expect("the written page is a binding standard");

    assert_eq!(standard.severity, normative::Severity::Must);
    assert_eq!(
        standard.applies_to.actions,
        vec![normative::ActionKind::FileEdit]
    );
    assert_eq!(standard.applies_to.tags, vec!["workload-repo".to_string()]);
    assert_eq!(standard.checks, Vec::new());
}

#[test]
fn a_contribution_that_is_not_binding_writes_none_of_the_normative_keys() {
    let contents = written_frontmatter(&valid_input());
    let doc = frontmatter::parse(&contents).unwrap();

    assert_eq!(normative::parse(&doc.frontmatter, &doc.body).unwrap(), None);
    assert!(!contents.contains("binding"));
    assert!(!contents.contains("severity"));
    assert!(!contents.contains("applies_to"));
}

#[test]
fn no_narrowing_tags_writes_no_tags_key_rather_than_an_empty_one() {
    // `tags: []` reads as a deliberate empty filter. "No tags given" is a
    // different statement, and the frontmatter has to say the second one.
    let input = ApplyInput {
        binding: Some(BindingInput {
            severity: normative::Severity::Should,
            actions: vec![normative::ActionKind::CommitMessage],
            tags: Vec::new(),
        }),
        ..valid_input()
    };
    let contents = written_frontmatter(&input);

    assert!(contents.contains("severity: should"));
    assert!(contents.contains("commit-message"));
    assert!(
        !contents.contains("tags: []"),
        "wrote an empty narrowing filter: {contents}"
    );

    let doc = frontmatter::parse(&contents).unwrap();
    let standard = normative::parse(&doc.frontmatter, &doc.body)
        .unwrap()
        .expect("still a binding standard");
    assert_eq!(standard.applies_to.tags, Vec::<String>::new());
}

#[test]
fn every_action_kind_the_schema_knows_survives_the_round_trip() {
    // A vocabulary sweep, not an enumeration: an action added to the schema
    // and not serializable here fails without anyone remembering to look.
    for action in normative::ActionKind::ALL {
        let input = ApplyInput {
            binding: Some(BindingInput {
                severity: normative::Severity::Must,
                actions: vec![action],
                tags: Vec::new(),
            }),
            ..valid_input()
        };
        let contents = written_frontmatter(&input);
        let doc = frontmatter::parse(&contents).unwrap();
        let standard = normative::parse(&doc.frontmatter, &doc.body)
            .unwrap()
            .unwrap_or_else(|| panic!("{} did not round trip", action.as_str()));
        assert_eq!(standard.applies_to.actions, vec![action]);
    }
}

#[test]
fn a_binding_standard_is_not_appended_to_a_page_that_already_states_one() {
    // One page, one normative claim, one verdict. Appending a second claim
    // to a page that already binds is the atomicity failure itself, so it
    // stops before anything is written rather than being linted afterwards.
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    std::fs::create_dir_all(clone.join("kaibo/how-to")).unwrap();
    let config = config_with_repo(&clone);

    let input = ApplyInput {
        placement: Placement::Append {
            path: "kaibo/how-to/existing.md".to_string(),
        },
        ..binding_input()
    };
    let runner = FakeCommandRunner::new();
    let report = ContributeApplyVerb::new(&config, input).apply(&runner, &FixedClock(now()));

    match &report.outcome {
        ApplyOutcome::Stopped(stop) => assert_eq!(
            stop,
            &ApplyStop::BindingOnAppend {
                path: "kaibo/how-to/existing.md".to_string()
            }
        ),
        other => panic!("expected Stopped, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Usage);
}
