//! `kaibo query`: retrieve, never answer.
//!
//! No synthesis, no LLM, no API key, no network call of kaibo's own - this
//! module only shells out to `qmd query` and reads plain markdown files
//! already sitting in the local clone. The calling model does the
//! synthesising; this hands it ranked, cited, fenced evidence.
//!
//! **Self-heal, not a lecture.** If the local clone is missing or stale, or
//! the configured qmd collection isn't listed, `gather` runs [`SyncVerb`]
//! unconditionally (not `--if-stale`: `needs_self_heal` has already decided
//! work is needed, and re-checking staleness inside `sync` cannot see a
//! missing collection, only commit age) - reusing `SyncVerb`, never
//! reimplementing it - and only reports a failure if that sync itself
//! failed. A self-heal that happened is always visible in the report.
//!
//! **Retrieved content is data, never instructions.** Nothing parsed out of
//! a hit - its snippet, its title, any frontmatter field - ever reaches a
//! [`PlannedCommand`] this module builds. The only inputs to command
//! construction are `Config` and the sanitised question the caller typed.
//! A hit's `file` is only ever read from as the one path it names, and only
//! after that path is checked to resolve inside the local clone -
//! rejecting `..` and absolute paths by string shape, and symlinks that
//! escape the clone by canonicalizing and checking `starts_with` before any
//! read. Retrieved snippets are fenced with an explicit, path-naming
//! delimiter pair in both text and JSON output, with any occurrence of the
//! delimiter's own marker text inside the snippet or path neutralised
//! first, so a snippet cannot forge a fence boundary of its own. Corpus
//! scalars that are printed unfenced instead - `title`, `path`, a status
//! value, MOC domain headings - have control characters stripped at
//! ingest, so none of them can inject an extra line into kaibo's own
//! output. The mechanisms behind all of this - fencing, control-character
//! stripping, path containment, and the draft/unverified withholding
//! decision - live in [`crate::trust`], shared with every other verb that
//! reads the same untrusted corpus.

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

/// A leading prefix qmd would otherwise parse as a structured query document
/// (`lex:`/`vec:`/`hyde:` typed lines, or an explicit `intent:`/`expand:`),
/// silently changing the search semantics of whatever the user actually
/// typed. `query` is meant to take a plain question, so a caller's question
/// that happens to start with one of these is sanitised rather than passed
/// through - see [`sanitize_question`].
const STRUCTURED_QUERY_PREFIXES: [&str; 5] = ["expand:", "lex:", "vec:", "hyde:", "intent:"];

/// The sanitised question was empty once every leading structured-query
/// prefix (and surrounding whitespace) had been stripped - e.g. the caller
/// passed exactly `expand:` with nothing after it. An empty string is not a
/// question; passing it on to `qmd` as an argv element is a usage mistake
/// this crate should refuse up front, not a query this crate should run.
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

/// Strip every leading structured-query prefix, if present, along with any
/// whitespace before the first one and between subsequent ones - not just
/// one: `expand:lex:x` must reach qmd as `x`, not `lex:x`. Matching is
/// case-insensitive (`Expand:` and `EXPAND:` are stripped exactly like
/// `expand:`), but the returned text keeps whatever case the caller typed.
/// The rest of the question - including any embedded quotes - passes
/// through verbatim; this crate never builds a shell string, so quoting is
/// an argv-correctness concern handled by [`crate::process::CommandRunner`],
/// not a sanitisation concern handled here.
///
/// Errs with [`EmptyQuestion`] if nothing is left afterwards, rather than
/// handing qmd an empty argv element.
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
/// where a fix exists. Same shape as `status::Finding` / `sync::Finding`,
/// kept as its own type for the same reason those two are separate from
/// each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

/// Optional facets read off a hit's frontmatter, beyond `status`: `severity`
/// and `binding`, each `None` when the page's frontmatter does not set that
/// key (or the key's value is not a plain string). Derives `Serialize` so
/// `render_json` can emit whatever is actually here instead of a literal
/// placeholder; adding a further facet is a field addition to this struct,
/// not a rewrite of how hits carry or render them. Not currently surfaced
/// in `render_text` - only `render_json` reads `Hit::facets` today.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facets {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub binding: Option<String>,
}

