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
    use crate::process::testing::{FakeCommandRunner, ok};

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
}
