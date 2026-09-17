//! `kaibo sync`: clone-or-pull the corpus, then refresh its qmd index.
//!
//! Idempotent, and also the first-run bootstrap: if the clone is missing,
//! it is cloned; either way the clone is then checked out to `main` and
//! pulled - with git hooks disabled (`-c core.hooksPath=/dev/null`) on every
//! git command that touches the untrusted repo's working tree, because a
//! knowledge repo must never execute code on this machine. That is a
//! security guarantee, not a nicety - it is enforced on `clone`, `checkout`,
//! and `pull` alike, not only on the command the brief calls out by name.
//!
//! **Stop and report, never discard.** Uncommitted changes in the clone, a
//! blocked checkout, a failed clone/pull, or no repo configured all stop the
//! verb before anything destructive happens - `sync` never runs `git reset
//! --hard`, `git checkout -f`, or deletes the clone. Each stop condition is
//! reported with the exact next command to run.
//!
//! Once the clone is in a known-good state, `sync` ensures the configured
//! qmd collection exists (creating it if missing - qmd itself is not
//! idempotent about this, a second `collection add` with the same name
//! fails, so existence is checked first), then runs `qmd update` and `qmd
//! embed`, both always scoped to the configured index via
//! [`crate::qmd::QmdCommand::index_command`].
//!
//! `--if-stale` short-circuits the whole verb to a no-op when the corpus is
//! already fresh (same staleness threshold and probe as `kaibo status`),
//! so a caller like `query` can ask for a self-heal without paying for a
//! full sync round trip when there is nothing to heal.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::clock::Clock;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::qmd::QmdCommand;
use crate::status::{
    self, ConfigSummary, IndexStatus, STALE_THRESHOLD, commit_age, config_summary,
    git_last_commit_command, parse_commit_epoch, parse_qmd_status, render_count,
};

/// The mask `sync` restricts the `knowledge` collection to: typed content
/// folders only (`reference`, `how-to`, `faq`) one level under each domain
/// folder. Root files like `README.md`, `_index.md`, and `CODEOWNERS` are
/// navigation and metadata, not knowledge, and stay out of the index.
const COLLECTION_MASK: &str = "*/{reference,how-to,faq}/**/*.md";

/// One thing worth telling the user about, with the exact next command
/// where a fix exists. Same shape as `status::Finding`, kept as a separate
/// type so the two verbs' reports do not have to share a crate-visibility
/// boundary just to reuse a two-field struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

/// Why `gather` stopped before finishing, instead of running a command that
/// could discard something. Every variant is reported with a message naming
/// the exact next command - see [`SyncReport::findings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStop {
    /// No repo configured, and the clone does not exist yet - there is
    /// nothing to clone from.
    RepoNotConfigured,
    /// `git clone` failed (network, auth, the repo does not exist, ...).
    CloneFailed { detail: String },
    /// The clone's status could not be read at all (not spawnable, or the
    /// directory is not a valid git repository).
    CloneUnreadable { detail: String },
    /// `git status --porcelain` reported a dirty working tree.
    UncommittedChanges { detail: String },
    /// `git checkout main` failed.
    CheckoutBlocked { detail: String },
    /// `git pull` failed.
    PullFailed { detail: String },
}