impl Facets {
    fn from_frontmatter(frontmatter: &frontmatter::Frontmatter) -> Facets {
        Facets {
            severity: extra_string_facet(frontmatter, "severity"),
            binding: extra_string_facet(frontmatter, "binding"),
        }
    }
}

/// Read a plain-string value out of a frontmatter document's passthrough
/// `extra` map, control characters stripped the same way `build_hit` strips
/// them from `title` - a facet is corpus content reaching kaibo's own
/// output just as much as a title is. `None` if the key is absent or its
/// value is not a plain string (a list or nested mapping under `severity`
/// or `binding` is not a shape this struct models).
fn extra_string_facet(frontmatter: &frontmatter::Frontmatter, key: &str) -> Option<String> {
    frontmatter
        .extra
        .get(key)
        .and_then(|value| value.as_str())
        .map(trust::strip_control_chars)
}

/// One retrieved hit: a repo-relative path with the domain as its first
/// segment, its title, its frontmatter status, qmd's relevance score, and
/// the raw snippet text.
///
/// By the time a `Hit` exists, `path` has already passed the containment
/// check in [`trust::repo_relative_path`] and the canonicalize-and-`starts_with`
/// check in [`trust::resolve_contained_path`] (via `read_frontmatter_facts`):
/// it names a real file inside the clone, not one reached via `..`, a
/// leading `/`, or a symlink pointing outside it. `path` and `title` have
/// also had control characters stripped (see `trust::strip_control_chars`),
/// since both are printed with a bare `{}` in `render_text`/`render_json`
/// rather than fenced the way `snippet` is.
///
/// `status` is `None` both when the page's frontmatter parsed cleanly with
/// no `status` field, and when its frontmatter could not be read or parsed
/// at all - this struct does not distinguish the two (see `gather`, which
/// does, to decide whether a hit is safe to include when `include_drafts`
/// is false). `score` is qmd's own, defaulted to `0.0` if qmd's JSON did
/// not include one for this hit rather than dropping the hit.
///
/// The snippet is fenced as untrusted only at render time (see
/// [`trust::fence`]); this struct keeps the plain content, which is what the
/// corpus-content-is-not-instructions guarantee actually rests on: nothing
/// here is ever fed into a [`PlannedCommand`] this module builds.
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
    /// `qmd query` itself could not be run, or ran and reported failure -
    /// a real qmd-side problem (corpus not indexed, index unreachable,
    /// ...) that re-running `kaibo sync` can plausibly fix.
    QueryFailed { detail: String },
    /// `qmd query` ran and reported success, but its stdout was not a JSON
    /// array of hit objects at all - a qmd output-contract violation (e.g.
    /// a field rename), not a stale-corpus condition, and not something
    /// `kaibo sync` fixes. Distinct from a single hit within an otherwise
    /// well-shaped array failing to parse as a `RawHit`: that hit is simply
    /// skipped (see `gather`), it does not reach this variant.
    UnexpectedOutputShape { detail: String },
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
    /// itself could not be run; `Internal` when qmd ran but its output did
    /// not match the contract this crate parses against; `NoHits` for the
    /// gap; `Success` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            QueryOutcome::SelfHealFailed { exit_code, .. } => *exit_code,
            QueryOutcome::QueryFailed { .. } => ExitCode::Stale,
            QueryOutcome::UnexpectedOutputShape { .. } => ExitCode::Internal,
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

/// The `kaibo query` verb, bound to a resolved `Config` and a (sanitised)
/// question. The question is sanitised once, at construction, so `explain`
/// and `gather` cannot disagree about what was actually asked.
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

