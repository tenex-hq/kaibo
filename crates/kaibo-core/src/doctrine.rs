//! `kaibo doctrine <domain>`: a load, not a question.
//!
//! Where `query` takes a question and returns ranked, cited snippets across
//! the whole corpus, `doctrine` takes a domain name and returns everything
//! an agent needs to load before working in that domain in one call: the
//! domain's own section of the root MOC (owner, topics, summary) plus the
//! full body of every `current` page under that domain's `reference/`
//! folder. No question to formulate, no search to rank - a domain either
//! has doctrine to load or it doesn't.
//!
//! **Self-heal, not a lecture.** Exactly the trigger `query` uses (see
//! `crate::self_heal`): a missing or stale clone, or a configured qmd
//! collection that isn't listed, runs [`SyncVerb`] unconditionally before
//! anything is read, and only reports a failure if that sync itself failed.
//! `doctrine` never calls `qmd query` itself - it reads the clone's
//! markdown directly - but it still depends on the clone being present and
//! reasonably fresh, and on sync having completed at least once.
//!
//! **Unknown domain is a gap, not a failure.** A domain name that matches
//! no heading in the root MOC exits with the gap signal
//! ([`crate::error::ExitCode::NoHits`]), carrying the available domain
//! inventory so a caller can report the gap honestly instead of kaibo
//! inventing one. A domain that *is* known but has no `current` pages under
//! its `reference/` folder is the same gap signal, for the same reason
//! `query`'s "every hit was a draft" is: nothing useful came back. Neither
//! is confused with a broken corpus: the root MOC itself being unreadable
//! is reported as [`crate::error::ExitCode::Stale`] instead, since a broken
//! index or clone must never present as a knowledge gap.
//!
//! **The domain name is data from argv, not a path.** `<domain>` is a
//! positional argument, never a flag naming a repo, clone path, index or
//! collection - see the `no_verb_accepts_a_target_bearing_argument`
//! architecture test in `crates/kaibo/src/main.rs`. But argv is not the
//! only untrusted input here: the root MOC that lists which domain names
//! are "real" is itself corpus content, editable by anyone who can merge a
//! knowledge PR, so a heading cannot be trusted to name a safe, contained
//! path just because it matches. Every path built from a domain name -
//! whether or not that name matched a MOC heading - is validated with
//! [`trust::resolve_contained_path`] before anything is read from it,
//! exactly the canonicalize-and-`starts_with` check `query` already uses
//! for a qmd hit's `file` field. A domain of `..`, or a MOC heading naming
//! one, must not be able to reach a file outside the clone.
//!
//! **Page content is retrieved corpus content: fenced, never instructions.**
//! Each page's body is wrapped with [`trust::fence`] exactly like `query`'s
//! snippets; its title (and the MOC's `owner`/`topics`/`summary` fields,
//! parsed by [`crate::moc`]) are short scalars printed unfenced, so those
//! are control-character-stripped instead, at the point [`crate::moc`] and
//! this module read them, never at render time.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::clock::Clock;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::frontmatter::{self, Status};
use crate::moc::{self, DomainSection};
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::self_heal;
use crate::sync::{self, SyncVerb};
use crate::trust;

/// One thing worth telling the user about, with the exact next command
/// where a fix exists. Same shape as `query::Finding` / `sync::Finding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

/// One `current` (or `deprecated`) page loaded from a domain's
/// `reference/` folder.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctrinePage {
    pub path: String,
    pub title: String,
    pub status: Option<Status>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DoctrineOutcome {
    /// Self-heal was needed and the sync it ran failed; `doctrine` never
    /// even attempted to read a page. `findings` are `sync`'s own.
    SelfHealFailed {
        exit_code: ExitCode,
        findings: Vec<Finding>,
    },
    /// The root MOC itself could not be read (after self-heal, if one
    /// ran) - a broken corpus, not a gap: this crate has no basis to say
    /// whether the requested domain exists at all.
    MocUnavailable { detail: String },
    /// `domain` matched no heading in the root MOC. The gap signal.
    UnknownDomain { available_domains: Vec<String> },
    /// `domain` matched a heading, but no page under its `reference/`
    /// folder was both readable and admitted (verified, not a draft). The
    /// gap signal - `section` is still returned so a caller can report
    /// what the MOC does say about the domain even though there is
    /// nothing to load yet.
    NoCurrentPages { section: DomainSection },
    /// At least one page, current or deprecated.
    Loaded {
        section: DomainSection,
        pages: Vec<DoctrinePage>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DoctrineReport {
    pub domain: String,
    /// `Some` iff self-heal ran.
    pub self_heal: Option<sync::SyncOutcome>,
    pub outcome: DoctrineOutcome,
}

impl DoctrineReport {
    /// `Usage`/`Stale` when self-heal was needed and sync itself failed;
    /// `Stale` when the root MOC could not be read; `NoHits` for either
    /// flavour of gap (unknown domain, or a known domain with nothing
    /// current); `Success` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            DoctrineOutcome::SelfHealFailed { exit_code, .. } => *exit_code,
            DoctrineOutcome::MocUnavailable { .. } => ExitCode::Stale,
            DoctrineOutcome::UnknownDomain { .. } => ExitCode::NoHits,
            DoctrineOutcome::NoCurrentPages { .. } => ExitCode::NoHits,
            DoctrineOutcome::Loaded { .. } => ExitCode::Success,
        }
    }

    /// Findings, each carrying the exact next command where one exists.
    pub fn findings(&self) -> Vec<Finding> {
        match &self.outcome {
            DoctrineOutcome::SelfHealFailed { findings, .. } => findings.clone(),
            DoctrineOutcome::MocUnavailable { detail } => vec![Finding {
                message: format!("root MOC could not be read: {detail}"),
                fix: Some("re-run `kaibo sync`, then try `kaibo doctrine` again".to_string()),
            }],
            DoctrineOutcome::UnknownDomain { .. }
            | DoctrineOutcome::NoCurrentPages { .. }
            | DoctrineOutcome::Loaded { .. } => Vec::new(),
        }
    }
}

