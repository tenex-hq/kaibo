//! `kaibo query`: retrieve, never answer. No synthesis, no LLM, no API key,
//! no network call of kaibo's own - this module shells out to `qmd query`
//! and reads markdown already in the local clone; the calling model
//! synthesises.
//!
//! Self-heal runs `sync` unconditionally rather than `--if-stale`: `sync`'s
//! own freshness probe looks only at commit age, so it cannot see a missing
//! collection - see `needs_self_heal`.
//!
//! Retrieved content is data, never instructions: see [`crate::trust`] for
//! the fencing, path-containment, and control-character stripping this
//! module builds on.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
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
use crate::trust;

/// Prefixes qmd parses as structured query syntax, silently changing the
/// search semantics of whatever the caller typed. `query` takes a plain
/// question, so a question that happens to start with one of these is
/// sanitised rather than passed through - see [`sanitize_question`].
const STRUCTURED_QUERY_PREFIXES: [&str; 5] = ["expand:", "lex:", "vec:", "hyde:", "intent:"];

/// The question was empty after stripping every structured-query prefix
/// (e.g. the caller passed exactly `expand:`). A usage mistake to refuse up
/// front, not a query to run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "question is empty after stripping structured-query prefixes \
     (`expand:`, `lex:`, `vec:`, `hyde:`, `intent:`)"
)]
pub struct EmptyQuestion;

impl crate::error::ExitCoded for EmptyQuestion {
    fn exit_code(&self) -> ExitCode {
        ExitCode::Usage
    }
}

/// Strips every leading structured-query prefix, not just one:
/// `expand:lex:x` must reach qmd as `x`, not `lex:x`. Case-insensitive
/// match, but the returned text keeps the caller's own casing.
///
/// Errs with [`EmptyQuestion`] if nothing is left afterwards.
fn sanitize_question(question: &str) -> Result<String, EmptyQuestion> {
    let mut current = question.trim_start().to_string();
    loop {
        let lower = current.to_ascii_lowercase();
        let matched = STRUCTURED_QUERY_PREFIXES
            .iter()
            .find(|prefix| lower.starts_with(*prefix));
        match matched {
            Some(prefix) => {
                current = current[prefix.len()..].trim_start().to_string();
            }
            None => break,
        }
    }
    if current.is_empty() {
        Err(EmptyQuestion)
    } else {
        Ok(current)
    }
}

/// One thing worth telling the user about, with the exact next command
/// where a fix exists. Deliberately its own type, not shared with
/// `status::Finding` / `sync::Finding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

/// Optional frontmatter facets beyond `status`: the two keys the normative
/// schema adds (see [`crate::normative`]). Only `render_json` surfaces
/// these today; `render_text` does not.
///
/// Each is `None` when the key is unset or is not of the type the schema
/// gives it. That matters most for `binding`, which the schema makes a
/// boolean: a page writing a word there binds nothing, and the facet says
/// so rather than passing the word along as if it did. Nothing here
/// validates the schema - `kaibo lint`'s `normative-schema` rule is what
/// tells an author their page is malformed, and it is not `query`'s place
/// to withhold a hit over it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facets {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub binding: Option<bool>,
}

impl Facets {
    fn from_frontmatter(frontmatter: &frontmatter::Frontmatter) -> Facets {
        Facets {
            severity: extra_string_facet(frontmatter, "severity"),
            binding: frontmatter
                .extra
                .get("binding")
                .and_then(serde_yaml_ng::Value::as_bool),
        }
    }
}

/// A facet is corpus content reaching kaibo's output just as much as a
/// title is, so it gets the same control-character stripping `build_hit`
/// applies. `None` if the key is absent or not a plain string.
fn extra_string_facet(frontmatter: &frontmatter::Frontmatter, key: &str) -> Option<String> {
    frontmatter
        .extra
        .get(key)
        .and_then(|value| value.as_str())
        .map(trust::strip_control_chars)
}

