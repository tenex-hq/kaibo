use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::clock::testing::FixedClock;
use crate::config::ConfigSource;
use crate::config::testing::ConfigBuilder;
use crate::error::ExitCode;
use crate::explain::Explainable;
use crate::process::testing::{FakeCommandRunner, failed, failed_with_stdout, ok};
use crate::qmd::QmdCommand;

const NOW_EPOCH: u64 = 1_700_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
}

fn config_with_repo(clone: &Path) -> crate::config::Config {
    ConfigBuilder::new(clone)
        .repo("org/corpus", ConfigSource::File)
        .build()
}

fn git_dir(clone: &Path) {
    std::fs::create_dir_all(clone.join(".git")).unwrap();
}

fn fresh_pipeline(config: &crate::config::Config, clone: &Path) -> FakeCommandRunner {
    FakeCommandRunner::new()
        .on(git_status_porcelain_command(clone), ok(""))
        .on(git_checkout_main_command(clone), ok("Switched to branch 'main'\n"))
        .on(git_pull_command(clone), ok("Already up to date.\n"))
        .on(QmdCommand::collection_list(config), ok("No collections found.\n"))
        .on(
            QmdCommand::collection_add(config, clone, config.collection(), COLLECTION_MASK),
            ok("Collection 'knowledge' created successfully\n"),
        )
        .on(QmdCommand::update(config), ok("All collections updated.\n"))
        .on(QmdCommand::embed(config), ok("Done.\n"))
        .on(
            QmdCommand::status(config),
            ok("QMD Status\n\nDocuments\n  Total:    3 files indexed\n  Vectors:  3 embedded\n  Updated:  1s ago\n"),
        )
}

/// Git subcommands that cannot run a repository-supplied hook, and so are
/// the only ones allowed to omit the hooks-disabled flag. Keep this list
/// short: justify every addition against whether it writes the working
/// tree, fetches objects, or can recurse into a submodule.
const HOOKLESS_GIT_SUBCOMMANDS: [&str; 2] = ["status", "log"];

/// Guardrail: every git command `sync` plans carries
/// `-c core.hooksPath=/dev/null`, because a knowledge repo must never
/// execute code on this machine.
///
/// Asserts over the whole planned pipeline, not a hand-listed set of
/// constructors, so a command added to `planned_commands` later is swept
/// from day one. Both clone states are checked, since the clone command
/// only appears in one of them.
///
/// Does not cover `--if-stale`'s freshness probe (`git log`), which runs
/// outside `planned_commands` - it is read-only, so it is exempted via
/// `HOOKLESS_GIT_SUBCOMMANDS` rather than swept here.
#[test]
fn hooks_are_disabled_on_every_git_command_sync_plans() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    let config = config_with_repo(&clone);

    let mut checked = 0;
    for clone_present in [false, true] {
        for command in planned_commands(&config, clone_present) {
            if command.program != "git" {
                continue;
            }
            // The subcommand is the first bare word, skipping git's
            // own leading options - and crucially the *value* that
            // follows `-C` or `-c`, which is a bare word too. Matching
            // on "first non-flag argument" alone would read the clone
            // path out of `git -C <path> status` and never see a
            // subcommand at all.
            let mut args = command.args.iter();
            let subcommand = loop {
                let Some(arg) = args.next() else {
                    break "";
                };
                if arg == "-C" || arg == "-c" {
                    args.next();
                    continue;
                }
                if arg.starts_with('-') {
                    continue;
                }
                break arg.as_str();
            };
            if HOOKLESS_GIT_SUBCOMMANDS.contains(&subcommand) {
                continue;
            }
            assert!(
                command
                    .args
                    .windows(2)
                    .any(|w| w == ["-c", "core.hooksPath=/dev/null"]),
                "missing hooks-disabled flag on: {command}"
            );
            checked += 1;
        }
    }

    assert!(
        checked >= 3,
        "expected to sweep at least clone, checkout and pull; swept {checked}"
    );
}

/// Qmd subcommands within `sync`'s planned pipeline that are exempt from
/// carrying `--index` - none, today. Unlike `qmd.rs`'s own guardrail, which
/// carves out `QmdCommand::version` (addresses no index at all) and
/// `default_index_collection_list` (deliberately reads qmd's default index,
/// to prove it was left untouched), `sync` never has a legitimate reason to
/// build either of those: every command it plans writes to or reads from the
/// one configured index. Kept as an explicit (empty) list, named here rather
/// than skipped ad hoc inside the loop, so a future exemption has to be
/// argued for in a comment next to this one.
const INDEXLESS_QMD_SUBCOMMANDS: [&str; 0] = [];

