//! `kaibo status`: the single "is this healthy" verb.
//!
//! Read-only by design - it never clones, pulls, indexes, embeds, or writes
//! anything. It reports what it can, and says plainly when a fact isn't
//! available (qmd not on `PATH`, a clone that isn't there yet) instead of
//! guessing. A tool being unreachable from a non-interactive shell is an
//! everyday, expected state for this verb to diagnose, not a reason to
//! abort before checking everything else.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::clock::Clock;
use crate::config::{Config, ConfigKey, ConfigSource};
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::qmd::QmdCommand;

/// The qmd version this crate is verified against (see the qmd contract
/// notes this project follows). A different version is reported as a
/// finding, not treated as a failure - qmd itself has no stable compatibility
/// promise across versions, so drift is worth surfacing, not worth failing a
/// build over.
pub const PINNED_QMD_VERSION: &str = "2.8.3";

/// A clone whose last commit is older than this is reported as stale. No
/// doctrine pins this number yet; revisit once `kaibo sync` has real-world
/// sync cadence to calibrate against.
pub(crate) const STALE_THRESHOLD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A resolved config value paired with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValue<T> {
    pub value: T,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSummary {
    pub repo: ConfigValue<Option<String>>,
    pub clone: ConfigValue<PathBuf>,
    pub index: ConfigValue<String>,
    pub collection: ConfigValue<String>,
    pub api_url: ConfigValue<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneStatus {
    Absent,
    Present {
        /// `None` if the current branch could not be read.
        branch: Option<String>,
        /// Age of the last commit relative to the clock `gather` ran with.
        /// `None` if it could not be read - honest gap, not a guess.
        last_commit_age: Option<Duration>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexStatus {
    /// qmd is missing, or the status call failed; `detail` says why.
    Unavailable { detail: String },
    Available {
        total_files: Option<u64>,
        vectors_embedded: Option<u64>,
        pending: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QmdStatus {
    NotFound,
    Found { version: String, matches_pin: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationStatus {
    /// The structural guarantee holds (see `crate::qmd`), and the empirical
    /// probe corroborated it: the configured collection is absent from
    /// qmd's default index.
    Verified,
    /// The structural guarantee always holds; the empirical probe could not
    /// run because qmd is unavailable.
    UnverifiedQmdUnavailable,
    /// The structural guarantee holds - kaibo cannot have put this there -
    /// but the configured collection name is already present in qmd's
    /// default index. Not something kaibo did, but worth telling the user.
    NameCollisionInDefaultIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMode {
    Local,
    Api(String),
}

/// One thing worth telling the user about, with the exact next command
/// where a fix exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub cli_version: String,
    pub config: ConfigSummary,
    pub backend: BackendMode,
    pub clone: CloneStatus,
    pub qmd: QmdStatus,
    pub index: IndexStatus,
    pub isolation: IsolationStatus,
}

impl StatusReport {
    /// `Stale` when the clone is missing or its freshness can't be
    /// confirmed; `Success` otherwise. qmd being absent or off-pin is
    /// reported as a finding, not a reason to fail the exit code - see the
    /// module doc.
    pub fn exit_code(&self) -> ExitCode {
        if self.is_stale() {
            ExitCode::Stale
        } else {
            ExitCode::Success
        }
    }

    fn is_stale(&self) -> bool {
        match &self.clone {
            CloneStatus::Absent => true,
            CloneStatus::Present {
                last_commit_age, ..
            } => match last_commit_age {
                Some(age) => *age > STALE_THRESHOLD,
                // Can't confirm freshness: report as stale rather than
                // assuming the best.
                None => true,
            },
        }
    }

    /// Findings, each carrying the exact next command where one exists.
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();

        match &self.clone {
            CloneStatus::Absent => findings.push(Finding {
                message: format!("no clone at {}", self.config.clone.value.display()),
                fix: Some("kaibo sync".to_string()),
            }),
            CloneStatus::Present { branch: None, .. } => findings.push(Finding {
                message: "clone exists but its current branch could not be read".to_string(),
                fix: Some("kaibo sync".to_string()),
            }),
            CloneStatus::Present {
                last_commit_age: None,
                ..
            } => findings.push(Finding {
                message: "clone exists but its last commit time could not be read".to_string(),
                fix: Some("kaibo sync".to_string()),
            }),
            CloneStatus::Present {
                last_commit_age: Some(age),
                ..
            } if *age > STALE_THRESHOLD => findings.push(Finding {
                message: format!(
                    "clone is stale: last commit was {}",
                    render_relative_age(*age)
                ),
                fix: Some("kaibo sync".to_string()),
            }),
            CloneStatus::Present { .. } => {}
        }

        if self.qmd == QmdStatus::NotFound {
            findings.push(Finding {
                message: "qmd not found on PATH".to_string(),
                fix: Some(
                    "install qmd (https://github.com/tobi/qmd) so it is reachable on \
                     PATH, then re-run `kaibo status`"
                        .to_string(),
                ),
            });
        }
        if let QmdStatus::Found {
            matches_pin: false,
            version,
        } = &self.qmd
        {
            findings.push(Finding {
                message: format!(
                    "qmd {version} does not match the pinned version {PINNED_QMD_VERSION}"
                ),
                fix: None,
            });
        }
        if self.isolation == IsolationStatus::NameCollisionInDefaultIndex {
            findings.push(Finding {
                message: format!(
                    "collection '{}' is also present in qmd's default index; kaibo did \
                     not put it there, but it's worth checking what did",
                    self.config.collection.value
                ),
                fix: None,
            });
        }

        findings
    }
}

/// The `kaibo status` verb, bound to a resolved `Config`.
pub struct StatusVerb<'a> {
    config: &'a Config,
}

impl<'a> StatusVerb<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Run the checks for real and produce a report. `cli_version` is the
    /// `kaibo` binary's own version - known only to the binary crate, so it
    /// is threaded in here rather than read from this crate's own metadata.
    pub fn gather(
        &self,
        runner: &dyn CommandRunner,
        clock: &dyn Clock,
        cli_version: &str,
    ) -> StatusReport {
        gather(self.config, runner, clock, cli_version)
    }
}

impl Explainable for StatusVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        let clone_present = clone_git_dir(self.config).is_dir();
        planned_commands(self.config, clone_present)
    }
}

fn clone_git_dir(config: &Config) -> PathBuf {
    config.clone_path().join(".git")
}

/// The commands `gather` would run for the given clone state. Shared by
/// `gather` (which runs them) and `explain` (which only lists them), so the
/// two cannot drift apart.
fn planned_commands(config: &Config, clone_present: bool) -> Vec<PlannedCommand> {
    let mut commands = Vec::new();
    if clone_present {
        commands.push(git_branch_command(config.clone_path()));
        commands.push(git_last_commit_command(config.clone_path()));
    }
    commands.push(QmdCommand::version());
    commands.push(QmdCommand::status(config));
    commands.push(QmdCommand::default_index_collection_list());
    commands
}

/// Build the [`ConfigSummary`] every verb's report opens with: every
/// resolved config value paired with where it came from. Shared by `status`
/// and `sync` so the two cannot drift into reporting config differently.
pub(crate) fn config_summary(config: &Config) -> ConfigSummary {
    ConfigSummary {
        repo: ConfigValue {
            value: config.repo().map(str::to_string),
            source: config.source(ConfigKey::Repo),
        },
        clone: ConfigValue {
            value: config.clone_path().to_path_buf(),
            source: config.source(ConfigKey::Clone),
        },
        index: ConfigValue {
            value: config.index().to_string(),
            source: config.source(ConfigKey::Index),
        },
        collection: ConfigValue {
            value: config.collection().to_string(),
            source: config.source(ConfigKey::Collection),
        },
        api_url: ConfigValue {
            value: config.api_url().map(str::to_string),
            source: config.source(ConfigKey::ApiUrl),
        },
    }
}

fn gather(
    config: &Config,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
    cli_version: &str,
) -> StatusReport {
    let config_summary = config_summary(config);

    let backend = match config.api_url() {
        Some(url) => BackendMode::Api(url.to_string()),
        None => BackendMode::Local,
    };

    let clone_present = clone_git_dir(config).is_dir();
    let clone = if clone_present {
        gather_clone_status(config, runner, clock)
    } else {
        CloneStatus::Absent
    };

    let qmd = match runner.run(&QmdCommand::version()) {
        Ok(output) if output.success() => {
            let version = parse_qmd_version(&output.stdout)
                .unwrap_or_else(|| output.stdout.trim().to_string());
            QmdStatus::Found {
                matches_pin: version == PINNED_QMD_VERSION,
                version,
            }
        }
        // Not on PATH, or ran but reported failure either way: qmd is not
        // usable. Don't attempt the calls below - they'd only fail the same
        // way - and still return everything gathered so far.
        _ => QmdStatus::NotFound,
    };

    let index = match &qmd {
        QmdStatus::NotFound => IndexStatus::Unavailable {
            detail: "qmd not found on PATH".to_string(),
        },
        QmdStatus::Found { .. } => match runner.run(&QmdCommand::status(config)) {
            Ok(output) if output.success() => parse_qmd_status(&output.stdout),
            Ok(output) => IndexStatus::Unavailable {
                detail: output.stderr.trim().to_string(),
            },
            Err(err) => IndexStatus::Unavailable {
                detail: err.to_string(),
            },
        },
    };

    let isolation = match &qmd {
        QmdStatus::NotFound => IsolationStatus::UnverifiedQmdUnavailable,
        QmdStatus::Found { .. } => {
            match runner.run(&QmdCommand::default_index_collection_list()) {
                Ok(output) if output.success() => {
                    if collection_listed(&output.stdout, config.collection()) {
                        IsolationStatus::NameCollisionInDefaultIndex
                    } else {
                        IsolationStatus::Verified
                    }
                }
                // Couldn't read the default index at all; the structural
                // guarantee holds regardless, but nothing was corroborated.
                _ => IsolationStatus::UnverifiedQmdUnavailable,
            }
        }
    };

    StatusReport {
        cli_version: cli_version.to_string(),
        config: config_summary,
        backend,
        clone,
        qmd,
        index,
        isolation,
    }
}

fn gather_clone_status(
    config: &Config,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
) -> CloneStatus {
    let branch = match runner.run(&git_branch_command(config.clone_path())) {
        Ok(output) if output.success() => Some(output.stdout.trim().to_string()),
        _ => None,
    };

    let last_commit_age = match runner.run(&git_last_commit_command(config.clone_path())) {
        Ok(output) if output.success() => {
            parse_commit_epoch(&output.stdout).and_then(|epoch| commit_age(epoch, clock.now()))
        }
        _ => None,
    };

    CloneStatus::Present {
        branch,
        last_commit_age,
    }
}

pub(crate) fn git_branch_command(path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "rev-parse".to_string(),
            "--abbrev-ref".to_string(),
            "HEAD".to_string(),
        ],
    )
}

pub(crate) fn git_last_commit_command(path: &Path) -> PlannedCommand {
    PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "log".to_string(),
            "-1".to_string(),
            "--format=%ct".to_string(),
        ],
    )
}