/// One retrieved hit. By construction, `path` has already passed
/// containment checks (see `build_hit`) - it names a real file inside the
/// clone, never one reached via `..`, a leading `/`, or an escaping
/// symlink.
///
/// `status` is `None` both for "no status field" and for "frontmatter could
/// not be read" - the two are deliberately not distinguished here (see
/// `gather`, which does, to decide inclusion under `include_drafts`).
///
/// `snippet` is kept as plain content and only fenced at render time (see
/// [`trust::fence`]); nothing here is ever fed into a [`PlannedCommand`]
/// this module builds.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub title: String,
    pub status: Option<Status>,
    pub score: f64,
    pub snippet: String,
    pub facets: Facets,
}

/// The root MOC's domain inventory from `<clone_path>/_index.md`, or why it
/// could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MocInventory {
    Domains(Vec<String>),
    Unavailable { detail: String },
}

/// The result of one `gather` call.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryOutcome {
    /// Self-heal was needed and its sync failed; the real `qmd query` call
    /// never ran. `findings` are `sync`'s own, carried through unchanged.
    SelfHealFailed {
        exit_code: ExitCode,
        findings: Vec<Finding>,
    },
    /// `qmd query` itself failed - a qmd-side problem that `kaibo sync` can
    /// plausibly fix.
    QueryFailed { detail: String },
    /// `qmd query` succeeded but its stdout was not a JSON array of hits at
    /// all - a qmd output-contract violation, not something `sync` fixes. A
    /// single hit failing to parse is a different case: it is skipped, not
    /// reported here (see `gather`).
    UnexpectedOutputShape { detail: String },
    /// The query ran but nothing useful came back: no hits, or every hit
    /// was a draft with `include_drafts` unset. The gap signal.
    NoHits { moc: MocInventory },
    /// At least one hit, already status-filtered and ordered (current
    /// preferred over deprecated).
    Hits(Vec<Hit>),
}

/// What became of every hit `qmd` returned.
///
/// `NoHits` fires on the *filtered* list, so an empty result can mean "the
/// corpus has nothing" or "everything it had was withheld" - and those call
/// for opposite editorial actions, writing a page versus promoting a draft.
/// Without the buckets the two are the same signal. `raw` always equals the
/// sum of the other four, which
/// `every_hit_qmd_returned_is_accounted_for_in_exactly_one_census_bucket`
/// enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HitCensus {
    pub raw: usize,
    /// `qmd` named a file kaibo will not address: outside the clone, or not
    /// a repo-relative path at all.
    pub unaddressable: usize,
    /// Frontmatter could not be parsed, so the page is treated with the same
    /// caution as a draft.
    pub withheld_unverified: usize,
    pub withheld_draft: usize,
    pub kept: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryReport {
    pub question: String,
    pub include_drafts: bool,
    /// `Some` iff self-heal ran - visible even when the rest of the result
    /// looks successful.
    pub self_heal: Option<sync::SyncOutcome>,
    pub outcome: QueryOutcome,
    /// Empty on the paths where `qmd query` never ran.
    pub census: HitCensus,
}

impl QueryReport {
    /// `NoHits` must read as a gap, never a failure - see `AGENTS.md`.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            QueryOutcome::SelfHealFailed { exit_code, .. } => *exit_code,
            QueryOutcome::QueryFailed { .. } => ExitCode::Stale,
            QueryOutcome::UnexpectedOutputShape { .. } => ExitCode::Internal,
            QueryOutcome::NoHits { .. } => ExitCode::NoHits,
            QueryOutcome::Hits(_) => ExitCode::Success,
        }
    }

    pub fn findings(&self) -> Vec<Finding> {
        match &self.outcome {
            QueryOutcome::SelfHealFailed { findings, .. } => findings.clone(),
            QueryOutcome::QueryFailed { detail } => vec![Finding {
                message: format!("qmd query failed: {detail}"),
                fix: Some("re-run `kaibo sync`, then try the query again".to_string()),
            }],
            QueryOutcome::UnexpectedOutputShape { detail } => vec![Finding {
                message: format!("qmd query returned output kaibo could not parse: {detail}"),
                fix: Some(
                    "run `kaibo status` to check qmd's version and health - this is not a stale corpus".to_string(),
                ),
            }],
            QueryOutcome::NoHits { .. } | QueryOutcome::Hits(_) => Vec::new(),
        }
    }
}

