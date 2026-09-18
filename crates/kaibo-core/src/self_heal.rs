//! The self-heal trigger check shared by every verb that reads the local
//! clone directly and wants `query`'s "missing clone, stale clone, or
//! missing qmd collection triggers an unconditional `kaibo sync`" behaviour
//! without re-deciding it from scratch, or reimplementing any of the git/qmd
//! polling `sync` itself owns.
//!
//! `query::needs_self_heal` is not reused directly - it is private to that
//! module - so this recomposes the identical check from the same shared
//! primitives (`status::git_last_commit_command`, `status::parse_commit_epoch`,
//! `status::commit_age`, `status::STALE_THRESHOLD`,
//! `qmd::QmdCommand::collection_list`, `status::collection_listed`) that
//! `query`, `status` and `sync` already share, so `doctrine` and `domains`
//! cannot drift from what `query` already does.
//!
//! Deliberately not `sync --if-stale`: a fresh clone whose qmd collection is
//! missing would read as "fresh" to `sync`'s own staleness probe, which
//! looks only at commit age - see `query`'s own module doc for the full
//! reasoning, which applies unchanged here.

use std::path::PathBuf;

use crate::clock::Clock;
use crate::config::Config;
use crate::process::CommandRunner;
use crate::qmd::QmdCommand;
use crate::status;

pub(crate) fn clone_git_dir(config: &Config) -> PathBuf {
    config.clone_path().join(".git")
}

/// Whether a self-heal (`kaibo sync`) should run before a verb reads the
/// local clone for real: the clone is missing, its last commit is older
/// than `status::STALE_THRESHOLD` (or unreadable), or the configured
/// collection is not listed in the configured index (or qmd could not be
/// reached to check).
pub(crate) fn needs_self_heal(
    config: &Config,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
) -> bool {
    if !clone_git_dir(config).is_dir() {
        return true;
    }

    let stale = match runner.run(&status::git_last_commit_command(config.clone_path())) {
        Ok(output) if output.success() => {
            match status::parse_commit_epoch(&output.stdout)
                .and_then(|epoch| status::commit_age(epoch, clock.now()))
            {
                Some(age) => age > status::STALE_THRESHOLD,
                None => true,
            }
        }
        _ => true,
    };
    if stale {
        return true;
    }

    match runner.run(&QmdCommand::collection_list(config)) {
        Ok(output) if output.success() => {
            !status::collection_listed(&output.stdout, config.collection())
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::clock::testing::FixedClock;
    use crate::config::ConfigSource;
    use crate::config::testing::ConfigBuilder;
    use crate::process::testing::{FakeCommandRunner, failed_with_stdout, ok};

    const NOW_EPOCH: u64 = 1_700_000_000;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
    }

    fn git_dir(clone: &std::path::Path) {
        std::fs::create_dir_all(clone.join(".git")).unwrap();
    }

    fn config_with_repo(clone: &std::path::Path) -> Config {
        ConfigBuilder::new(clone)
            .repo("org/corpus", ConfigSource::File)
            .build()
    }

    #[test]
    fn a_missing_clone_needs_self_heal_without_any_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        let config = config_with_repo(&clone);
        let runner = FakeCommandRunner::new();
        let clock = FixedClock(now());

        assert!(needs_self_heal(&config, &runner, &clock));
    }

    #[test]
    fn a_fresh_clone_with_its_collection_listed_does_not_need_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let runner = FakeCommandRunner::new()
            .on(
                status::git_last_commit_command(&clone),
                ok(format!("{}\n", NOW_EPOCH - 60)),
            )
            .on(
                QmdCommand::collection_list(&config),
                ok(format!("{}\n", config.collection())),
            );
        let clock = FixedClock(now());

        assert!(!needs_self_heal(&config, &runner, &clock));
    }

    #[test]
    fn a_fresh_clone_with_its_collection_missing_still_needs_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let runner = FakeCommandRunner::new()
            .on(
                status::git_last_commit_command(&clone),
                ok(format!("{}\n", NOW_EPOCH - 60)),
            )
            .on(
                QmdCommand::collection_list(&config),
                ok("No collections found.\n".to_string()),
            );
        let clock = FixedClock(now());

        assert!(needs_self_heal(&config, &runner, &clock));
    }

    /// A commit exactly `STALE_THRESHOLD` old is not yet stale - only
    /// strictly older triggers self-heal. Pins the boundary at `>`, not
    /// `>=` or `==`: either of those would treat "exactly at the threshold"
    /// as needing a heal, which is not what the threshold means.
    #[test]
    fn a_commit_exactly_at_the_stale_threshold_does_not_need_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let at_threshold_epoch = NOW_EPOCH - status::STALE_THRESHOLD.as_secs();
        let runner = FakeCommandRunner::new()
            .on(
                status::git_last_commit_command(&clone),
                ok(format!("{at_threshold_epoch}\n")),
            )
            .on(
                QmdCommand::collection_list(&config),
                ok(format!("{}\n", config.collection())),
            );
        let clock = FixedClock(now());

        assert!(!needs_self_heal(&config, &runner, &clock));
    }

    /// One second older than [`a_commit_exactly_at_the_stale_threshold_does_not_need_self_heal`]'s
    /// boundary, everything else held equal: this is what actually needing
    /// a heal on account of age looks like.
    #[test]
    fn a_commit_one_second_past_the_stale_threshold_needs_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let past_threshold_epoch = NOW_EPOCH - status::STALE_THRESHOLD.as_secs() - 1;
        let runner = FakeCommandRunner::new().on(
            status::git_last_commit_command(&clone),
            ok(format!("{past_threshold_epoch}\n")),
        );
        let clock = FixedClock(now());

        assert!(needs_self_heal(&config, &runner, &clock));
    }

    /// `git log` running but exiting non-zero (a real repo in a broken
    /// state, not merely "not on PATH") must read the same as any other
    /// unreadable freshness probe: needs a heal, not a guess at "fresh".
    ///
    /// `stdout` here is deliberately a value that *would* parse as a fresh
    /// commit if the success check were skipped, and `collection_list` is
    /// scripted as already listed - so if a mutant bypassed the success
    /// check, this would read as healthy and the assertion below would
    /// fail instead of merely tripping over an unrelated missing-response
    /// panic.
    #[test]
    fn a_failing_last_commit_probe_needs_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let runner = FakeCommandRunner::new()
            .on(
                status::git_last_commit_command(&clone),
                failed_with_stdout(format!("{}\n", NOW_EPOCH - 60)),
            )
            .on(
                QmdCommand::collection_list(&config),
                ok(format!("{}\n", config.collection())),
            );
        let clock = FixedClock(now());

        assert!(needs_self_heal(&config, &runner, &clock));
    }

    /// `qmd collection list` running but exiting non-zero must read the
    /// same as qmd being unreachable entirely: needs a heal, since this
    /// crate has no basis to trust a failed command's stdout enough to
    /// check whether the collection is listed in it.
    ///
    /// `stdout` here is deliberately a value that *would* read as "listed"
    /// if the success check were skipped, so a mutant that drops the check
    /// produces a different, catchable answer.
    #[test]
    fn a_failing_collection_list_probe_needs_self_heal() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let runner = FakeCommandRunner::new()
            .on(
                status::git_last_commit_command(&clone),
                ok(format!("{}\n", NOW_EPOCH - 60)),
            )
            .on(
                QmdCommand::collection_list(&config),
                failed_with_stdout(format!("{}\n", config.collection())),
            );
        let clock = FixedClock(now());

        assert!(needs_self_heal(&config, &runner, &clock));
    }
}