impl SyncStop {
    fn exit_code(&self) -> ExitCode {
        match self {
            // Missing config is a usage problem the user can fix; every
            // other stop condition leaves the corpus unsynced.
            SyncStop::RepoNotConfigured => ExitCode::Usage,
            SyncStop::CloneFailed { .. }
            | SyncStop::CloneUnreadable { .. }
            | SyncStop::UncommittedChanges { .. }
            | SyncStop::CheckoutBlocked { .. }
            | SyncStop::PullFailed { .. } => ExitCode::Stale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneOutcome {
    /// The clone did not exist; a fresh clone was made (and then, like
    /// every run, checked out to `main` and pulled).
    Bootstrapped,
    /// The clone already existed and was checked out to `main` and pulled.
    Pulled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionState {
    /// The configured collection already existed; nothing was created.
    AlreadyPresent,
    /// The configured collection did not exist and was created.
    Created,
    /// qmd could not be reached, or refused to list/create the collection.
    Unavailable { detail: String },
}

/// The overall result of one `gather` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// `--if-stale` was set and the corpus was already fresh: nothing ran.
    SkippedFresh,
    /// Stopped before finishing - see [`SyncStop`] for why.
    Stopped(SyncStop),
    /// The clone is in a known-good state and the collection-ensure step
    /// was reached (whether or not it, or the reindex/embed step after it,
    /// actually succeeded - see `collection` and `index`).
    Completed {
        clone: CloneOutcome,
        collection: CollectionState,
        index: IndexStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub config: ConfigSummary,
    pub outcome: SyncOutcome,
}

impl SyncReport {
    /// `Usage` if no repo is configured and there is no clone to fall back
    /// on; `Stale` if a stop condition left the corpus unsynced, or if qmd
    /// could not be reached; `NoHits` if sync completed but indexed no
    /// files (the mask matched nothing - the gap signal); `Success`
    /// otherwise, including the `--if-stale` no-op case.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            SyncOutcome::SkippedFresh => ExitCode::Success,
            SyncOutcome::Stopped(stop) => stop.exit_code(),
            SyncOutcome::Completed {
                collection, index, ..
            } => {
                if matches!(collection, CollectionState::Unavailable { .. }) {
                    return ExitCode::Stale;
                }
                match index {
                    IndexStatus::Unavailable { .. } => ExitCode::Stale,
                    IndexStatus::Available {
                        total_files: Some(0),
                        ..
                    } => ExitCode::NoHits,
                    IndexStatus::Available { .. } => ExitCode::Success,
                }
            }
        }
    }

    /// Findings, each carrying the exact next command where one exists.
    pub fn findings(&self) -> Vec<Finding> {
        let clone_display = self.config.clone.value.display().to_string();
        let mut findings = Vec::new();

        match &self.outcome {
            SyncOutcome::SkippedFresh => {}
            SyncOutcome::Stopped(stop) => findings.push(stop_finding(stop, &clone_display)),
            SyncOutcome::Completed {
                collection, index, ..
            } => {
                if let CollectionState::Unavailable { detail } = collection {
                    findings.push(Finding {
                        message: format!(
                            "could not ensure the '{}' collection exists: {detail}",
                            self.config.collection.value
                        ),
                        fix: Some(
                            "install qmd (https://github.com/tobi/qmd) so it is reachable on \
                             PATH, then re-run `kaibo sync`"
                                .to_string(),
                        ),
                    });
                }
                if let IndexStatus::Unavailable { detail } = index {
                    findings.push(Finding {
                        message: format!("qmd update/embed did not complete: {detail}"),
                        fix: Some("re-run `kaibo sync`".to_string()),
                    });
                }
                if let IndexStatus::Available {
                    total_files: Some(0),
                    ..
                } = index
                {
                    findings.push(Finding {
                        message: "sync completed, but no files matched the collection mask"
                            .to_string(),
                        fix: Some(format!(
                            "check that {clone_display} contains files under {COLLECTION_MASK}"
                        )),
                    });
                }
            }
        }

        findings
    }
}

fn stop_finding(stop: &SyncStop, clone_display: &str) -> Finding {
    match stop {
        SyncStop::RepoNotConfigured => Finding {
            message: "no corpus repo configured".to_string(),
            fix: Some(
                "set KAIBO_REPO=<owner>/<name> (or `repo` in ~/.kaibo/config.toml), then \
                 re-run `kaibo sync`"
                    .to_string(),
            ),
        },
        SyncStop::CloneFailed { detail } => Finding {
            message: format!("clone failed: {detail}"),
            fix: Some(
                "check network access and that the configured repo exists and is reachable, \
                 then re-run `kaibo sync`"
                    .to_string(),
            ),
        },
        SyncStop::CloneUnreadable { detail } => Finding {
            message: format!("could not read the status of {clone_display}: {detail}"),
            fix: Some(format!(
                "inspect {clone_display} by hand (e.g. `git -C {clone_display} status`), then \
                 re-run `kaibo sync`"
            )),
        },
        SyncStop::UncommittedChanges { detail } => Finding {
            message: format!("uncommitted changes in {clone_display}: {detail}"),
            fix: Some(format!(
                "commit or stash your changes in {clone_display}, then re-run `kaibo sync`"
            )),
        },
        SyncStop::CheckoutBlocked { detail } => Finding {
            message: format!("checkout of main was blocked in {clone_display}: {detail}"),
            fix: Some(format!(
                "resolve the checkout conflict in {clone_display} by hand, then re-run \
                 `kaibo sync`"
            )),
        },
        SyncStop::PullFailed { detail } => Finding {
            message: format!("pull failed in {clone_display}: {detail}"),
            fix: Some(format!(
                "resolve the pull failure in {clone_display} by hand (check network and auth), \
                 then re-run `kaibo sync`"
            )),
        },
    }
}

impl Render for SyncReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push("kaibo sync".to_string());

        lines.push(format!(
            "config: index={} ({}), collection={} ({}), clone={} ({}), repo={} ({})",
            self.config.index.value,
            self.config.index.source,
            self.config.collection.value,
            self.config.collection.source,
            self.config.clone.value.display(),
            self.config.clone.source,
            self.config.repo.value.as_deref().unwrap_or("unset"),
            self.config.repo.source,
        ));

        match &self.outcome {
            SyncOutcome::SkippedFresh => {
                lines.push("clone: skipped, corpus already fresh (--if-stale)".to_string());
            }
            SyncOutcome::Stopped(_) => {
                lines.push("clone: stopped".to_string());
            }
            SyncOutcome::Completed {
                clone,
                collection,
                index,
            } => {
                lines.push(format!(
                    "clone: {}",
                    match clone {
                        CloneOutcome::Bootstrapped => "bootstrapped (cloned fresh)",
                        CloneOutcome::Pulled => "pulled",
                    }
                ));
                lines.push(format!(
                    "collection: {}",
                    match collection {
                        CollectionState::AlreadyPresent => "already present".to_string(),
                        CollectionState::Created => "created".to_string(),
                        CollectionState::Unavailable { detail } =>
                            format!("unavailable ({detail})"),
                    }
                ));
                lines.push(match index {
                    IndexStatus::Unavailable { detail } => format!("index: unavailable ({detail})"),
                    IndexStatus::Available {
                        total_files,
                        vectors_embedded,
                        pending,
                    } => format!(
                        "index: {} docs, {} embedded, {} pending",
                        render_count(*total_files),
                        render_count(*vectors_embedded),
                        render_count(*pending),
                    ),
                });
            }
        }

        let findings = self.findings();
        if findings.is_empty() {
            lines.push("result: healthy".to_string());
        } else {
            lines.push("result: problem found".to_string());
            for finding in &findings {
                match &finding.fix {
                    Some(fix) => lines.push(format!("  - {} -> next: `{fix}`", finding.message)),
                    None => lines.push(format!("  - {}", finding.message)),
                }
            }
        }

        if options.full {
            lines.push(format!("exit code: {}", self.exit_code().code()));
        }

        lines.join("\n")
    }