/// Bound to a resolved `Config` and a sanitised question - sanitised once,
/// at construction, so `explain` and `gather` cannot disagree about what
/// was asked.
pub struct QueryVerb<'a> {
    config: &'a Config,
    question: String,
}

impl<'a> QueryVerb<'a> {
    /// Errs with [`EmptyQuestion`] (a usage error) if `question` sanitises
    /// away to nothing - see [`sanitize_question`].
    pub fn new(config: &'a Config, question: &str) -> Result<Self, EmptyQuestion> {
        Ok(Self {
            config,
            question: sanitize_question(question)?,
        })
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

/// Self-heal fires if the clone is missing, its last commit predates
/// `status::STALE_THRESHOLD` (or is unreadable), or the configured
/// collection is not listed in the index (or qmd is unreachable to check).
///
/// Deliberately not `sync --if-stale`: this function has already decided
/// work is needed, and `sync`'s own freshness probe looks only at commit
/// age, so a fresh clone with a missing collection would read as fresh to
/// `sync` and self-heal would no-op.
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
        // `if_stale: false` - work is already known to be needed (that is
        // exactly what `needs_self_heal` just decided), so `sync` must not
        // re-check freshness and possibly no-op. See the doc comment on
        // `needs_self_heal` for why `--if-stale` here cannot converge on
        // "collection missing, clone otherwise fresh".
        let sync_report = SyncVerb::new(config).gather(runner, clock, false);
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
                census: HitCensus::default(),
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
                census: HitCensus::default(),
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
                census: HitCensus::default(),
            };
        }
    };

    // Parse the top level as a bare JSON array first, not straight into
    // `Vec<RawHit>`: `serde`'s derived `Vec<T>` deserialization fails the
    // *entire* array if even one element does not match `RawHit`'s shape,
    // which turns one qmd field rename into every query looking like a
    // stale corpus. The array shape itself is still a hard requirement -
    // qmd not returning a JSON array at all is a genuine contract
    // violation, not something a re-sync fixes - so only that outer shape
    // failure is reported; each element is then decoded on its own below,
    // and an element that doesn't match `RawHit` is skipped rather than
    // failing every other hit alongside it.
    let raw_values: Vec<Value> = match serde_json::from_str(&output.stdout) {
        Ok(values) => values,
        Err(err) => {
            return QueryReport {
                question: question.to_string(),
                include_drafts,
                self_heal,
                outcome: QueryOutcome::UnexpectedOutputShape {
                    detail: format!("qmd query did not return a JSON array of hits: {err}"),
                },
                census: HitCensus::default(),
            };
        }
    };

    let raw_hits: Vec<RawHit> = raw_values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<RawHit>(value).ok())
        .collect();

    let mut census = HitCensus {
        raw: raw_hits.len(),
        ..HitCensus::default()
    };
    let mut hits: Vec<Hit> = Vec::new();
    for raw in raw_hits {
        let Some((hit, verified)) = build_hit(config, raw) else {
            census.unaddressable += 1;
            continue;
        };
        if !trust::admits_unverified(verified, include_drafts) {
            census.withheld_unverified += 1;
            continue;
        }
        if !trust::admits_draft_status(&hit.status, include_drafts) {
            census.withheld_draft += 1;
            continue;
        }
        hits.push(hit);
    }
    census.kept = hits.len();
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
        census,
    }
}

