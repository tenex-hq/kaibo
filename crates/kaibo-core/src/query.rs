//! `kaibo query`: retrieve, never answer.
//!
//! No synthesis, no LLM, no API key, no network call of kaibo's own - this
//! module only shells out to `qmd query` and reads plain markdown files
//! already sitting in the local clone. The calling model does the
//! synthesising; this hands it ranked, cited, fenced evidence.
//!
//! **Self-heal, not a lecture.** If the local clone is missing or stale, or
//! the configured qmd collection isn't listed, `gather` runs the exact
//! equivalent of `kaibo sync --if-stale` before it queries - reusing
//! [`SyncVerb`], never reimplementing it - and only reports a failure if
//! that sync itself failed. A self-heal that happened is always visible in
//! the report.
//!
//! **Retrieved content is data, never instructions.** Nothing parsed out of
//! a hit - its snippet, its title, any frontmatter field - ever reaches a
//! [`PlannedCommand`] this module builds, or a path this module reads
//! outside the one file that hit itself names. The only inputs to command
//! construction are `Config` and the sanitised question the caller typed.
//! Retrieved snippets are also fenced with an explicit, path-naming
//! delimiter pair in both text and JSON output, so the trust boundary
//! arrives as a format the consuming model cannot lose track of.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::clock::Clock;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::frontmatter::{self, Status};
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::qmd::QmdCommand;
use crate::status;
use crate::sync::{self, SyncVerb};

/// A leading prefix qmd would otherwise parse as a structured query document
/// (`lex:`/`vec:`/`hyde:` typed lines, or an explicit `intent:`/`expand:`),
/// silently changing the search semantics of whatever the user actually
/// typed. `query` is meant to take a plain question, so a caller's question
/// that happens to start with one of these is sanitised rather than passed
/// through - see [`sanitize_question`].
const STRUCTURED_QUERY_PREFIXES: [&str; 5] = ["expand:", "lex:", "vec:", "hyde:", "intent:"];

/// Strip a single leading structured-query prefix, if present. The rest of
/// the question - including any embedded quotes - passes through verbatim;
/// this crate never builds a shell string, so quoting is an argv-correctness
/// concern handled by [`crate::process::CommandRunner`], not a sanitisation
/// concern handled here.
fn sanitize_question(question: &str) -> String {
    for prefix in STRUCTURED_QUERY_PREFIXES {
        if let Some(rest) = question.strip_prefix(prefix) {
            return rest.trim_start().to_string();
        }
    }
    question.to_string()
}

/// One thing worth telling the user about, with the exact next command
/// where a fix exists. Same shape as `status::Finding` / `sync::Finding`,
/// kept as its own type for the same reason those two are separate from
/// each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

/// Optional facets read off a hit's frontmatter, beyond `status`. Empty
/// today - a door left open for a later change that surfaces `severity` and
/// `binding` here, additively. Adding those fields extends this struct
/// instead of changing `Hit`'s shape or introducing a second "mode" of hit
/// that carries facets and one that doesn't.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {}

impl Facets {
    fn from_frontmatter(_frontmatter: &frontmatter::Frontmatter) -> Facets {
        Facets::default()
    }
}

/// One retrieved hit: a repo-relative path with the domain as its first
/// segment, its title, its frontmatter status (`None` if the page's
/// frontmatter could not be read - an honest gap, not a guess), qmd's
/// relevance score, and the raw snippet text. The snippet is fenced as
/// untrusted only at render time (see [`fence`]) - this struct keeps the
/// plain content, which is what the corpus-content-is-not-instructions
/// guarantee actually rests on: nothing here is ever fed into a
/// [`PlannedCommand`].
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub title: String,
    pub status: Option<Status>,
    pub score: f64,
    pub snippet: String,
    pub facets: Facets,
}

/// The root MOC's domain inventory, read from `<clone_path>/_index.md`, or
/// why it could not be read. Carried by [`QueryOutcome::NoHits`] so a
/// caller can report the gap honestly instead of the CLI inventing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MocInventory {
    Domains(Vec<String>),
    Unavailable { detail: String },
}

