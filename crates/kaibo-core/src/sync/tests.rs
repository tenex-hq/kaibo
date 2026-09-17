use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::clock::testing::FixedClock;
use crate::config::ConfigSource;
use crate::config::testing::ConfigBuilder;
use crate::error::ExitCode;
use crate::explain::Explainable;
use crate::process::testing::{FakeCommandRunner, failed, ok};
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

/// Git subcommands that cannot run a repository-supplied hook, and so
/// are the only ones allowed to omit the hooks-disabled flag. Keep this
/// list short and justify every addition: `status` and `log` only read,
/// and neither consults `core.hooksPath`. Anything that writes the
/// working tree, fetches objects, or can recurse into a submodule does
/// not belong here.
const HOOKLESS_GIT_SUBCOMMANDS: [&str; 2] = ["status", "log"];

/// Guardrail: every git command `sync` plans carries
/// `-c core.hooksPath=/dev/null`, because a knowledge repo must never
/// execute code on this machine.
///
/// This asserts over the whole planned pipeline rather than over a
/// hand-listed set of constructors, so a git command added to
/// `planned_commands` later is covered the day it is added rather than
/// the day someone remembers to extend a list. Both clone states are
/// swept, since the clone command only appears in one of them.
///
/// What it does *not* cover: a git command `gather` runs without
/// planning it. There is one today - `--if-stale` probes the clone's
/// last commit with `git log`, which `planned_commands` deliberately
/// omits because explain describes the full pipeline regardless of
/// freshness. That probe is read-only and so sits in the carve-out list
/// below on its own merits, but a future unplanned command would need
/// its own check.
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

#[test]
fn explain_includes_clone_command_when_clone_is_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);

    let commands = SyncVerb::new(&config).explain();

    assert!(commands.iter().any(|c| c.to_string().contains("clone")));
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

// A helper duplicating status.rs's private git-last-commit command
// builder, since sync's own `is_fresh` probe must issue the exact same
// command status.rs's staleness check does - see the module doc for why
// this is exposed as `pub(crate)` from `status` rather than
// reimplemented independently.
fn git_last_commit_command_for_test(path: &Path) -> crate::explain::PlannedCommand {
    crate::status::git_last_commit_command(path)
}
