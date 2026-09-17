use super::*;
use crate::clock::testing::FixedClock;
use crate::config::testing::ConfigBuilder;
use crate::process::testing::{FakeCommandRunner, failed, ok};

const NOW_EPOCH: u64 = 1_700_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
}

fn healthy_config(clone: &Path) -> Config {
    ConfigBuilder::new(clone).build()
}

fn sample_status_output() -> &'static str {
    "QMD Status\n\nDocuments\n  Total:    21 files indexed\n  Vectors:  38 embedded\n  Pending:  0 need embedding\n  Updated:  2h ago\n"
}

/// Real qmd 2.8.3 output once every document is embedded: the `Pending:`
/// line is omitted entirely rather than printed as `Pending: 0`.
/// Verified against a live qmd 2.8.3 binary, not assumed.
fn sample_status_output_fully_embedded() -> &'static str {
    "QMD Status\n\nDocuments\n  Total:    21 files indexed\n  Vectors:  38 embedded\n  Updated:  2h ago\n"
}

/// Real qmd 2.8.3 output when stale embedding chunks exist: an
/// `Orphaned:` line appears, unrelated to `Pending:` - it means "run qmd
/// cleanup", not "needs embedding" - and `Pending:` is still omitted
/// because nothing needs embedding.
fn sample_status_output_with_orphaned_chunks() -> &'static str {
    "QMD Status\n\nDocuments\n  Total:    39 files indexed\n  Vectors:  142 embedded\n  Orphaned: 49 embedding chunks (35%) \u{2014} run 'qmd cleanup'\n  Updated:  4d ago\n"
}

/// Mandatory regression test for the doc-drift fix: a `Pending:` line
/// missing from an otherwise-successful `qmd status` call means zero
/// docs are pending, not "unreadable". Getting this wrong previously
/// rendered a fully-synced index as `? pending` in `kaibo status`.
#[test]
fn missing_pending_line_on_success_means_zero_not_unknown() {
    assert_eq!(
        parse_qmd_status(sample_status_output_fully_embedded()),
        IndexStatus::Available {
            total_files: Some(21),
            vectors_embedded: Some(38),
            pending: Some(0),
        }
    );
}

/// The other half of the missing-`Pending:` rule: absence means zero
/// only when the output was recognisably qmd status. If a future qmd
/// changes its format wholesale, every field must degrade to unknown
/// together - reporting `0 pending` beside `? total` would be a guess
/// dressed as a fact.
#[test]
fn unrecognised_output_leaves_pending_unknown_rather_than_zero() {
    assert_eq!(
        parse_qmd_status("qmd: index summary unavailable in this build\n"),
        IndexStatus::Available {
            total_files: None,
            vectors_embedded: None,
            pending: None,
        }
    );
}

/// `Orphaned:` is a distinct "needs cleanup" signal, not a renamed
/// `Pending:` - it must not be folded into the pending count.
#[test]
fn orphaned_chunks_line_does_not_affect_pending_count() {
    assert_eq!(
        parse_qmd_status(sample_status_output_with_orphaned_chunks()),
        IndexStatus::Available {
            total_files: Some(39),
            vectors_embedded: Some(142),
            pending: Some(0),
        }
    );
}

#[test]
fn healthy_system_exits_zero_with_no_findings() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(report.exit_code(), ExitCode::Success);
    assert!(report.findings().is_empty(), "{:?}", report.findings());
    assert_eq!(
        report.qmd,
        QmdStatus::Found {
            version: "2.8.3".to_string(),
            matches_pin: true
        }
    );
    assert_eq!(report.isolation, IsolationStatus::Verified);
    assert_eq!(
        report.index,
        IndexStatus::Available {
            total_files: Some(21),
            vectors_embedded: Some(38),
            pending: Some(0),
        }
    );
}

/// Mandatory test: a fake runner reporting qmd absent still produces a
/// full report and a useful finding, instead of aborting.
#[test]
fn qmd_absent_does_not_abort_the_report() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on_missing(QmdCommand::version());
    // No response scripted for qmd status / collection list: gather
    // must not call them once qmd is known absent, or this test panics.
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(report.qmd, QmdStatus::NotFound);
    assert_eq!(
        report.index,
        IndexStatus::Unavailable {
            detail: "qmd not found on PATH".to_string()
        }
    );
    assert_eq!(report.isolation, IsolationStatus::UnverifiedQmdUnavailable);
    // Clone is healthy, so qmd's absence alone must not fail the exit code.
    assert_eq!(report.exit_code(), ExitCode::Success);

    let findings = report.findings();
    let finding = findings
        .iter()
        .find(|f| f.message.contains("qmd not found on PATH"))
        .expect("qmd-not-found finding");
    assert!(finding.fix.is_some());
}

#[test]
fn missing_clone_maps_to_stale_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    // Deliberately don't create the clone directory at all.
    let config = healthy_config(&tmp.path().join("nonexistent-clone"));

    let runner = FakeCommandRunner::new()
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(report.clone, CloneStatus::Absent);
    assert_eq!(report.exit_code(), ExitCode::Stale);
    assert!(
        report
            .findings()
            .iter()
            .any(|f| f.fix.as_deref() == Some("kaibo sync"))
    );
}

#[test]
fn stale_clone_maps_to_stale_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let ten_days_secs = 10 * 24 * 60 * 60;
    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - ten_days_secs)),
        )
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(report.exit_code(), ExitCode::Stale);
    assert!(
        report
            .findings()
            .iter()
            .any(|f| f.message.contains("stale"))
    );
}