/// The result of one `gather` call.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryOutcome {
    /// Self-heal was needed and the sync it ran failed; `query` never even
    /// attempted the real `qmd query` call. `findings` are `sync`'s own,
    /// carried through unchanged - reusing `sync`'s reporting rather than
    /// inventing new prose for the same failure.
    SelfHealFailed {
        exit_code: ExitCode,
        findings: Vec<Finding>,
    },
    /// `qmd query` itself could not be run, or ran and reported failure.
    QueryFailed { detail: String },
    /// The query ran (after self-heal, if one was needed) but nothing
    /// useful came back - either qmd returned no hits at all, or every hit
    /// it returned was a draft and `include_drafts` was not set. The gap
    /// signal.
    NoHits { moc: MocInventory },
    /// At least one hit, already status-filtered and ordered (current
    /// preferred over deprecated).
    Hits(Vec<Hit>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryReport {
    pub question: String,
    pub include_drafts: bool,
    /// `Some` iff self-heal ran - visible proof it happened, not folded
    /// silently into a successful-looking result. Reuses `sync`'s own
    /// outcome type rather than re-describing what sync did in a second
    /// vocabulary.
    pub self_heal: Option<sync::SyncOutcome>,
    pub outcome: QueryOutcome,
}

impl QueryReport {
    /// `Usage`/`Stale` when self-heal was needed and sync itself failed
    /// (whatever `sync` would have exited with); `Stale` when `qmd query`
    /// itself could not be run; `NoHits` for the gap; `Success` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            QueryOutcome::SelfHealFailed { exit_code, .. } => *exit_code,
            QueryOutcome::QueryFailed { .. } => ExitCode::Stale,
            QueryOutcome::NoHits { .. } => ExitCode::NoHits,
            QueryOutcome::Hits(_) => ExitCode::Success,
        }
    }

    /// Findings, each carrying the exact next command where one exists.
    pub fn findings(&self) -> Vec<Finding> {
        match &self.outcome {
            QueryOutcome::SelfHealFailed { findings, .. } => findings.clone(),
            QueryOutcome::QueryFailed { detail } => vec![Finding {
                message: format!("qmd query failed: {detail}"),
                fix: Some("re-run `kaibo sync`, then try the query again".to_string()),
            }],
            QueryOutcome::NoHits { .. } | QueryOutcome::Hits(_) => Vec::new(),
        }
    }
}

/// The `kaibo query` verb, bound to a resolved `Config` and a (sanitised)
/// question. The question is sanitised once, at construction, so `explain`
/// and `gather` cannot disagree about what was actually asked.
pub struct QueryVerb<'a> {
    config: &'a Config,
    question: String,
}

impl<'a> QueryVerb<'a> {
    pub fn new(config: &'a Config, question: &str) -> Self {
        Self {
            config,
            question: sanitize_question(question),
        }
    }

    /// Run query for real: self-heal if needed, then retrieve.
    /// `include_drafts` surfaces draft pages instead of excluding them, each
    /// still labelled as a draft.
    pub fn gather(
        &self,
        runner: &dyn CommandRunner,
        clock: &dyn Clock,
        include_drafts: bool,
    ) -> QueryReport {
        gather(self.config, &self.question, runner, clock, include_drafts)
    }
}

impl Explainable for QueryVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        // `explain` never has a runner or clock to determine whether
        // self-heal would actually fire, so - like `sync::explain` - it
        // always describes the full pipeline: the query itself, plus every
        // command self-heal could run.
        let mut commands = vec![QmdCommand::query(self.config, &self.question)];
        commands.extend(SyncVerb::new(self.config).explain());
        commands
    }
}

fn clone_git_dir(config: &Config) -> PathBuf {
    config.clone_path().join(".git")
}