pub(crate) fn parse_commit_epoch(stdout: &str) -> Option<u64> {
    stdout.trim().parse().ok()
}

pub(crate) fn commit_age(epoch_secs: u64, now: SystemTime) -> Option<Duration> {
    let commit_time = UNIX_EPOCH + Duration::from_secs(epoch_secs);
    now.duration_since(commit_time).ok()
}

/// Render a [`Duration`] as a short, human relative age ("3 days ago").
pub fn render_relative_age(duration: Duration) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let secs = duration.as_secs();
    if secs < MINUTE {
        "just now".to_string()
    } else if secs < HOUR {
        plural_ago(secs / MINUTE, "minute")
    } else if secs < DAY {
        plural_ago(secs / HOUR, "hour")
    } else {
        plural_ago(secs / DAY, "day")
    }
}

fn plural_ago(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

/// qmd's version output isn't documented as a stable format; scan for the
/// first whitespace-separated token that looks like `N.N.N` rather than
/// assuming a fixed position.
fn parse_qmd_version(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .find(|token| is_dotted_version(token))
        .map(str::to_string)
}

fn is_dotted_version(token: &str) -> bool {
    let mut parts = token.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), Some(c), None)
            if is_ascii_digits(a) && is_ascii_digits(b) && is_ascii_digits(c)
    )
}