/// Guardrail: every qmd command `sync` plans carries `--index` with the
/// configured value, not a literal - the same guarantee `qmd.rs` enforces
/// for `QmdCommand::index_command` itself, checked again here at the point
/// where `sync` assembles its pipeline.
///
/// Asserts over the whole planned pipeline, not a hand-listed set of
/// constructor calls, so a qmd command added to `planned_commands` later -
/// which would carry `--index` only if its author remembered to route it
/// through `QmdCommand::index_command` - is swept from day one instead of
/// silently escaping a list that only knows about today's four commands.
/// Both clone states are checked, since the clone step is a `git` command
/// and never appears in this sweep either way.
#[test]
fn sync_commands_carry_the_configured_index() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("corpus");
    let config = ConfigBuilder::new(&clone)
        .repo("org/corpus", ConfigSource::File)
        .index("from-config-not-a-literal", ConfigSource::File)
        .build();

    let mut checked = 0;
    for clone_present in [false, true] {
        for command in planned_commands(&config, clone_present) {
            if command.program != "qmd" {
                continue;
            }
            let subcommand = command.args.first().map(String::as_str).unwrap_or("");
            if INDEXLESS_QMD_SUBCOMMANDS.contains(&subcommand) {
                continue;
            }
            let position = command
                .args
                .iter()
                .position(|arg| arg == "--index")
                .unwrap_or_else(|| panic!("no --index in: {command}"));
            assert_eq!(command.args[position + 1], "from-config-not-a-literal");
            checked += 1;
        }
    }

    assert!(
        checked >= 8,
        "expected to sweep collection list, collection add, update and embed \
         across both clone states; swept {checked}"
    );
}

#[test]
fn missing_clone_bootstraps() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);

    let runner = fresh_pipeline(&config, &clone)
        .on(git_clone_command("org/corpus", &clone), ok("Cloning...\n"));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { clone, .. } => assert_eq!(*clone, CloneOutcome::Bootstrapped),
        other => panic!("expected Completed(Bootstrapped), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Success);
}

#[test]
fn uncommitted_changes_stop_the_verb_without_discarding() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    // Deliberately no scripted response for checkout/pull/qmd: if gather
    // tried to proceed past the dirty check, this test would panic on
    // "no scripted response", which is exactly the guarantee under test.
    let runner = FakeCommandRunner::new().on(
        git_status_porcelain_command(&clone),
        ok(" M some/file.md\n"),
    );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Stopped(SyncStop::UncommittedChanges { .. }) => {}
        other => panic!("expected Stopped(UncommittedChanges), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
    let findings = report.findings();
    assert!(findings.iter().any(|f| f.message.contains("uncommitted")));
    assert!(findings.iter().any(|f| {
        f.fix
            .as_deref()
            .is_some_and(|fix| fix.contains("commit or stash"))
    }));
}

#[test]
fn blocked_checkout_stops_the_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(
            git_checkout_main_command(&clone),
            failed("error: pathspec 'main' did not match any file(s) known to git"),
        );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Stopped(SyncStop::CheckoutBlocked { .. }) => {}
        other => panic!("expected Stopped(CheckoutBlocked), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

#[test]
fn pull_failure_stops_the_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(
            git_pull_command(&clone),
            failed("fatal: unable to access repo"),
        );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Stopped(SyncStop::PullFailed { .. }) => {}
        other => panic!("expected Stopped(PullFailed), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

#[test]
fn clone_failure_when_bootstrapping_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new().on(
        git_clone_command("org/corpus", &clone),
        failed("fatal: repository not found"),
    );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Stopped(SyncStop::CloneFailed { .. }) => {}
        other => panic!("expected Stopped(CloneFailed), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

#[test]
fn missing_repo_config_is_a_usage_error_and_runs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    // No repo configured at all - ConfigBuilder defaults repo to None.
    let config = ConfigBuilder::new(&clone).build();

    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Stopped(SyncStop::RepoNotConfigured) => {}
        other => panic!("expected Stopped(RepoNotConfigured), got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Usage);
    assert!(runner.calls().is_empty());
}

#[test]
fn if_stale_is_a_noop_when_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    // Only the freshness probe is scripted; any further call panics.
    let runner = FakeCommandRunner::new().on(
        git_last_commit_command_for_test(&clone),
        ok(format!("{}\n", NOW_EPOCH - 60)),
    );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, true);

    assert_eq!(report.outcome, SyncOutcome::SkippedFresh);
    assert_eq!(report.exit_code(), ExitCode::Success);
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn if_stale_acts_when_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let ten_days_secs = 10 * 24 * 60 * 60;
    let runner = fresh_pipeline(&config, &clone).on(
        git_last_commit_command_for_test(&clone),
        ok(format!("{}\n", NOW_EPOCH - ten_days_secs)),
    );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, true);

    match &report.outcome {
        SyncOutcome::Completed { clone, .. } => assert_eq!(*clone, CloneOutcome::Pulled),
        other => panic!("expected Completed(Pulled), got {other:?}"),
    }
}