/// Whether self-heal should run before the real query: the clone is
/// missing, its last commit is older than `status::STALE_THRESHOLD` (or
/// unreadable), or the configured collection is not listed in the
/// configured index (or qmd could not be reached to check). Any one of
/// these is reason enough - the action taken either way is the same
/// (`sync --if-stale`, see [`gather`]).
fn needs_self_heal(config: &Config, runner: &dyn CommandRunner, clock: &dyn Clock) -> bool {
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

fn gather(
    config: &Config,
    question: &str,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
    include_drafts: bool,
) -> QueryReport {
    let self_heal = if needs_self_heal(config, runner, clock) {
        let sync_report = SyncVerb::new(config).gather(runner, clock, true);
        let sync_exit = sync_report.exit_code();

        if matches!(sync_exit, ExitCode::Usage | ExitCode::Stale) {
            let findings = sync_report
                .findings()
                .into_iter()
                .map(|f| Finding {
                    message: f.message,
                    fix: f.fix,
                })
                .collect();
            return QueryReport {
                question: question.to_string(),
                include_drafts,
                self_heal: Some(sync_report.outcome),
                outcome: QueryOutcome::SelfHealFailed {
                    exit_code: sync_exit,
                    findings,
                },
            };
        }

        Some(sync_report.outcome)
    } else {
        None
    };

    let output = match runner.run(&QmdCommand::query(config, question)) {
        Ok(output) if output.success() => output,
        Ok(output) => {
            return QueryReport {
                question: question.to_string(),
                include_drafts,
                self_heal,
                outcome: QueryOutcome::QueryFailed {
                    detail: output.stderr.trim().to_string(),
                },
            };
        }
        Err(err) => {
            return QueryReport {
                question: question.to_string(),
                include_drafts,
                self_heal,
                outcome: QueryOutcome::QueryFailed {
                    detail: err.to_string(),
                },
            };
        }
    };

    let raw_hits: Vec<RawHit> = match serde_json::from_str(&output.stdout) {
        Ok(hits) => hits,
        Err(err) => {
            return QueryReport {
                question: question.to_string(),
                include_drafts,
                self_heal,
                outcome: QueryOutcome::QueryFailed {
                    detail: format!("could not parse qmd query output as JSON: {err}"),
                },
            };
        }
    };

    let mut hits: Vec<Hit> = raw_hits
        .into_iter()
        .map(|raw| build_hit(config, raw))
        .filter(|hit| include_drafts || hit.status != Some(Status::Draft))
        .collect();
    // Stable partition: current (and everything else) first, deprecated
    // last - `sort_by_key` on a bool is stable, so relevance order within
    // each group is preserved.
    hits.sort_by_key(|hit| matches!(hit.status, Some(Status::Deprecated)));

    let outcome = if hits.is_empty() {
        QueryOutcome::NoHits {
            moc: read_moc_inventory(config),
        }
    } else {
        QueryOutcome::Hits(hits)
    };

    QueryReport {
        question: question.to_string(),
        include_drafts,
        self_heal,
        outcome,
    }
}

/// The fields of one element of `qmd query --format json`'s output this
/// module actually uses. Unknown fields (`docid`, `line`, ...) are ignored
/// by default rather than rejected - this is qmd's output, not a contract
/// this crate controls the shape of.
#[derive(Debug, Deserialize)]
struct RawHit {
    score: f64,
    file: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    snippet: String,
}

fn build_hit(config: &Config, raw: RawHit) -> Hit {
    let path = repo_relative_path(&raw.file).unwrap_or(raw.file);
    let (status, facets) = read_frontmatter_facts(config, &path);
    Hit {
        path,
        title: raw.title,
        status,
        score: raw.score,
        snippet: raw.snippet,
        facets,
    }
}

/// Recover the repo-relative path (domain folder first) from qmd's `file`
/// field, e.g. `qmd://knowledge/kaibo/how-to/write-a-good-query.md?index=kaibo`
/// becomes `kaibo/how-to/write-a-good-query.md` - the qmd collection name
/// (`knowledge`) is qmd's own addressing, not part of the corpus's own
/// layout, so it is stripped along with the `qmd://` scheme and the
/// trailing `?index=...` qmd appends.
fn repo_relative_path(file: &str) -> Option<String> {
    let without_query = file.split('?').next().unwrap_or(file);
    let rest = without_query.strip_prefix("qmd://")?;
    let (_, path) = rest.split_once('/')?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Read a hit's frontmatter straight off the local clone - a plain file
/// read, not a shelled-out command, exactly like `crate::config` reads
/// `~/.kaibo/config.toml` directly. An unreadable or unparsable file
/// degrades to `status: None` (rendered as "unknown") rather than excluding
/// the hit or guessing its status.
fn read_frontmatter_facts(config: &Config, repo_relative_path: &str) -> (Option<Status>, Facets) {
    let full_path = config.clone_path().join(repo_relative_path);
    match std::fs::read_to_string(&full_path)
        .ok()
        .and_then(|contents| frontmatter::parse(&contents).ok())
    {
        Some(doc) => (
            doc.frontmatter.status.clone(),
            Facets::from_frontmatter(&doc.frontmatter),
        ),
        None => (None, Facets::default()),
    }
}

fn read_moc_inventory(config: &Config) -> MocInventory {
    let path = config.clone_path().join("_index.md");
    match std::fs::read_to_string(&path) {
        Ok(contents) => MocInventory::Domains(parse_domain_headings(&contents)),
        Err(err) => MocInventory::Unavailable {
            detail: err.to_string(),
        },
    }
}

/// Lenient heading scan for the root MOC's domain sections: each top-level
/// `## ` heading names one domain folder. Not a strict schema parse - same
/// "a degraded fact beats a guess" spirit as `status::parse_qmd_status`.
fn parse_domain_headings(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| line.strip_prefix("## ").map(|s| s.trim().to_string()))
        .collect()
}

fn status_label(status: &Option<Status>) -> String {
    match status {
        Some(Status::Draft) => "draft".to_string(),
        Some(Status::Current) => "current".to_string(),
        Some(Status::Deprecated) => "deprecated".to_string(),
        Some(Status::Unknown(s)) => s.clone(),
        None => "unknown".to_string(),
    }
}

fn self_heal_summary(outcome: &sync::SyncOutcome) -> String {
    match outcome {
        sync::SyncOutcome::SkippedFresh => "already fresh".to_string(),
        sync::SyncOutcome::Stopped(_) => "stopped".to_string(),
        sync::SyncOutcome::Completed {
            clone,
            collection,
            index,
        } => format!(
            "clone {}, collection {}, index {}",
            match clone {
                sync::CloneOutcome::Bootstrapped => "bootstrapped",
                sync::CloneOutcome::Pulled => "pulled",
            },
            match collection {
                sync::CollectionState::AlreadyPresent => "already present",
                sync::CollectionState::Created => "created",
                sync::CollectionState::Unavailable { .. } => "unavailable",
            },
            match index {
                status::IndexStatus::Available { .. } => "available",
                status::IndexStatus::Unavailable { .. } => "unavailable",
            },
        ),
    }
}

/// Wrap retrieved content in an explicit, path-naming delimiter pair so the
/// trust boundary arrives as a format the consuming model cannot lose track
/// of, rather than a discipline it has to maintain. Used identically by
/// both `render_text` and `render_json`, so the fencing can never drift
/// between the two.
fn fence(path: &str, content: &str) -> String {
    format!(
        "<<<UNTRUSTED CORPUS CONTENT path={path:?}>>>\n{content}\n<<<END UNTRUSTED CORPUS CONTENT path={path:?}>>>"
    )
}

impl Render for QueryReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push(format!("kaibo query {:?}", self.question));

        lines.push(match &self.self_heal {
            None => "self-heal: not needed".to_string(),
            Some(outcome) => format!("self-heal: ran ({})", self_heal_summary(outcome)),
        });

        match &self.outcome {
            QueryOutcome::SelfHealFailed { findings, .. } => {
                lines.push("result: self-heal failed".to_string());
                for finding in findings {
                    match &finding.fix {
                        Some(fix) => {
                            lines.push(format!("  - {} -> next: `{fix}`", finding.message))
                        }
                        None => lines.push(format!("  - {}", finding.message)),
                    }
                }
            }
            QueryOutcome::QueryFailed { detail } => {
                lines.push(format!("result: query failed ({detail})"));
                lines.push("  - next: `kaibo sync`".to_string());
            }
            QueryOutcome::NoHits { moc } => {
                lines.push("result: gap, no hits".to_string());
                match moc {
                    MocInventory::Domains(domains) if !domains.is_empty() => {
                        lines.push(format!("known domains: {}", domains.join(", ")));
                    }
                    MocInventory::Domains(_) => {
                        lines.push("known domains: none listed".to_string());
                    }
                    MocInventory::Unavailable { detail } => {
                        lines.push(format!("known domains: unavailable ({detail})"));
                    }
                }
            }
            QueryOutcome::Hits(hits) => {
                lines.push(format!("result: {} hit(s)", hits.len()));
                for hit in hits {
                    let mut tags = String::new();
                    if matches!(hit.status, Some(Status::Draft)) {
                        tags.push_str(" [draft]");
                    }
                    if matches!(hit.status, Some(Status::Deprecated)) {
                        tags.push_str(" [deprecated]");
                    }
                    lines.push(format!(
                        "- {}{} - {} (score {:.2}, status {})",
                        hit.path,
                        tags,
                        hit.title,
                        hit.score,
                        status_label(&hit.status),
                    ));
                    lines.push(fence(&hit.path, &hit.snippet));
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
            QueryOutcome::SelfHealFailed { findings, .. } => serde_json::json!({
                "state": "self_heal_failed",
                "findings": findings.iter().map(|f| serde_json::json!({"message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
            }),
            QueryOutcome::QueryFailed { detail } => serde_json::json!({
                "state": "query_failed",
                "detail": detail,
            }),
            QueryOutcome::NoHits { moc } => serde_json::json!({
                "state": "no_hits",
                "domain_inventory": match moc {
                    MocInventory::Domains(domains) => serde_json::json!({"available": true, "domains": domains}),
                    MocInventory::Unavailable { detail } => serde_json::json!({"available": false, "detail": detail}),
                },
            }),
            QueryOutcome::Hits(hits) => serde_json::json!({
                "state": "hits",
                "hits": hits.iter().map(|hit| serde_json::json!({
                    "path": hit.path,
                    "title": hit.title,
                    "status": status_label(&hit.status),
                    "score": hit.score,
                    "snippet": fence(&hit.path, &hit.snippet),
                    "facets": {},
                })).collect::<Vec<_>>(),
            }),
        };

        serde_json::json!({
            "question": self.question,
            "include_drafts": self.include_drafts,
            "self_heal": self.self_heal.as_ref().map(|outcome| serde_json::json!({
                "ran": true,
                "summary": self_heal_summary(outcome),
            })),
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::clock::testing::FixedClock;
    use crate::config::ConfigSource;
    use crate::config::testing::ConfigBuilder;
    use crate::error::ExitCode;
    use crate::explain::Explainable;
    use crate::frontmatter::Status;
    use crate::process::testing::{FakeCommandRunner, ok};
    use crate::qmd::QmdCommand;
    use crate::sync;

    const NOW_EPOCH: u64 = 1_700_000_000;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
    }

    fn git_dir(clone: &Path) {
        std::fs::create_dir_all(clone.join(".git")).unwrap();
    }

    fn write_page(clone: &Path, repo_relative_path: &str, frontmatter: &str, body: &str) {
        let full = clone.join(repo_relative_path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, format!("---\n{frontmatter}\n---\n{body}\n")).unwrap();
    }

    fn qmd_hit(file: &str, title: &str, score: f64, snippet: &str) -> serde_json::Value {
        json!({
            "docid": "#abc123",
            "score": score,
            "file": file,
            "line": 1,
            "title": title,
            "snippet": snippet,
        })
    }

    fn qmd_query_json(hits: &[serde_json::Value]) -> String {
        serde_json::to_string(hits).unwrap()
    }

    /// A healthy config: clone present and fresh, collection listed - so
    /// `needs_self_heal` reads false and `gather` goes straight to the real
    /// query without touching sync at all.
    fn healthy_fixture(clone: &Path, config: &crate::config::Config) -> FakeCommandRunner {
        FakeCommandRunner::new()
            .on(
                sync_git_last_commit_command(clone),
                ok(format!("{}\n", NOW_EPOCH - 60)),
            )
            .on(
                QmdCommand::collection_list(config),
                ok(format!("{}\n", config.collection())),
            )
    }

    // sync.rs's own `git_last_commit_command` builder is private to that
    // module; `query`'s self-heal probe must issue the exact same command
    // `status`/`sync` use, so this test helper mirrors it exactly rather
    // than reimplementing anything different.
    fn sync_git_last_commit_command(path: &Path) -> crate::explain::PlannedCommand {
        crate::status::git_last_commit_command(path)
    }

    fn config_with_repo(clone: &Path) -> crate::config::Config {
        ConfigBuilder::new(clone)
            .repo("org/corpus", ConfigSource::File)
            .build()
    }

    // --- prefix stripping -----------------------------------------------

    #[test]
    fn strips_expand_prefix() {
        assert_eq!(sanitize_question("expand:what is kaibo"), "what is kaibo");
    }

    #[test]
    fn strips_lex_prefix() {
        assert_eq!(sanitize_question("lex:exact phrase"), "exact phrase");
    }

    #[test]
    fn strips_vec_prefix() {
        assert_eq!(sanitize_question("vec:semantic thing"), "semantic thing");
    }

    #[test]
    fn strips_hyde_prefix() {
        assert_eq!(
            sanitize_question("hyde:hypothetical answer"),
            "hypothetical answer"
        );
    }

    #[test]
    fn strips_intent_prefix() {
        assert_eq!(sanitize_question("intent:find the doc"), "find the doc");
    }

    #[test]
    fn leaves_a_plain_question_untouched() {
        assert_eq!(
            sanitize_question("how does auth work"),
            "how does auth work"
        );
    }

    #[test]
    fn a_question_containing_quotes_survives_sanitisation_verbatim() {
        let question = r#"what does "foo" mean"#;
        assert_eq!(sanitize_question(question), question);
    }

    // --- end-to-end quoting through the runner ---------------------------

    #[test]
    fn a_question_containing_quotes_reaches_the_runner_as_a_single_argv_element() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        let question = r#"what does "foo" mean"#;

        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, question),
            ok(qmd_query_json(&[])),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, question).gather(&runner, &clock, false);

        assert_eq!(report.question, question);
    }

    // --- draft exclusion / inclusion --------------------------------------

    #[test]
    fn draft_pages_are_excluded_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        write_page(
            &clone,
            "kaibo/reference/draft-page.md",
            "status: draft",
            "Draft body.",
        );

        let hits = vec![qmd_hit(
            "qmd://knowledge/kaibo/reference/draft-page.md?index=kaibo",
            "Draft Page",
            0.9,
            "some snippet",
        )];
        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&hits)),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert_eq!(report.exit_code(), ExitCode::NoHits);
        match &report.outcome {
            QueryOutcome::NoHits { .. } => {}
            other => panic!("expected NoHits, got {other:?}"),
        }
    }

    #[test]
    fn include_drafts_flag_surfaces_draft_pages_labelled_as_such() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        write_page(
            &clone,
            "kaibo/reference/draft-page.md",
            "status: draft",
            "Draft body.",
        );

        let hits = vec![qmd_hit(
            "qmd://knowledge/kaibo/reference/draft-page.md?index=kaibo",
            "Draft Page",
            0.9,
            "some snippet",
        )];
        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&hits)),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, true);

        match &report.outcome {
            QueryOutcome::Hits(hits) => {
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0].status, Some(Status::Draft));
            }
            other => panic!("expected Hits, got {other:?}"),
        }
        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("draft"));
    }

    // --- deprecated marking ------------------------------------------------

    #[test]
    fn deprecated_hits_are_shown_but_marked_and_sorted_after_current() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        write_page(
            &clone,
            "kaibo/reference/old-page.md",
            "status: deprecated",
            "Old body.",
        );
        write_page(
            &clone,
            "kaibo/reference/new-page.md",
            "status: current",
            "New body.",
        );

        // Deprecated hit ranks first by raw score; the report must still
        // prefer the current page in the final order.
        let hits = vec![
            qmd_hit(
                "qmd://knowledge/kaibo/reference/old-page.md?index=kaibo",
                "Old Page",
                0.95,
                "old snippet",
            ),
            qmd_hit(
                "qmd://knowledge/kaibo/reference/new-page.md?index=kaibo",
                "New Page",
                0.80,
                "new snippet",
            ),
        ];
        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&hits)),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        match &report.outcome {
            QueryOutcome::Hits(hits) => {
                assert_eq!(hits.len(), 2);
                assert_eq!(hits[0].status, Some(Status::Current));
                assert_eq!(hits[1].status, Some(Status::Deprecated));
            }
            other => panic!("expected Hits, got {other:?}"),
        }

        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("deprecated"));
        let json = report.render_json();
        let hit_statuses: Vec<_> = json["outcome"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["status"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(hit_statuses, vec!["current", "deprecated"]);
    }

    // --- exit 3, no hits, with domain inventory -----------------------------

    #[test]
    fn no_hits_exits_3_and_carries_the_moc_domain_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        std::fs::write(
            clone.join("_index.md"),
            "---\ntype: index\n---\n\n## kaibo\n\nsome text\n\n## observability\n\nmore text\n",
        )
        .unwrap();
        let config = config_with_repo(&clone);

        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&[])),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert_eq!(report.exit_code(), ExitCode::NoHits);
        match &report.outcome {
            QueryOutcome::NoHits {
                moc: MocInventory::Domains(domains),
            } => {
                assert_eq!(
                    domains,
                    &vec!["kaibo".to_string(), "observability".to_string()]
                );
            }
            other => panic!("expected NoHits with domains, got {other:?}"),
        }
    }

    #[test]
    fn no_hits_with_unreadable_moc_still_exits_3_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        // Deliberately no _index.md at all.
        let config = config_with_repo(&clone);

        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&[])),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert_eq!(report.exit_code(), ExitCode::NoHits);
        match &report.outcome {
            QueryOutcome::NoHits {
                moc: MocInventory::Unavailable { .. },
            } => {}
            other => panic!("expected NoHits with an unavailable moc, got {other:?}"),
        }
        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("unavailable"));
    }

    // --- self-heal ----------------------------------------------------------

    #[test]
    fn self_heal_fires_when_the_clone_is_missing_and_the_query_proceeds_after() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        // No clone at all.
        let config = config_with_repo(&clone);

        let runner = FakeCommandRunner::new()
            .on(
                sync_git_clone_command("org/corpus", &clone),
                ok("Cloning...\n"),
            )
            .on(sync_git_status_porcelain_command(&clone), ok(""))
            .on(
                sync_git_checkout_main_command(&clone),
                ok("Switched to branch 'main'\n"),
            )
            .on(sync_git_pull_command(&clone), ok("Already up to date.\n"))
            .on(
                QmdCommand::collection_list(&config),
                ok("No collections found.\n"),
            )
            .on(
                sync_collection_add_command(&config, &clone),
                ok("Collection 'knowledge' created successfully\n"),
            )
            .on(QmdCommand::update(&config), ok("All collections updated.\n"))
            .on(QmdCommand::embed(&config), ok("Done.\n"))
            .on(
                QmdCommand::status(&config),
                ok("QMD Status\n\nDocuments\n  Total:    1 files indexed\n  Vectors:  1 embedded\n"),
            )
            .on(
                QmdCommand::query(&config, "question"),
                ok(qmd_query_json(&[])),
            );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert!(report.self_heal.is_some());
        match &report.self_heal {
            Some(sync::SyncOutcome::Completed { clone, .. }) => {
                assert_eq!(*clone, sync::CloneOutcome::Bootstrapped);
            }
            other => panic!("expected a completed self-heal, got {other:?}"),
        }
        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("self-heal"));
        assert!(!text.contains("not needed"));
    }

    #[test]
    fn self_heal_does_not_fire_when_the_corpus_is_already_healthy() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);

        // Only the freshness probe, the collection-list probe, and the real
        // query are scripted. Any sync-only command (clone/checkout/pull/
        // collection add/update/embed) would panic on "no scripted
        // response", which is exactly the guarantee under test.
        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&[])),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert!(report.self_heal.is_none());
        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("not needed"));
    }

    #[test]
    fn self_heal_failure_is_reported_not_masked() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        // No clone, and no repo configured either - sync stops immediately
        // with RepoNotConfigured, a Usage-exit condition.
        let config = ConfigBuilder::new(&clone).build();

        let runner = FakeCommandRunner::new();
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        assert_eq!(report.exit_code(), ExitCode::Usage);
        match &report.outcome {
            QueryOutcome::SelfHealFailed { .. } => {}
            other => panic!("expected SelfHealFailed, got {other:?}"),
        }
        assert!(!report.findings().is_empty());
    }

    // --- fencing --------------------------------------------------------

    #[test]
    fn retrieved_snippets_are_fenced_as_untrusted_in_text_and_json() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        write_page(
            &clone,
            "kaibo/reference/some-page.md",
            "status: current",
            "Body.",
        );

        let hits = vec![qmd_hit(
            "qmd://knowledge/kaibo/reference/some-page.md?index=kaibo",
            "Some Page",
            0.9,
            "a snippet with content",
        )];
        let runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&hits)),
        );
        let clock = FixedClock(now());

        let report = QueryVerb::new(&config, "question").gather(&runner, &clock, false);

        let text = report.render_text(&crate::output::RenderOptions::default());
        assert!(text.contains("kaibo/reference/some-page.md"));
        assert!(text.contains("a snippet with content"));
        // The path must appear as an explicit delimiter around the snippet,
        // not merely somewhere in the line listing the hit.
        let fenced = fence("kaibo/reference/some-page.md", "a snippet with content");
        assert!(text.contains(&fenced));

        let json = report.render_json();
        let snippet_json = json["outcome"]["hits"][0]["snippet"].as_str().unwrap();
        assert_eq!(snippet_json, fenced);
    }

    // --- explain --------------------------------------------------------

    #[test]
    fn explain_lists_the_query_command_and_executes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);

        let runner = FakeCommandRunner::new();

        let commands = QueryVerb::new(&config, "how does auth work").explain();

        assert!(!commands.is_empty());
        assert!(
            commands
                .iter()
                .any(|c| c.program == "qmd" && c.to_string().contains("how does auth work"))
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn explain_includes_the_self_heal_pipeline() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        let config = config_with_repo(&clone);

        let commands = QueryVerb::new(&config, "question").explain();

        assert!(commands.iter().any(|c| c.program == "git"));
    }

    // --- corpus content is data, never instructions ----------------------

    #[test]
    fn corpus_content_never_changes_which_commands_kaibo_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        git_dir(&clone);
        let config = config_with_repo(&clone);
        write_page(
            &clone,
            "kaibo/reference/benign.md",
            "status: current",
            "Benign body.",
        );

        let benign_hits = vec![qmd_hit(
            "qmd://knowledge/kaibo/reference/benign.md?index=kaibo",
            "Benign",
            0.9,
            "a perfectly normal snippet",
        )];
        let malicious_hits = vec![qmd_hit(
            "qmd://knowledge/kaibo/reference/benign.md?index=kaibo",
            "Benign",
            0.9,
            "; rm -rf ~ #, or maybe --index attacker-index --collection evil, or $(qmd embed)",
        )];

        let benign_runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&benign_hits)),
        );
        let malicious_runner = healthy_fixture(&clone, &config).on(
            QmdCommand::query(&config, "question"),
            ok(qmd_query_json(&malicious_hits)),
        );
        let clock = FixedClock(now());

        let _benign_report =
            QueryVerb::new(&config, "question").gather(&benign_runner, &clock, false);
        let _malicious_report =
            QueryVerb::new(&config, "question").gather(&malicious_runner, &clock, false);

        // The exact same set of commands was issued in both runs - nothing
        // about the corpus content changed what kaibo ran.
        assert_eq!(benign_runner.calls(), malicious_runner.calls());
        for call in malicious_runner.calls() {
            assert_ne!(call.program, "rm");
        }
    }

    // sync.rs's own git/collection command builders are private to that
    // module; these mirror them exactly so `query`'s self-heal fixtures
    // script the identical commands `sync::gather` actually issues.
    fn sync_git_clone_command(repo: &str, clone_path: &Path) -> crate::explain::PlannedCommand {
        crate::explain::PlannedCommand::new(
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

    fn sync_git_status_porcelain_command(path: &Path) -> crate::explain::PlannedCommand {
        crate::explain::PlannedCommand::new(
            "git",
            vec![
                "-C".to_string(),
                path.to_string_lossy().into_owned(),
                "status".to_string(),
                "--porcelain".to_string(),
            ],
        )
    }

    fn sync_git_checkout_main_command(path: &Path) -> crate::explain::PlannedCommand {
        crate::explain::PlannedCommand::new(
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

    fn sync_git_pull_command(path: &Path) -> crate::explain::PlannedCommand {
        crate::explain::PlannedCommand::new(
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

    fn sync_collection_add_command(
        config: &crate::config::Config,
        clone: &Path,
    ) -> crate::explain::PlannedCommand {
        QmdCommand::collection_add(
            config,
            clone,
            config.collection(),
            "*/{reference,how-to,faq}/**/*.md",
        )
    }
}