fn is_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Lenient line-based parse of `qmd status --index <x>` text output. Any
/// field that doesn't parse is `None` rather than guessed - the format isn't
/// a stable contract, so degrading a single unreadable field beats failing
/// the whole report.
///
/// Verified empirically against a real qmd 2.8.3 binary (no contract doc
/// ships in this repo to drift against): `Total:` and `Vectors:` are always
/// present, but `Pending:` ("N need embedding") is an *optional* line that
/// qmd omits entirely once nothing needs embedding - it does not print
/// `Pending: 0`. So a `Pending:` line's absence on an otherwise-successful
/// call means zero, not "unreadable"; `pending` therefore resolves to `Some(0)`
/// rather than `None` - but only when `Total:` parsed, which is what
/// distinguishes qmd status output from output this parser does not
/// recognise at all.
///
/// qmd also prints an unrelated `Orphaned: N embedding chunks (…%) - run
/// 'qmd cleanup'` line when stale vector chunks exist (from since-deleted or
/// -changed documents). That is a distinct concept - "needs cleanup", not
/// "needs embedding" - so it is deliberately not folded into `pending` here;
/// this parser ignores unrecognized lines, `Orphaned:` included.
pub(crate) fn parse_qmd_status(stdout: &str) -> IndexStatus {
    let mut total_files = None;
    let mut vectors_embedded = None;
    let mut pending = None;
    let mut saw_pending_line = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Total:") {
            total_files = first_integer(rest);
        } else if let Some(rest) = trimmed.strip_prefix("Vectors:") {
            vectors_embedded = first_integer(rest);
        } else if let Some(rest) = trimmed.strip_prefix("Pending:") {
            saw_pending_line = true;
            pending = first_integer(rest);
        }
    }

    // An absent `Pending:` line means zero only when this *was* qmd status
    // output. `Total:` is the marker for that: qmd always prints it, so
    // parsing it is what separates "qmd said nothing is pending" from "this
    // is not output we recognise". Without that guard, a future qmd whose
    // format changed wholesale would render as `? total, ? embedded, 0
    // pending` - two honest unknowns beside one confident falsehood, which
    // is the guess this function's contract promises not to make.
    if !saw_pending_line && total_files.is_some() {
        pending = Some(0);
    }

    IndexStatus::Available {
        total_files,
        vectors_embedded,
        pending,
    }
}