#[test]
fn if_stale_with_absent_clone_still_runs_the_bootstrap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);

    let runner = fresh_pipeline(&config, &clone)
        .on(git_clone_command("org/corpus", &clone), ok("Cloning...\n"));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, true);

    match &report.outcome {
        SyncOutcome::Completed { clone, .. } => assert_eq!(*clone, CloneOutcome::Bootstrapped),
        other => panic!("expected Completed(Bootstrapped), got {other:?}"),
    }
}

#[test]
fn collection_already_present_is_not_recreated() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    // No scripted response for collection_add: if gather tried to
    // create it anyway, the test panics.
    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on(
            QmdCommand::collection_list(&config),
            ok("knowledge (qmd://knowledge/)\n"),
        )
        .on(QmdCommand::update(&config), ok("updated\n"))
        .on(QmdCommand::embed(&config), ok("embedded\n"))
        .on(
            QmdCommand::status(&config),
            ok("QMD Status\n\nDocuments\n  Total:    3 files indexed\n  Vectors:  3 embedded\n"),
        );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { collection, .. } => {
            assert_eq!(*collection, CollectionState::AlreadyPresent)
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[test]
fn missing_collection_is_created() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = fresh_pipeline(&config, &clone);
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { collection, .. } => {
            assert_eq!(*collection, CollectionState::Created)
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[test]
fn qmd_unavailable_while_ensuring_collection_is_a_finding_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on_missing(QmdCommand::collection_list(&config));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed {
            collection, index, ..
        } => {
            assert!(matches!(collection, CollectionState::Unavailable { .. }));
            assert!(matches!(
                index,
                crate::status::IndexStatus::Unavailable { .. }
            ));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
    assert!(report.findings().iter().any(|f| f.fix.is_some()));
}

#[test]
fn no_files_matched_mask_is_reported_as_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on(
            QmdCommand::collection_list(&config),
            ok("No collections found.\n"),
        )
        .on(
            QmdCommand::collection_add(&config, &clone, config.collection(), COLLECTION_MASK),
            ok("created\n"),
        )
        .on(
            QmdCommand::update(&config),
            ok("No files found matching pattern.\n"),
        )
        .on(QmdCommand::embed(&config), ok("nothing to embed\n"))
        .on(
            QmdCommand::status(&config),
            ok("QMD Status\n\nDocuments\n  Total:    0 files indexed\n  Vectors:  0 embedded\n"),
        );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    assert_eq!(report.exit_code(), ExitCode::NoHits);
}

#[test]
fn healthy_full_sync_exits_success_with_no_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = fresh_pipeline(&config, &clone);
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    assert_eq!(report.exit_code(), ExitCode::Success);
    assert!(report.findings().is_empty(), "{:?}", report.findings());
}

#[test]
fn explain_lists_git_and_qmd_commands_and_executes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new();

    let commands = SyncVerb::new(&config).explain();

    assert!(!commands.is_empty());
    assert!(commands.iter().any(|c| c.program == "git"));
    assert!(commands.iter().any(|c| c.program == "qmd"));
    assert!(runner.calls().is_empty());
}

/// Asserts on the literal `git clone` argv element, not a substring of the
/// rendered command line: the clone *path* is itself named `.../clone` in
/// every test in this file, so a loose `.contains("clone")` check would
/// pass even if the `git clone` command were never planned at all, as long
/// as some other planned command's path argument happened to contain the
/// same word.
#[test]
fn explain_includes_clone_command_when_clone_is_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);

    let commands = SyncVerb::new(&config).explain();

    assert!(
        commands
            .iter()
            .any(|c| c.program == "git" && c.args.iter().any(|a| a == "clone")),
        "expected a `git clone` command, got: {commands:?}"
    );
}

/// The complement of the test above: once the clone already exists,
/// `explain` must not plan a second `git clone` on top of it.
#[test]
fn explain_omits_clone_command_when_clone_is_already_present() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let commands = SyncVerb::new(&config).explain();

    assert!(
        !commands
            .iter()
            .any(|c| c.program == "git" && c.args.iter().any(|a| a == "clone")),
        "did not expect a `git clone` command, got: {commands:?}"
    );
}

#[test]
fn explain_is_empty_when_repo_unconfigured_and_clone_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = ConfigBuilder::new(&clone).build();

    let commands = SyncVerb::new(&config).explain();

    assert!(commands.is_empty());
}