/// The `kaibo doctrine` verb, bound to a resolved `Config` and a domain
/// name taken verbatim from argv.
pub struct DoctrineVerb<'a> {
    config: &'a Config,
    domain: String,
}

impl<'a> DoctrineVerb<'a> {
    pub fn new(config: &'a Config, domain: &str) -> Self {
        Self {
            config,
            domain: domain.to_string(),
        }
    }

    /// Run doctrine for real: self-heal if needed, then load.
    pub fn gather(&self, runner: &dyn CommandRunner, clock: &dyn Clock) -> DoctrineReport {
        gather(self.config, &self.domain, runner, clock)
    }
}

impl Explainable for DoctrineVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        // `doctrine` never calls `qmd query` itself; the only commands it
        // could ever run are the self-heal pipeline `query::explain`
        // already describes the same way, for the same reason: explain
        // never has a runner or clock to determine whether self-heal
        // would actually fire, so it always describes the full pipeline.
        SyncVerb::new(self.config).explain()
    }
}

fn gather(
    config: &Config,
    domain: &str,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
) -> DoctrineReport {
    let self_heal_outcome = if self_heal::needs_self_heal(config, runner, clock) {
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
            return DoctrineReport {
                domain: domain.to_string(),
                self_heal: Some(sync_report.outcome),
                outcome: DoctrineOutcome::SelfHealFailed {
                    exit_code: sync_exit,
                    findings,
                },
            };
        }

        Some(sync_report.outcome)
    } else {
        None
    };

    let sections = match moc::read_domain_sections(config.clone_path()) {
        Ok(sections) => sections,
        Err(moc::MocUnreadable { detail }) => {
            return DoctrineReport {
                domain: domain.to_string(),
                self_heal: self_heal_outcome,
                outcome: DoctrineOutcome::MocUnavailable { detail },
            };
        }
    };

    let Some(section) = sections.iter().find(|s| s.name == domain).cloned() else {
        let available_domains = sections.into_iter().map(|s| s.name).collect();
        return DoctrineReport {
            domain: domain.to_string(),
            self_heal: self_heal_outcome,
            outcome: DoctrineOutcome::UnknownDomain { available_domains },
        };
    };

    let pages = load_current_pages(config, domain);

    let outcome = if pages.is_empty() {
        DoctrineOutcome::NoCurrentPages { section }
    } else {
        DoctrineOutcome::Loaded { section, pages }
    };

    DoctrineReport {
        domain: domain.to_string(),
        self_heal: self_heal_outcome,
        outcome,
    }
}

/// Load every `current`/`deprecated` page under `<clone>/<domain>/reference/`,
/// sorted by path for a deterministic order. `domain` is validated with
/// [`trust::resolve_contained_path`] before its `reference/` folder is even
/// listed - and every individual file found underneath is validated again
/// before being read, the same defense-in-depth the module doc explains: a
/// domain matching a MOC heading is not, by itself, a reason to trust the
/// path it builds.
fn load_current_pages(config: &Config, domain: &str) -> Vec<DoctrinePage> {
    let reference_relative = format!("{domain}/reference");
    // Validated for containment before anything is read from it - but the
    // *canonicalized* path this returns is deliberately not what gets
    // walked below: canonicalizing can rewrite the clone root itself (e.g.
    // a `/tmp` that is a symlink to `/private/tmp`), which would make a
    // later `strip_prefix(config.clone_path())` fail for every file found,
    // not just an escaping one. The raw join is what gets walked; this
    // call exists purely to reject an escaping `domain` up front.
    if trust::resolve_contained_path(config, &reference_relative).is_none() {
        return Vec::new();
    }
    let reference_dir_raw = config.clone_path().join(domain).join("reference");

    let mut candidates = Vec::new();
    collect_markdown_files(&reference_dir_raw, &mut candidates);
    candidates.sort();

    let mut pages = Vec::new();
    for full_path in candidates {
        let Ok(repo_relative) = full_path.strip_prefix(config.clone_path()) else {
            continue;
        };
        let repo_relative = repo_relative.to_string_lossy().replace('\\', "/");

        // Re-validated per file, not just for the `reference/` directory
        // as a whole: a file discovered underneath it can itself be a
        // symlink resolving outside the clone, exactly the gap
        // `query::read_frontmatter_facts` closes for qmd hits.
        let Some(canonical) = trust::resolve_contained_path(config, &repo_relative) else {
            continue;
        };
        let Ok(contents) = std::fs::read_to_string(&canonical) else {
            continue;
        };

        let (status, verified, title, body) = match frontmatter::parse(&contents) {
            Ok(doc) => (
                doc.frontmatter.status.clone(),
                true,
                doc.frontmatter.title.clone().unwrap_or_default(),
                doc.body,
            ),
            Err(_) => (None, false, String::new(), String::new()),
        };

        if !trust::admits_unverified(verified, false) {
            continue;
        }
        if !trust::admits_draft_status(&status, false) {
            continue;
        }

        pages.push(DoctrinePage {
            path: trust::strip_control_chars(&repo_relative),
            title: trust::strip_control_chars(&title),
            status,
            body,
        });
    }

    pages
}