    fn render_json(&self) -> Value {
        let outcome = match &self.outcome {
            SyncOutcome::SkippedFresh => serde_json::json!({"state": "skipped_fresh"}),
            SyncOutcome::Stopped(stop) => serde_json::json!({
                "state": "stopped",
                "reason": stop_code(stop),
            }),
            SyncOutcome::Completed {
                clone,
                collection,
                index,
            } => serde_json::json!({
                "state": "completed",
                "clone": match clone {
                    CloneOutcome::Bootstrapped => "bootstrapped",
                    CloneOutcome::Pulled => "pulled",
                },
                "collection": match collection {
                    CollectionState::AlreadyPresent => serde_json::json!({"state": "already_present"}),
                    CollectionState::Created => serde_json::json!({"state": "created"}),
                    CollectionState::Unavailable { detail } => serde_json::json!({"state": "unavailable", "detail": detail}),
                },
                "index": match index {
                    IndexStatus::Unavailable { detail } => serde_json::json!({"available": false, "detail": detail}),
                    IndexStatus::Available { total_files, vectors_embedded, pending } => serde_json::json!({
                        "available": true,
                        "total_files": total_files,
                        "vectors_embedded": vectors_embedded,
                        "pending": pending,
                    }),
                },
            }),
        };

        serde_json::json!({
            "config": {
                "repo": {"value": self.config.repo.value, "source": self.config.repo.source.to_string()},
                "clone": {"value": self.config.clone.value.display().to_string(), "source": self.config.clone.source.to_string()},
                "index": {"value": self.config.index.value, "source": self.config.index.source.to_string()},
                "collection": {"value": self.config.collection.value, "source": self.config.collection.source.to_string()},
            },
            "outcome": outcome,
            "findings": self.findings().iter().map(|f| serde_json::json!({"message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
            "exit_code": self.exit_code().code(),
        })
    }
}

fn stop_code(stop: &SyncStop) -> &'static str {
    match stop {
        SyncStop::RepoNotConfigured => "repo_not_configured",
        SyncStop::CloneFailed { .. } => "clone_failed",
        SyncStop::CloneUnreadable { .. } => "clone_unreadable",
        SyncStop::UncommittedChanges { .. } => "uncommitted_changes",
        SyncStop::CheckoutBlocked { .. } => "checkout_blocked",
        SyncStop::PullFailed { .. } => "pull_failed",
    }
}

/// The `kaibo sync` verb, bound to a resolved `Config`.
pub struct SyncVerb<'a> {
    config: &'a Config,
}

impl<'a> SyncVerb<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Run sync for real. `if_stale` makes the whole call a no-op unless
    /// the corpus is currently stale (same threshold and probe as `kaibo
    /// status`).
    pub fn gather(
        &self,
        runner: &dyn CommandRunner,
        clock: &dyn Clock,
        if_stale: bool,
    ) -> SyncReport {
        gather(self.config, runner, clock, if_stale)
    }
}