fn first_integer(s: &str) -> Option<u64> {
    s.split_whitespace().next()?.parse().ok()
}

/// Whether `collection` appears as a listed collection's name in `qmd
/// collection list` output - matched as the first whitespace-separated
/// token on a line, not a substring, so `knowledge-extra` doesn't false-
/// positive against `knowledge`.
pub(crate) fn collection_listed(stdout: &str, collection: &str) -> bool {
    stdout
        .lines()
        .any(|line| line.split_whitespace().next() == Some(collection))
}

impl Render for StatusReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push(format!("kaibo status ({})", self.cli_version));

        lines.push(format!(
            "config: index={} ({}), collection={} ({}), clone={} ({}), repo={} ({}), backend={}",
            self.config.index.value,
            self.config.index.source,
            self.config.collection.value,
            self.config.collection.source,
            self.config.clone.value.display(),
            self.config.clone.source,
            self.config.repo.value.as_deref().unwrap_or("unset"),
            self.config.repo.source,
            render_backend(&self.backend),
        ));

        lines.push(match &self.clone {
            CloneStatus::Absent => "clone: absent".to_string(),
            CloneStatus::Present {
                branch,
                last_commit_age,
            } => format!(
                "clone: present, branch {}, last commit {}",
                branch.as_deref().unwrap_or("unknown"),
                last_commit_age
                    .map(render_relative_age)
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
        });

        lines.push(match &self.qmd {
            QmdStatus::NotFound => "qmd: not found on PATH".to_string(),
            QmdStatus::Found {
                version,
                matches_pin,
            } => format!(
                "qmd: {version} on PATH ({})",
                if *matches_pin {
                    format!("matches pin {PINNED_QMD_VERSION}")
                } else {
                    format!("pin is {PINNED_QMD_VERSION}")
                }
            ),
        });

        lines.push(match &self.index {
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

        lines.push(format!(
            "isolation: {}",
            match self.isolation {
                IsolationStatus::Verified =>
                    "guaranteed by construction; default index verified untouched",
                IsolationStatus::UnverifiedQmdUnavailable =>
                    "guaranteed by construction; could not verify empirically (qmd unavailable)",
                IsolationStatus::NameCollisionInDefaultIndex =>
                    "guaranteed by construction; but see finding below",
            }
        ));

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
        serde_json::json!({
            "cli_version": self.cli_version,
            "config": {
                "repo": {"value": self.config.repo.value, "source": self.config.repo.source.to_string()},
                "clone": {"value": self.config.clone.value.display().to_string(), "source": self.config.clone.source.to_string()},
                "index": {"value": self.config.index.value, "source": self.config.index.source.to_string()},
                "collection": {"value": self.config.collection.value, "source": self.config.collection.source.to_string()},
                "api_url": {"value": self.config.api_url.value, "source": self.config.api_url.source.to_string()},
            },
            "backend": match &self.backend {
                BackendMode::Local => serde_json::json!({"mode": "local"}),
                BackendMode::Api(url) => serde_json::json!({"mode": "api", "url": url}),
            },
            "clone": match &self.clone {
                CloneStatus::Absent => serde_json::json!({"present": false}),
                CloneStatus::Present { branch, last_commit_age } => serde_json::json!({
                    "present": true,
                    "branch": branch,
                    "last_commit_relative_age": last_commit_age.map(render_relative_age),
                    "last_commit_age_secs": last_commit_age.map(|age| age.as_secs()),
                }),
            },
            "qmd": match &self.qmd {
                QmdStatus::NotFound => serde_json::json!({"found": false}),
                QmdStatus::Found { version, matches_pin } => serde_json::json!({
                    "found": true,
                    "version": version,
                    "pinned_version": PINNED_QMD_VERSION,
                    "matches_pin": matches_pin,
                }),
            },
            "index": match &self.index {
                IndexStatus::Unavailable { detail } => serde_json::json!({"available": false, "detail": detail}),
                IndexStatus::Available { total_files, vectors_embedded, pending } => serde_json::json!({
                    "available": true,
                    "total_files": total_files,
                    "vectors_embedded": vectors_embedded,
                    "pending": pending,
                }),
            },
            "isolation": match self.isolation {
                IsolationStatus::Verified => "verified",
                IsolationStatus::UnverifiedQmdUnavailable => "unverified_qmd_unavailable",
                IsolationStatus::NameCollisionInDefaultIndex => "name_collision_in_default_index",
            },
            "findings": self.findings().iter().map(|f| serde_json::json!({"message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
            "exit_code": self.exit_code().code(),
        })
    }
}

fn render_backend(backend: &BackendMode) -> String {
    match backend {
        BackendMode::Local => "local".to_string(),
        BackendMode::Api(url) => format!("api ({url})"),
    }
}

pub(crate) fn render_count(value: Option<u64>) -> String {
    value.map_or_else(|| "?".to_string(), |v| v.to_string())
}

/// Opt-in, non-hermetic smoke check against a real `qmd` binary - see the
/// module doc there for why it is not part of the hermetic `tests` module
/// below and how to run it. Declared as a child of this module (rather
/// than in `lib.rs`) because it reuses this module's `parse_qmd_status` and
/// `IndexStatus` to read `qmd status` output, and this is the file that
/// change belongs in.
#[cfg(test)]
mod qmd_contract_check;

#[cfg(test)]
mod tests {
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
}