#[test]
fn unreadable_git_metadata_is_a_finding_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let runner = FakeCommandRunner::new()
        .on(
            git_branch_command(tmp.path()),
            failed("fatal: not a git repository"),
        )
        .on(
            git_last_commit_command(tmp.path()),
            failed("fatal: bad revision"),
        )
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(
        report.clone,
        CloneStatus::Present {
            branch: None,
            last_commit_age: None
        }
    );
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

#[test]
fn version_mismatch_is_a_finding_not_a_failure() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(QmdCommand::version(), ok("qmd 9.9.9\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(
        report.qmd,
        QmdStatus::Found {
            version: "9.9.9".to_string(),
            matches_pin: false
        }
    );
    // A version other than the pin is a finding, not a failure.
    assert_eq!(report.exit_code(), ExitCode::Success);
    assert!(
        report
            .findings()
            .iter()
            .any(|f| f.message.contains("9.9.9"))
    );
}

#[test]
fn isolation_finding_when_collection_name_collides_with_default_index() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok(format!("{}\nsomething-else\n", config.collection())),
        );
    let clock = FixedClock(now());

    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    assert_eq!(
        report.isolation,
        IsolationStatus::NameCollisionInDefaultIndex
    );
    assert!(
        report
            .findings()
            .iter()
            .any(|f| f.message.contains("also present"))
    );
}

/// Mandatory test: `--explain` produces the planned commands and
/// executes none.
#[test]
fn explain_lists_commands_and_executes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = healthy_config(tmp.path());

    // `explain`'s signature never takes a runner, so this fake is never
    // wired in - it exists to assert the guarantee in the same shape as
    // every other test in this module, and would catch a future
    // refactor that threaded a runner through and called it by mistake.
    let runner = FakeCommandRunner::new();

    let commands = StatusVerb::new(&config).explain();

    assert!(!commands.is_empty());
    assert!(commands.iter().any(|c| c.program == "git"));
    assert!(commands.iter().any(|c| c.program == "qmd"));
    assert!(runner.calls().is_empty());
}

#[test]
fn explain_omits_git_commands_when_clone_is_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let config = healthy_config(&tmp.path().join("nonexistent-clone"));

    let commands = StatusVerb::new(&config).explain();

    assert!(commands.iter().all(|c| c.program != "git"));
    assert!(commands.iter().any(|c| c.program == "qmd"));
}

#[test]
fn relative_age_renders_common_buckets() {
    assert_eq!(render_relative_age(Duration::from_secs(10)), "just now");
    assert_eq!(render_relative_age(Duration::from_secs(90)), "1 minute ago");
    assert_eq!(
        render_relative_age(Duration::from_secs(3 * 60 * 60)),
        "3 hours ago"
    );
    assert_eq!(
        render_relative_age(Duration::from_secs(3 * 24 * 60 * 60)),
        "3 days ago"
    );
}

#[test]
fn render_text_is_narrow_by_default_and_mentions_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let config = healthy_config(&tmp.path().join("nonexistent-clone"));
    let runner = FakeCommandRunner::new().on_missing(QmdCommand::version());
    let clock = FixedClock(now());
    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    let text = report.render_text(&RenderOptions::default());
    assert!(text.contains("result: problem found"));
    assert!(text.contains("kaibo sync"));
    assert!(!text.contains("exit code:"));

    let full = report.render_text(&RenderOptions { full: true });
    assert!(full.contains("exit code: 4"));
}

/// `Config::source` is otherwise unused in the crate - this is its first
/// consumer, so exercise every field's value and provenance together,
/// including an API backend, rather than only the all-default config the
/// other tests build.
#[test]
fn render_text_shows_every_resolved_config_value_and_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let config = ConfigBuilder::new(tmp.path())
        .index("custom-index", ConfigSource::Env)
        .collection("custom-collection", ConfigSource::File)
        .repo("org/corpus", ConfigSource::File)
        .api_url("https://kaibo.example/api", ConfigSource::Env)
        .build();

    let runner = FakeCommandRunner::new()
        .on(git_branch_command(tmp.path()), ok("main\n"))
        .on(
            git_last_commit_command(tmp.path()),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(QmdCommand::version(), ok("qmd 2.8.3\n"))
        .on(QmdCommand::status(&config), ok(sample_status_output()))
        .on(
            QmdCommand::default_index_collection_list(),
            ok("some-other-collection\n"),
        );
    let clock = FixedClock(now());
    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    let text = report.render_text(&RenderOptions::default());
    assert!(text.contains("index=custom-index (env)"));
    assert!(text.contains("collection=custom-collection (file)"));
    assert!(text.contains("repo=org/corpus (file)"));
    assert!(text.contains("backend=api (https://kaibo.example/api)"));

    let json = report.render_json();
    assert_eq!(json["config"]["repo"]["source"], "file");
    assert_eq!(json["backend"]["mode"], "api");
}

#[test]
fn render_json_reports_found_false_when_qmd_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let config = healthy_config(&tmp.path().join("nonexistent-clone"));
    let runner = FakeCommandRunner::new().on_missing(QmdCommand::version());
    let clock = FixedClock(now());
    let report = StatusVerb::new(&config).gather(&runner, &clock, "0.1.0");

    let json = report.render_json();
    assert_eq!(json["qmd"]["found"], false);
    assert_eq!(json["clone"]["present"], false);
    assert_eq!(json["exit_code"], 4);
}