impl Explainable for SyncVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        let clone_present = clone_git_dir(self.config).is_dir();
        planned_commands(self.config, clone_present)
    }
}

fn clone_git_dir(config: &Config) -> PathBuf {
    config.clone_path().join(".git")
}

/// The commands `gather` would run for the given clone state, ignoring
/// `--if-stale` - explain never has a runner or clock to determine
/// freshness without shelling out, so it always describes the full
/// pipeline, the same simplification `status::planned_commands` makes for
/// qmd's reachability. The one case explain refuses to describe is the one
/// `gather` refuses to run anything for at all: no clone and no repo
/// configured.
fn planned_commands(config: &Config, clone_present: bool) -> Vec<PlannedCommand> {
    if !clone_present && config.repo().is_none() {
        return Vec::new();
    }

    let mut commands = Vec::new();
    if !clone_present {
        let repo = config
            .repo()
            .expect("checked above: repo is Some when clone is absent");
        commands.push(git_clone_command(repo, config.clone_path()));
    }
    commands.push(git_status_porcelain_command(config.clone_path()));
    commands.push(git_checkout_main_command(config.clone_path()));
    commands.push(git_pull_command(config.clone_path()));
    commands.push(QmdCommand::collection_list(config));
    commands.push(QmdCommand::collection_add(
        config,
        config.clone_path(),
        config.collection(),
        COLLECTION_MASK,
    ));
    commands.push(QmdCommand::update(config));
    commands.push(QmdCommand::embed(config));
    commands
}

fn gather(
    config: &Config,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
    if_stale: bool,
) -> SyncReport {
    let config_summary = config_summary(config);
    let clone_present = clone_git_dir(config).is_dir();

    if if_stale && is_fresh(config, runner, clock, clone_present) {
        return SyncReport {
            config: config_summary,
            outcome: SyncOutcome::SkippedFresh,
        };
    }

    let clone = match ensure_clone_and_pull(config, runner, clone_present) {
        Ok(outcome) => outcome,
        Err(stop) => {
            return SyncReport {
                config: config_summary,
                outcome: SyncOutcome::Stopped(stop),
            };
        }
    };

    let collection = match ensure_collection(config, runner) {
        Ok(state) => state,
        Err(detail) => {
            return SyncReport {
                config: config_summary,
                outcome: SyncOutcome::Completed {
                    clone,
                    collection: CollectionState::Unavailable {
                        detail: detail.clone(),
                    },
                    index: IndexStatus::Unavailable { detail },
                },
            };
        }
    };

    let index = update_and_embed(config, runner);

    SyncReport {
        config: config_summary,
        outcome: SyncOutcome::Completed {
            clone,
            collection,
            index,
        },
    }
}

/// Same staleness question `kaibo status` answers (clone absent, or its
/// last commit older than [`STALE_THRESHOLD`], or unreadable, all count as
/// stale), reusing its exact command and parsing so the two verbs cannot
/// disagree about what "fresh" means.
fn is_fresh(
    config: &Config,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
    clone_present: bool,
) -> bool {
    if !clone_present {
        return false;
    }

    let last_commit_age = match runner.run(&git_last_commit_command(config.clone_path())) {
        Ok(output) if output.success() => {
            parse_commit_epoch(&output.stdout).and_then(|epoch| commit_age(epoch, clock.now()))
        }
        _ => None,
    };

    match last_commit_age {
        Some(age) => age <= STALE_THRESHOLD,
        // Unreadable is treated as stale, same as `status::StatusReport`
        // does - `--if-stale` must not silently skip real work just
        // because freshness could not be confirmed.
        None => false,
    }
}