/// Recursively collect every `.md` file under `dir`, without following a
/// symlinked directory (`DirEntry::file_type` does not follow symlinks, so
/// a symlinked subdirectory is skipped here rather than walked into - it is
/// a symlinked *file* this module means to exercise the containment check
/// against, not an open-ended symlinked directory tree). A file that is
/// itself a symlink is still collected as a candidate: whether it is safe
/// to read is decided by [`trust::resolve_contained_path`] in
/// [`load_current_pages`], not by this walk.
fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_markdown_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

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
                crate::status::IndexStatus::Available { .. } => "available",
                crate::status::IndexStatus::Unavailable { .. } => "unavailable",
            },
        ),
    }
}

fn render_section_lines(lines: &mut Vec<String>, section: &DomainSection) {
    lines.push(format!("domain: {}", section.name));
    lines.push(format!(
        "owner: {}",
        section.owner.as_deref().unwrap_or("unknown")
    ));
    lines.push(format!(
        "topics: {}",
        if section.topics.is_empty() {
            "none listed".to_string()
        } else {
            section.topics.join(", ")
        }
    ));
    lines.push(format!(
        "summary: {}",
        section.summary.as_deref().unwrap_or("none")
    ));
}

fn section_json(section: &DomainSection) -> Value {
    serde_json::json!({
        "name": section.name,
        "owner": section.owner,
        "topics": section.topics,
        "summary": section.summary,
    })
}

impl Render for DoctrineReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push(format!("kaibo doctrine {:?}", self.domain));

        lines.push(match &self.self_heal {
            None => "self-heal: not needed".to_string(),
            Some(outcome) => format!("self-heal: ran ({})", self_heal_summary(outcome)),
        });

        match &self.outcome {
            DoctrineOutcome::SelfHealFailed { findings, .. } => {
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
            DoctrineOutcome::MocUnavailable { detail } => {
                lines.push(format!("result: moc unavailable ({detail})"));
                lines.push("  - next: `kaibo sync`".to_string());
            }
            DoctrineOutcome::UnknownDomain { available_domains } => {
                lines.push("result: gap, unknown domain".to_string());
                if available_domains.is_empty() {
                    lines.push("known domains: none listed".to_string());
                } else {
                    lines.push(format!("known domains: {}", available_domains.join(", ")));
                }
            }
            DoctrineOutcome::NoCurrentPages { section } => {
                lines.push("result: gap, no current pages".to_string());
                render_section_lines(&mut lines, section);
            }
            DoctrineOutcome::Loaded { section, pages } => {
                lines.push(format!("result: {} page(s)", pages.len()));
                render_section_lines(&mut lines, section);
                for page in pages {
                    let tag = if matches!(page.status, Some(Status::Deprecated)) {
                        " [deprecated]"
                    } else {
                        ""
                    };
                    lines.push(format!(
                        "- {}{} - {} (status {})",
                        page.path,
                        tag,
                        page.title,
                        status_label(&page.status),
                    ));
                    lines.push(trust::fence(&page.path, &page.body));
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
            DoctrineOutcome::SelfHealFailed { findings, .. } => serde_json::json!({
                "state": "self_heal_failed",
                "findings": findings.iter().map(|f| serde_json::json!({"message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
            }),
            DoctrineOutcome::MocUnavailable { detail } => serde_json::json!({
                "state": "moc_unavailable",
                "detail": detail,
            }),
            DoctrineOutcome::UnknownDomain { available_domains } => serde_json::json!({
                "state": "unknown_domain",
                "available_domains": available_domains,
            }),
            DoctrineOutcome::NoCurrentPages { section } => serde_json::json!({
                "state": "no_current_pages",
                "section": section_json(section),
            }),
            DoctrineOutcome::Loaded { section, pages } => serde_json::json!({
                "state": "loaded",
                "section": section_json(section),
                "pages": pages.iter().map(|page| serde_json::json!({
                    "path": page.path,
                    "title": page.title,
                    "status": status_label(&page.status),
                    "body": trust::fence(&page.path, &page.body),
                })).collect::<Vec<_>>(),
            }),
        };

        serde_json::json!({
            "domain": self.domain,
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