#[test]
fn render_text_mentions_result_and_next_command_on_a_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = ConfigBuilder::new(&clone).build();
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);
    let text = report.render_text(&crate::output::RenderOptions::default());

    assert!(text.contains("problem found"));
    assert!(text.contains("kaibo sync"));
}

#[test]
fn render_json_reports_the_stop_reason_as_a_stable_machine_code() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    // No repo configured, and no clone either - stops with RepoNotConfigured.
    let config = ConfigBuilder::new(&clone).build();
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);
    let json = report.render_json();

    assert_eq!(json["outcome"]["state"], "stopped");
    assert_eq!(json["outcome"]["reason"], "repo_not_configured");
    assert_eq!(json["exit_code"], 2);
}

/// `--if-stale`'s freshness probe running but exiting non-zero (a real git
/// failure, not merely "not found") must read the same as any other
/// unreadable probe: not fresh, so the sync pipeline still runs rather
/// than skipping on a guess.
#[test]
fn if_stale_treats_a_failing_freshness_probe_as_not_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    // `stdout` here is deliberately a value that *would* parse as a fresh
    // commit if the success check were skipped: an empty stdout can't fail
    // either way (unparsable regardless -> "not fresh" either way), so it
    // can't tell "the check was skipped" apart from "the check ran and
    // correctly treated the failure as unreadable".
    let runner = fresh_pipeline(&config, &clone).on(
        git_last_commit_command_for_test(&clone),
        failed_with_stdout(format!("{}\n", NOW_EPOCH - 60)),
    );
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, true);

    match &report.outcome {
        SyncOutcome::Completed { clone, .. } => assert_eq!(*clone, CloneOutcome::Pulled),
        other => panic!(
            "expected the sync pipeline to run despite an unreadable freshness probe, got {other:?}"
        ),
    }
}

/// `qmd update` running but exiting non-zero must be reported as an
/// unavailable index carrying its stderr, not silently treated as if it had
/// succeeded and left to `qmd embed`/`qmd status` to paper over.
#[test]
fn a_failing_update_command_is_an_unavailable_index_not_a_success() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on(QmdCommand::collection_list(&config), ok("knowledge\n"))
        .on(QmdCommand::update(&config), failed("update exploded"));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { index, .. } => match index {
            crate::status::IndexStatus::Unavailable { detail } => {
                assert_eq!(detail, "update exploded")
            }
            other => panic!("expected Unavailable, got {other:?}"),
        },
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

/// Same as above, one step later in the pipeline: `qmd embed` failing must
/// not be masked by `update` having already succeeded.
#[test]
fn a_failing_embed_command_is_an_unavailable_index_not_a_success() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on(QmdCommand::collection_list(&config), ok("knowledge\n"))
        .on(
            QmdCommand::update(&config),
            ok("All collections updated.\n"),
        )
        .on(QmdCommand::embed(&config), failed("embed exploded"));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { index, .. } => match index {
            crate::status::IndexStatus::Unavailable { detail } => {
                assert_eq!(detail, "embed exploded")
            }
            other => panic!("expected Unavailable, got {other:?}"),
        },
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

/// Same again, one step further: `qmd status` failing after a successful
/// `update`/`embed` must still be reported as unavailable, not skipped.
#[test]
fn a_failing_status_command_is_an_unavailable_index_not_a_success() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);

    let runner = FakeCommandRunner::new()
        .on(git_status_porcelain_command(&clone), ok(""))
        .on(git_checkout_main_command(&clone), ok("Already on 'main'\n"))
        .on(git_pull_command(&clone), ok("Already up to date.\n"))
        .on(QmdCommand::collection_list(&config), ok("knowledge\n"))
        .on(
            QmdCommand::update(&config),
            ok("All collections updated.\n"),
        )
        .on(QmdCommand::embed(&config), ok("Done.\n"))
        .on(QmdCommand::status(&config), failed("status exploded"));
    let clock = FixedClock(now());

    let report = SyncVerb::new(&config).gather(&runner, &clock, false);

    match &report.outcome {
        SyncOutcome::Completed { index, .. } => match index {
            crate::status::IndexStatus::Unavailable { detail } => {
                assert_eq!(detail, "status exploded")
            }
            other => panic!("expected Unavailable, got {other:?}"),
        },
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(report.exit_code(), ExitCode::Stale);
}

// A helper duplicating status.rs's private git-last-commit command
// builder, since sync's own `is_fresh` probe must issue the exact same
// command status.rs's staleness check does - see the module doc for why
// this is exposed as `pub(crate)` from `status` rather than
// reimplemented independently.
fn git_last_commit_command_for_test(path: &Path) -> crate::explain::PlannedCommand {
    crate::status::git_last_commit_command(path)
}