fn ensure_clone_and_pull(
    config: &Config,
    runner: &dyn CommandRunner,
    clone_present: bool,
) -> Result<CloneOutcome, SyncStop> {
    let bootstrapped = if !clone_present {
        let repo = config.repo().ok_or(SyncStop::RepoNotConfigured)?;
        let output = runner
            .run(&git_clone_command(repo, config.clone_path()))
            .map_err(|err| SyncStop::CloneFailed {
                detail: err.to_string(),
            })?;
        if !output.success() {
            return Err(SyncStop::CloneFailed {
                detail: output.stderr.trim().to_string(),
            });
        }
        true
    } else {
        false
    };

    let status_output = runner
        .run(&git_status_porcelain_command(config.clone_path()))
        .map_err(|err| SyncStop::CloneUnreadable {
            detail: err.to_string(),
        })?;
    if !status_output.success() {
        return Err(SyncStop::CloneUnreadable {
            detail: status_output.stderr.trim().to_string(),
        });
    }
    if !status_output.stdout.trim().is_empty() {
        return Err(SyncStop::UncommittedChanges {
            detail: status_output.stdout.trim().to_string(),
        });
    }

    let checkout_output = runner
        .run(&git_checkout_main_command(config.clone_path()))
        .map_err(|err| SyncStop::CheckoutBlocked {
            detail: err.to_string(),
        })?;
    if !checkout_output.success() {
        return Err(SyncStop::CheckoutBlocked {
            detail: checkout_output.stderr.trim().to_string(),
        });
    }

    let pull_output = runner
        .run(&git_pull_command(config.clone_path()))
        .map_err(|err| SyncStop::PullFailed {
            detail: err.to_string(),
        })?;
    if !pull_output.success() {
        return Err(SyncStop::PullFailed {
            detail: pull_output.stderr.trim().to_string(),
        });
    }

    Ok(if bootstrapped {
        CloneOutcome::Bootstrapped
    } else {
        CloneOutcome::Pulled
    })
}

/// qmd's `collection add` is not idempotent - a second call with a name
/// that already exists fails - so existence is checked with `collection
/// list` first, and `add` is only run when the name is missing from it.
fn ensure_collection(
    config: &Config,
    runner: &dyn CommandRunner,
) -> Result<CollectionState, String> {
    let list_output = runner
        .run(&QmdCommand::collection_list(config))
        .map_err(|err| err.to_string())?;
    if !list_output.success() {
        return Err(list_output.stderr.trim().to_string());
    }
    if status::collection_listed(&list_output.stdout, config.collection()) {
        return Ok(CollectionState::AlreadyPresent);
    }

    let add_output = runner
        .run(&QmdCommand::collection_add(
            config,
            config.clone_path(),
            config.collection(),
            COLLECTION_MASK,
        ))
        .map_err(|err| err.to_string())?;
    if !add_output.success() {
        return Err(add_output.stderr.trim().to_string());
    }

    Ok(CollectionState::Created)
}

fn update_and_embed(config: &Config, runner: &dyn CommandRunner) -> IndexStatus {
    match runner.run(&QmdCommand::update(config)) {
        Ok(output) if output.success() => {}
        Ok(output) => {
            return IndexStatus::Unavailable {
                detail: output.stderr.trim().to_string(),
            };
        }
        Err(err) => {
            return IndexStatus::Unavailable {
                detail: err.to_string(),
            };
        }
    }

    match runner.run(&QmdCommand::embed(config)) {
        Ok(output) if output.success() => {}
        Ok(output) => {
            return IndexStatus::Unavailable {
                detail: output.stderr.trim().to_string(),
            };
        }
        Err(err) => {
            return IndexStatus::Unavailable {
                detail: err.to_string(),
            };
        }
    }

    match runner.run(&QmdCommand::status(config)) {
        Ok(output) if output.success() => parse_qmd_status(&output.stdout),
        Ok(output) => IndexStatus::Unavailable {
            detail: output.stderr.trim().to_string(),
        },
        Err(err) => IndexStatus::Unavailable {
            detail: err.to_string(),
        },
    }
}

fn git_clone_command(repo: &str, clone_path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "clone".to_string(),
            format!("https://github.com/{repo}.git"),
            clone_path.to_string_lossy().into_owned(),
        ],
    )
}

fn git_status_porcelain_command(path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "status".to_string(),
            "--porcelain".to_string(),
        ],
    )
}

fn git_checkout_main_command(path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "checkout".to_string(),
            "main".to_string(),
        ],
    )
}

fn git_pull_command(path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "pull".to_string(),
        ],
    )
}

#[cfg(test)]
mod tests;