/// Whether self-heal should run before the real query: the clone is
/// missing, its last commit is older than `status::STALE_THRESHOLD` (or
/// unreadable), or the configured collection is not listed in the
/// configured index (or qmd could not be reached to check). Any one of
/// these is reason enough - the action taken either way is the same
/// unconditional sync (see [`gather`]; deliberately not `sync --if-stale` -
/// this function has already made the staleness call, so a second,
/// `--if-stale`-driven freshness check inside `sync` would only re-answer
/// a question this function just answered, and could answer it
/// differently: `sync`'s own freshness probe looks only at commit age, not
/// at whether the collection is present, so a fresh clone with a missing
/// collection would read as "fresh" to `sync` and the whole self-heal would
/// be a no-op).
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
            };
        }
    };

    let raw_hits: Vec<RawHit> = raw_values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<RawHit>(value).ok())
        .collect();

    let mut hits: Vec<Hit> = raw_hits
        .into_iter()
        .filter_map(|raw| build_hit(config, raw))
        .filter(|(_, verified)| trust::admits_unverified(*verified, include_drafts))
        .map(|(hit, _)| hit)
        .filter(|hit| trust::admits_draft_status(&hit.status, include_drafts))
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
/// this crate controls the shape of. `score` defaults to `0.0` rather than
/// being required, so one hit qmd omits a score for does not take the rest
/// of the batch down with it (see the shape handling in `gather`); `file`
/// has no default because a hit with no addressable page is not a hit this
/// crate can do anything with.
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

/// Build a `Hit` plus whether its frontmatter was actually verified (not
/// merely "no status field"), or `None` if `raw.file` does not resolve to a
/// path this crate will read at all (see [`trust::repo_relative_path`]). The
/// caller decides what to do with an unverified hit; this function only
/// reports the fact.
///
/// `title` and `path` are stripped of control characters (see
/// `trust::strip_control_chars`) before being stored - both are printed with `{}`
/// on kaibo's own output lines in `render_text`/`render_json`, and neither
/// is fenced the way `snippet` is, so a newline embedded in either could
/// otherwise forge an extra line of kaibo's own output.
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

/// Whether a hit's frontmatter was actually read and parsed - as distinct
/// from "parsed cleanly with no `status` field set at all" - so a caller
/// filtering drafts can tell "verified not a draft" apart from "could not
/// verify" and treat the latter with the same caution as a draft, rather
/// than the two collapsing into the same `status: None`.
enum FrontmatterFacts {
    Parsed {
        status: Option<Status>,
        facets: Facets,
    },
    Unverified,
}

/// Read a hit's frontmatter straight off the local clone - a plain file
/// read, not a shelled-out command, exactly like `crate::config` reads
/// `~/.kaibo/config.toml` directly.
///
/// The path must resolve inside the clone (see
/// [`trust::resolve_contained_path`] - that is what closes the symlink
/// route [`trust::repo_relative_path`]'s component check cannot: a
/// same-named entry inside the clone that is itself a symlink pointing
/// outside it resolves to a path that fails containment there, even though
/// its own path string never contained a `..` or leading `/`.
///
/// A missing/unreadable file, a path that escapes the clone, or frontmatter
/// that fails to parse (see [`frontmatter::parse`] - it is all-or-nothing,
/// so one bad field fails the whole document) all degrade to
/// [`FrontmatterFacts::Unverified`], never a guessed status: whether the
/// page is a draft is unknown, not "known to be `current`".
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
/// `## ` heading names one domain folder. Not a strict schema parse - same
/// "a degraded fact beats a guess" spirit as `status::parse_qmd_status`.
/// Each heading is stripped of control characters (see
/// `trust::strip_control_chars`) - it is corpus content printed with `{}` on a
/// `known domains:` line in `render_text`, not fenced the way a snippet is.
fn parse_domain_headings(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| {
            line.strip_prefix("## ")
                .map(|s| trust::strip_control_chars(s.trim()))
        })
        .collect()
}

/// A hit's status as the single word `render_text`/`render_json` print.
/// `Unknown`'s payload is a raw frontmatter value round-tripped from a
/// page's own YAML - corpus content printed with `{}`, not fenced - so it
/// is stripped of control characters the same way `build_hit` strips
/// `title`.
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