/// Fields of one `qmd query --format json` hit this module uses. Unknown
/// fields are ignored, not rejected - this is qmd's output, not a contract
/// this crate controls. `score` defaults to `0.0` so a hit missing a score
/// does not take the rest of the batch down with it; `file` has no default
/// since a hit with no addressable page is not one this crate can use.
#[derive(Debug, Deserialize)]
struct RawHit {
    #[serde(default)]
    score: f64,
    file: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    snippet: String,
}

/// Builds a `Hit` plus whether its frontmatter was actually verified (as
/// opposed to merely absent). `None` if `raw.file` does not resolve to a
/// path this crate will read (see [`trust::repo_relative_path`]).
///
/// `title` and `path` are stripped of control characters before storing,
/// since both print unfenced in `render_text`/`render_json`.
fn build_hit(config: &Config, raw: RawHit) -> Option<(Hit, bool)> {
    let path = trust::repo_relative_path(&raw.file)?;
    let (status, facets, verified) = match read_frontmatter_facts(config, &path) {
        FrontmatterFacts::Parsed { status, facets } => (status, facets, true),
        FrontmatterFacts::Unverified => (None, Facets::default(), false),
    };
    Some((
        Hit {
            path: trust::strip_control_chars(&path),
            title: trust::strip_control_chars(&raw.title),
            status,
            score: raw.score,
            snippet: raw.snippet,
            facets,
        },
        verified,
    ))
}

/// Whether a hit's frontmatter was actually read and parsed, as distinct
/// from "parsed cleanly with no `status` field" - lets a caller treat
/// "could not verify" with the same caution as a draft.
enum FrontmatterFacts {
    Parsed {
        status: Option<Status>,
        facets: Facets,
    },
    Unverified,
}

/// Reads a hit's frontmatter directly off the local clone - a plain file
/// read, not a shelled-out command.
///
/// The path must resolve inside the clone (see
/// [`trust::resolve_contained_path`]). A missing/unreadable file, an
/// escaping path, or frontmatter that fails to parse all degrade to
/// [`FrontmatterFacts::Unverified`] rather than a guessed status.
fn read_frontmatter_facts(config: &Config, repo_relative_path: &str) -> FrontmatterFacts {
    let Some(canonical_full) = trust::resolve_contained_path(config, repo_relative_path) else {
        return FrontmatterFacts::Unverified;
    };

    let Ok(contents) = std::fs::read_to_string(&canonical_full) else {
        return FrontmatterFacts::Unverified;
    };

    match frontmatter::parse(&contents) {
        Ok(doc) => FrontmatterFacts::Parsed {
            status: doc.frontmatter.status.clone(),
            facets: Facets::from_frontmatter(&doc.frontmatter),
        },
        Err(_) => FrontmatterFacts::Unverified,
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
/// `## ` heading names one domain. Headings are stripped of control
/// characters before being printed unfenced.
fn parse_domain_headings(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| {
            line.strip_prefix("## ")
                .map(|s| trust::strip_control_chars(s.trim()))
        })
        .collect()
}

/// A hit's status as the single word rendered in text/JSON. `Unknown`'s
/// payload is a raw frontmatter value round-tripped from a page's own
/// YAML, so it is stripped of control characters before print.
fn status_label(status: &Option<Status>) -> String {
    match status {
        Some(Status::Draft) => "draft".to_string(),
        Some(Status::Current) => "current".to_string(),
        Some(Status::Deprecated) => "deprecated".to_string(),
        Some(Status::Unknown(s)) => trust::strip_control_chars(s),
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
            QueryOutcome::UnexpectedOutputShape { detail } => {
                lines.push(format!("result: unexpected qmd output ({detail})"));
                lines.push("  - next: `kaibo status`".to_string());
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
                    lines.push(trust::fence(&hit.path, &hit.snippet));
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
            QueryOutcome::UnexpectedOutputShape { detail } => serde_json::json!({
                "state": "unexpected_output_shape",
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
                    "snippet": trust::fence(&hit.path, &hit.snippet),
                    "facets": hit.facets,
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
mod tests;
